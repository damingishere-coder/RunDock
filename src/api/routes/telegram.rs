// @group APIEndpoints : Telegram bot configuration endpoints

use crate::api::error::ApiError;
use crate::config::telegram_config;
use crate::daemon::state::DaemonState;
use crate::telegram::commands;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// @group Types : Request/response structs

#[derive(Serialize)]
struct TelegramConfigResponse {
    enabled: bool,
    /// Token is masked — shows last 4 chars only
    bot_token_hint: Option<String>,
    bot_token_set: bool,
    allowed_chat_ids: Vec<i64>,
    notify_on_crash: bool,
    notify_on_start: bool,
    notify_on_stop: bool,
    notify_on_restart: bool,
}

#[derive(Deserialize)]
struct UpdateTelegramConfig {
    enabled: Option<bool>,
    /// Send empty string to clear; omit to keep existing token
    bot_token: Option<String>,
    allowed_chat_ids: Option<Vec<i64>>,
    notify_on_crash: Option<bool>,
    notify_on_start: Option<bool>,
    notify_on_stop: Option<bool>,
    notify_on_restart: Option<bool>,
}

#[derive(Serialize)]
struct BotInfoResponse {
    ok: bool,
    username: Option<String>,
    first_name: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ValidateBotTokenRequest {
    bot_token: String,
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(get_config).put(update_config))
        .route("/test", post(test_message))
        .route("/botinfo", get(get_bot_info).post(validate_bot_token))
        .with_state(state)
}

fn token_hint(token: &str) -> String {
    let characters: Vec<char> = token.chars().collect();
    if characters.len() <= 4 {
        return "****".to_string();
    }
    let suffix: String = characters.iter().skip(characters.len() - 4).collect();
    format!("****{suffix}")
}

async fn read_telegram_json(mut response: reqwest::Response) -> Result<serde_json::Value, String> {
    const MAX_RESPONSE_BYTES: usize = 64 * 1024;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("Telegram response exceeded the size limit".to_string());
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err("Telegram response exceeded the size limit".to_string());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(error) => return Err(format!("Failed to read Telegram response: {error}")),
        }
    }
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("Telegram returned invalid JSON: {error}"))?;
    if !status.is_success() {
        return Err(parsed["description"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Telegram returned HTTP {status}")));
    }
    Ok(parsed)
}

// @group APIEndpoints > Telegram : GET /telegram — return config with masked token
async fn get_config(
    State(state): State<Arc<DaemonState>>,
) -> Result<Json<TelegramConfigResponse>, ApiError> {
    let cfg = state.telegram.read().await;
    let hint = cfg.bot_token.as_deref().map(token_hint);
    Ok(Json(TelegramConfigResponse {
        enabled: cfg.enabled,
        bot_token_hint: hint,
        bot_token_set: cfg.bot_token.is_some(),
        allowed_chat_ids: cfg.allowed_chat_ids.clone(),
        notify_on_crash: cfg.notify_on_crash,
        notify_on_start: cfg.notify_on_start,
        notify_on_stop: cfg.notify_on_stop,
        notify_on_restart: cfg.notify_on_restart,
    }))
}

// @group APIEndpoints > Telegram : PUT /telegram — update config
async fn update_config(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<UpdateTelegramConfig>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    let mut candidate = state.telegram.read().await.clone();

    if let Some(enabled) = req.enabled {
        candidate.enabled = enabled;
    }
    if let Some(token) = req.bot_token {
        if token.is_empty() {
            candidate.bot_token = None;
        } else {
            candidate.bot_token = Some(token);
        }
    }
    if let Some(ids) = req.allowed_chat_ids {
        candidate.allowed_chat_ids = ids;
    }
    if let Some(v) = req.notify_on_crash {
        candidate.notify_on_crash = v;
    }
    if let Some(v) = req.notify_on_start {
        candidate.notify_on_start = v;
    }
    if let Some(v) = req.notify_on_stop {
        candidate.notify_on_stop = v;
    }
    if let Some(v) = req.notify_on_restart {
        candidate.notify_on_restart = v;
    }

    candidate.normalize();
    candidate
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let candidate_for_save = candidate.clone();
    tokio::task::spawn_blocking(move || telegram_config::save(&candidate_for_save))
        .await
        .map_err(|error| ApiError::internal(format!("Telegram save task failed: {error}")))?
        .map_err(ApiError::from)?;
    *state.telegram.write().await = candidate;

    Ok(Json(serde_json::json!({ "success": true })))
}

// @group APIEndpoints > Telegram : POST /telegram/test — send a test message
async fn test_message(
    State(state): State<Arc<DaemonState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cfg = state.telegram.read().await;

    let token = cfg
        .bot_token
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("Bot token is not configured"))?;

    let chat_id = *cfg.allowed_chat_ids.first().ok_or_else(|| {
        ApiError::bad_request(
            "No allowed chat IDs configured — add at least one chat ID to send test messages",
        )
    })?;

    let token = token.to_string();
    drop(cfg);

    commands::send_message(
        &token,
        chat_id,
        "✅ <b>RunDock</b> Telegram bot is configured and working!",
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(
        serde_json::json!({ "success": true, "message": "Test message sent" }),
    ))
}

// @group APIEndpoints > Telegram : GET /telegram/botinfo — validate saved token and return bot username
async fn get_bot_info(State(state): State<Arc<DaemonState>>) -> Json<BotInfoResponse> {
    let cfg = state.telegram.read().await;
    let token = match cfg.bot_token.as_deref() {
        Some(t) => t.to_string(),
        None => {
            return Json(BotInfoResponse {
                ok: false,
                username: None,
                first_name: None,
                error: Some("No bot token configured".to_string()),
            });
        }
    };
    drop(cfg);

    fetch_bot_info(token).await
}

// @group APIEndpoints > Telegram : POST /telegram/botinfo — validate a candidate token without persisting it
async fn validate_bot_token(Json(req): Json<ValidateBotTokenRequest>) -> Json<BotInfoResponse> {
    if req.bot_token.is_empty()
        || req.bot_token.len() > 256
        || !req.bot_token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Json(BotInfoResponse {
            ok: false,
            username: None,
            first_name: None,
            error: Some("Telegram bot token must be 1-256 visible ASCII characters".to_string()),
        });
    }
    fetch_bot_info(req.bot_token).await
}

async fn fetch_bot_info(token: String) -> Json<BotInfoResponse> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return Json(BotInfoResponse {
                ok: false,
                username: None,
                first_name: None,
                error: Some("Telegram client setup failed".to_string()),
            });
        }
    };
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => match read_telegram_json(resp).await {
            Ok(body) => {
                if body["ok"].as_bool().unwrap_or(false) {
                    Json(BotInfoResponse {
                        ok: true,
                        username: body["result"]["username"].as_str().map(String::from),
                        first_name: body["result"]["first_name"].as_str().map(String::from),
                        error: None,
                    })
                } else {
                    Json(BotInfoResponse {
                        ok: false,
                        username: None,
                        first_name: None,
                        error: body["description"]
                            .as_str()
                            .map(String::from)
                            .or_else(|| Some("Invalid token".to_string())),
                    })
                }
            }
            Err(error) => Json(BotInfoResponse {
                ok: false,
                username: None,
                first_name: None,
                error: Some(error),
            }),
        },
        Err(error) => Json(BotInfoResponse {
            ok: false,
            username: None,
            first_name: None,
            error: Some(format!("Telegram request failed: {}", error.without_url())),
        }),
    }
}
