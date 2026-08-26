// @group APIEndpoints : AI assistant endpoints — settings CRUD, OAuth Device Flow, model listing, streaming chat

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use crate::models::ai::{AiSettings, ChatRequest, DeviceAuthState};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use copilot_client;
use futures::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::mpsc;

fn trusted_provider_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(Into::into)
}
use tokio_stream::wrappers::ReceiverStream;

const MAX_AI_STREAM_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_AI_TOTAL_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_AI_STREAM_DURATION: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_PROVIDER_JSON_BYTES: usize = 1024 * 1024;
const MAX_MODEL_COUNT: usize = 500;
const MIN_DEVICE_FLOW_EXPIRES_SECS: u64 = 60;
const MAX_DEVICE_FLOW_EXPIRES_SECS: u64 = 30 * 60;
const MIN_DEVICE_FLOW_INTERVAL_SECS: u64 = 1;
const MAX_DEVICE_FLOW_INTERVAL_SECS: u64 = 60;

fn device_flow_poll_token_fingerprint(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn valid_device_flow_poll_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_device_flow_response(
    data: &Value,
) -> Result<(String, String, String, u64, u64), ApiError> {
    let device_code = data
        .get("device_code")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 1_024)
        .ok_or_else(|| ApiError::internal("GitHub returned an invalid device_code"))?;
    let user_code = data
        .get("user_code")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .ok_or_else(|| ApiError::internal("GitHub returned an invalid user_code"))?;
    let verification_uri = data
        .get("verification_uri")
        .and_then(Value::as_str)
        .filter(|value| *value == "https://github.com/login/device")
        .ok_or_else(|| ApiError::internal("GitHub returned an untrusted verification URI"))?;
    let expires_in = data
        .get("expires_in")
        .and_then(Value::as_u64)
        .filter(|value| {
            (MIN_DEVICE_FLOW_EXPIRES_SECS..=MAX_DEVICE_FLOW_EXPIRES_SECS).contains(value)
        })
        .ok_or_else(|| ApiError::internal("GitHub returned an invalid Device Flow expiry"))?;
    let interval = data
        .get("interval")
        .and_then(Value::as_u64)
        .filter(|value| {
            (MIN_DEVICE_FLOW_INTERVAL_SECS..=MAX_DEVICE_FLOW_INTERVAL_SECS).contains(value)
        })
        .ok_or_else(|| ApiError::internal("GitHub returned an invalid Device Flow interval"))?;
    Ok((
        device_code.to_string(),
        user_code.to_string(),
        verification_uri.to_string(),
        expires_in,
        interval,
    ))
}

async fn read_bounded_json(mut response: reqwest::Response, label: &str) -> anyhow::Result<Value> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_JSON_BYTES as u64)
    {
        anyhow::bail!("{label} response exceeded the size limit");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| anyhow::anyhow!("failed to read {label} response: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_PROVIDER_JSON_BYTES {
            anyhow::bail!("{label} response exceeded the size limit");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|error| anyhow::anyhow!("failed to parse {label} response: {error}"))
}

fn append_bounded_stream_chunk(buffer: &mut BytesMut, chunk: &[u8]) -> anyhow::Result<()> {
    if buffer.len().saturating_add(chunk.len()) > MAX_AI_STREAM_BUFFER_BYTES {
        anyhow::bail!("AI provider returned an oversized streaming event");
    }
    buffer.extend_from_slice(chunk);
    Ok(())
}

fn take_stream_line(buffer: &mut BytesMut) -> anyhow::Result<Option<String>> {
    let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') else {
        return Ok(None);
    };
    let mut line = buffer.split_to(newline + 1);
    line.truncate(newline);
    if line.last() == Some(&b'\r') {
        line.truncate(line.len() - 1);
    }
    String::from_utf8(line.to_vec())
        .map(Some)
        .map_err(|_| anyhow::anyhow!("AI provider returned a non-UTF-8 streaming event"))
}

async fn yield_during_large_stream(line_count: &mut usize) {
    *line_count = line_count.saturating_add(1);
    if (*line_count).is_multiple_of(256) {
        tokio::task::yield_now().await;
    }
}

async fn send_bounded_delta(
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
    delta: &str,
    total_output_bytes: &mut usize,
) -> anyhow::Result<()> {
    let next_total = total_output_bytes
        .checked_add(delta.len())
        .ok_or_else(|| anyhow::anyhow!("AI provider output size overflowed"))?;
    if next_total > MAX_AI_TOTAL_OUTPUT_BYTES {
        anyhow::bail!("AI provider output exceeded the total size limit");
    }
    *total_output_bytes = next_total;
    tx.send(Ok(Bytes::from(format!(
        "data: {}\n\n",
        json!({ "delta": delta })
    ))))
    .await
    .map_err(|_| anyhow::anyhow!("AI client disconnected"))
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/settings", get(get_settings).put(save_settings))
        .route("/chat", post(chat))
        .route("/auth/start", post(auth_start))
        .route("/auth/status", get(auth_status))
        .route("/auth", delete(auth_logout))
        .route("/models", get(list_models))
        .with_state(state)
}

// @group Configuration : Path to ai-settings.json
fn settings_path() -> std::path::PathBuf {
    crate::config::paths::data_dir().join("ai-settings.json")
}

/// Client ID baked in at compile time via GH_OAUTH_CLIENT_ID env var (optional).
const BUILTIN_CLIENT_ID: Option<&str> = option_env!("GH_OAUTH_CLIENT_ID");

// @group Utilities > AI : Missing is first-run; corruption must stay visible.
fn load_settings_blocking() -> Result<AiSettings, ApiError> {
    let path = settings_path();
    let mut settings: AiSettings = crate::config::atomic_file::load_json_with_backup_validated(
        &path,
        |candidate: &AiSettings| {
            validate_settings(candidate).map_err(|error| anyhow::anyhow!(error.message))
        },
    )
    .map_err(|error| ApiError::internal(format!("AI settings are unreadable: {error}")))?;
    if settings.client_id.is_empty() {
        if let Some(id) = BUILTIN_CLIENT_ID {
            settings.client_id = id.to_string();
        }
    }
    Ok(settings)
}

async fn load_settings() -> Result<AiSettings, ApiError> {
    tokio::task::spawn_blocking(load_settings_blocking)
        .await
        .map_err(|error| ApiError::internal(format!("AI settings read task failed: {error}")))?
}

// @group Utilities > AI : Persist AI settings to disk
fn persist_settings_blocking(settings: &AiSettings) -> Result<(), ApiError> {
    let path = settings_path();
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        settings,
        |candidate: &AiSettings| {
            validate_settings(candidate).map_err(|error| anyhow::anyhow!(error.message))
        },
    )
    .map_err(|e| ApiError::internal(format!("write error: {e}")))?;
    Ok(())
}

async fn persist_settings(settings: &AiSettings) -> Result<(), ApiError> {
    let settings = settings.clone();
    tokio::task::spawn_blocking(move || persist_settings_blocking(&settings))
        .await
        .map_err(|error| ApiError::internal(format!("AI settings write task failed: {error}")))?
}

fn validate_settings(settings: &AiSettings) -> Result<(), ApiError> {
    use crate::utils::outbound::{validate_url, OutboundPolicy};

    if !matches!(
        settings.provider.as_str(),
        "ollama" | "github" | "copilot" | "claude" | "openai"
    ) {
        return Err(ApiError::bad_request("unsupported AI provider"));
    }
    if settings.model.trim().is_empty() || settings.model.len() > 200 {
        return Err(ApiError::bad_request(
            "AI model must contain 1 to 200 characters",
        ));
    }
    if !settings.client_id.is_empty()
        && (settings.client_id.len() > 128
            || !settings.client_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            }))
    {
        return Err(ApiError::bad_request(
            "GitHub client ID must contain 1 to 128 letters, numbers, '.', '_' or '-' characters",
        ));
    }
    for (label, secret) in [
        ("GitHub token", &settings.github_token),
        ("Anthropic key", &settings.anthropic_key),
        ("OpenAI key", &settings.openai_key),
    ] {
        if secret.len() > 8_192 {
            return Err(ApiError::bad_request(format!("{label} is too long")));
        }
    }

    let openai = validate_url(&settings.openai_base_url, OutboundPolicy::PublicHttps)
        .map_err(|error| ApiError::bad_request(format!("invalid OpenAI base URL: {error}")))?;
    if openai.query().is_some() || openai.fragment().is_some() {
        return Err(ApiError::bad_request(
            "OpenAI base URL cannot contain a query string or fragment",
        ));
    }
    let ollama = validate_url(&settings.ollama_base_url, OutboundPolicy::LoopbackHttp)
        .map_err(|error| ApiError::bad_request(format!("invalid Ollama base URL: {error}")))?;
    if ollama.query().is_some() || ollama.fragment().is_some() {
        return Err(ApiError::bad_request(
            "Ollama base URL cannot contain a query string or fragment",
        ));
    }

    if settings.enabled {
        let credential_missing = match settings.provider.as_str() {
            "github" => settings.github_token.is_empty(),
            "claude" => settings.anthropic_key.is_empty(),
            "openai" => settings.openai_key.is_empty(),
            _ => false,
        };
        if credential_missing {
            return Err(ApiError::bad_request(
                "the selected AI provider requires a configured credential",
            ));
        }
    }
    Ok(())
}

fn secret_hint(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    let characters: Vec<char> = token.chars().collect();
    if characters.len() <= 8 {
        return "****".to_string();
    }
    let prefix: String = characters.iter().take(4).collect();
    let suffix: String = characters.iter().skip(characters.len() - 4).collect();
    format!("{prefix}…{suffix}")
}

// @group APIEndpoints > AI : GET /ai/settings — load persisted AI config
async fn get_settings() -> Result<Json<Value>, ApiError> {
    let s = load_settings().await?;

    Ok(Json(json!({
        "provider":          s.provider,
        "enabled":           s.enabled,
        "model":             s.model,
        // GitHub
        "github_token_set":  !s.github_token.is_empty(),
        "github_token_hint": secret_hint(&s.github_token),
        "github_username":   s.github_username,
        "client_id_set":     !s.client_id.is_empty(),
        "client_id_builtin": BUILTIN_CLIENT_ID.is_some(),
        // Claude
        "anthropic_key_set": !s.anthropic_key.is_empty(),
        "anthropic_key_hint": secret_hint(&s.anthropic_key),
        // OpenAI
        "openai_key_set":    !s.openai_key.is_empty(),
        "openai_key_hint":   secret_hint(&s.openai_key),
        "openai_base_url":   s.openai_base_url,
        // Ollama
        "ollama_base_url":   s.ollama_base_url,
    })))
}

// @group APIEndpoints > AI : PUT /ai/settings — persist AI config (partial update, empty strings ignored for secrets)
async fn save_settings(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    let mut s = load_settings().await?;

    if let Some(v) = body.get("provider").and_then(|v| v.as_str()) {
        s.provider = v.to_string()
    }
    if let Some(v) = body.get("model").and_then(|v| v.as_str()) {
        s.model = v.to_string()
    }
    if let Some(v) = body.get("enabled").and_then(|v| v.as_bool()) {
        s.enabled = v
    }

    // GitHub
    if let Some(v) = body.get("github_token").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.github_token = v.to_string()
        }
    }
    if let Some(v) = body.get("client_id").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.client_id = v.to_string()
        }
    }
    // Claude
    if let Some(v) = body.get("anthropic_key").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.anthropic_key = v.to_string()
        }
    }
    if body
        .get("clear_anthropic_key")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        s.anthropic_key.clear();
    }
    // OpenAI
    if let Some(v) = body.get("openai_key").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.openai_key = v.to_string()
        }
    }
    if body
        .get("clear_openai_key")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        s.openai_key.clear();
    }
    if let Some(v) = body.get("openai_base_url").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.openai_base_url = v.to_string()
        }
    }
    // Ollama
    if let Some(v) = body.get("ollama_base_url").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            s.ollama_base_url = v.to_string()
        }
    }

    validate_settings(&s)?;
    persist_settings(&s).await?;
    Ok(Json(json!({ "success": true })))
}

// @group APIEndpoints > AI : POST /ai/auth/start — begin GitHub Device Flow
async fn auth_start(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let settings = load_settings().await?;

    if settings.client_id.is_empty() {
        return Err(ApiError::bad_request(
            "No GitHub OAuth App Client ID configured. Add one in Settings → AI Assistant.",
        ));
    }

    let _permit = Arc::clone(&state.ai_auth_limit)
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "too many concurrent GitHub Device Flow requests".into(),
        })?;

    let client = trusted_provider_client().map_err(ApiError::from)?;
    let resp = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .json(&json!({
            "client_id": settings.client_id,
            "scope": "read:user",
        }))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("GitHub request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(ApiError::internal(format!(
            "GitHub Device Flow request failed with HTTP {status}"
        )));
    }

    let data = read_bounded_json(resp, "GitHub Device Flow").await?;

    let (device_code, user_code, verification_uri, expires_in, interval) =
        parse_device_flow_response(&data)?;

    let flow_id = uuid::Uuid::new_v4().to_string();
    let poll_token = crate::config::auth_config::generate_token();
    let mut flows = state.ai_device_auth.lock().await;
    flows.retain(|_, flow| flow.expires_at > Utc::now());
    if flows.len() >= 8 {
        return Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "too many active GitHub Device Flow requests".into(),
        });
    }
    flows.insert(
        flow_id.clone(),
        DeviceAuthState {
            device_code,
            user_code: user_code.clone(),
            verification_uri: verification_uri.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(expires_in as i64),
            interval_secs: interval,
            last_poll_at: None,
            poll_attempts: 0,
            poll_token_fingerprint: device_flow_poll_token_fingerprint(&poll_token),
        },
    );
    drop(flows);

    Ok(Json(json!({
        "flow_id": flow_id,
        "poll_token": poll_token,
        "user_code": user_code,
        "verification_uri": verification_uri,
        "expires_in": expires_in,
        "interval": interval,
    })))
}

// @group APIEndpoints > AI : GET /ai/auth/status — poll GitHub token exchange
#[derive(serde::Deserialize)]
struct AuthStatusQuery {
    flow_id: String,
}

async fn auth_status(
    State(state): State<Arc<DaemonState>>,
    Query(query): Query<AuthStatusQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    if uuid::Uuid::parse_str(&query.flow_id).is_err() {
        return Err(ApiError::bad_request("invalid Device Flow identifier"));
    }
    let poll_token = headers
        .get("x-rundock-device-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::bad_request("Device Flow polling credential is missing"))?;
    if !valid_device_flow_poll_token(poll_token) {
        return Err(ApiError::bad_request(
            "invalid Device Flow polling credential",
        ));
    }
    let poll_token_fingerprint = device_flow_poll_token_fingerprint(poll_token);
    let settings = load_settings().await?;
    let now = Utc::now();
    let auth = {
        let mut flows = state.ai_device_auth.lock().await;
        let Some(current) = flows.get_mut(&query.flow_id) else {
            return Ok(Json(json!({ "status": "idle" })));
        };
        if current.poll_token_fingerprint != poll_token_fingerprint {
            return Err(ApiError::unauthorized(
                "Device Flow polling credential does not own this request",
            ));
        }
        if current.poll_attempts >= 240 {
            flows.remove(&query.flow_id);
            return Ok(Json(json!({
                "status": "error",
                "message": "GitHub Device Flow polling budget exhausted"
            })));
        }
        if current.last_poll_at.is_some_and(|last| {
            now < last + chrono::Duration::seconds(current.interval_secs as i64)
        }) {
            return Ok(Json(
                json!({ "status": "pending", "interval": current.interval_secs }),
            ));
        }
        current.last_poll_at = Some(now);
        current.poll_attempts = current.poll_attempts.saturating_add(1);
        current.clone()
    };

    if Utc::now() >= auth.expires_at {
        clear_device_auth_if_current(&state, &query.flow_id, &auth.device_code).await;
        return Ok(Json(json!({ "status": "expired" })));
    }

    let _permit = Arc::clone(&state.ai_auth_limit)
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "too many concurrent GitHub Device Flow polls".into(),
        })?;

    let client = trusted_provider_client().map_err(ApiError::from)?;
    let resp = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&json!({
            "client_id":   settings.client_id,
            "device_code": auth.device_code,
            "grant_type":  "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("GitHub poll request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(ApiError::internal(format!(
            "GitHub Device Flow poll failed with HTTP {status}"
        )));
    }

    let data = read_bounded_json(resp, "GitHub Device Flow poll").await?;

    if let Some(error) = data["error"].as_str() {
        match error {
            "authorization_pending" => {
                return Ok(Json(
                    json!({ "status": "pending", "interval": auth.interval_secs }),
                ))
            }
            "slow_down" => {
                let mut guard = state.ai_device_auth.lock().await;
                let i = if let Some(current) = guard
                    .get_mut(&query.flow_id)
                    .filter(|current| current.device_code == auth.device_code)
                {
                    current.interval_secs = current
                        .interval_secs
                        .saturating_add(5)
                        .min(MAX_DEVICE_FLOW_INTERVAL_SECS);
                    current.interval_secs
                } else {
                    return Ok(Json(json!({ "status": "idle" })));
                };
                return Ok(Json(json!({ "status": "pending", "interval": i })));
            }
            "expired_token" => {
                clear_device_auth_if_current(&state, &query.flow_id, &auth.device_code).await;
                return Ok(Json(json!({ "status": "expired" })));
            }
            "access_denied" => {
                clear_device_auth_if_current(&state, &query.flow_id, &auth.device_code).await;
                return Ok(Json(json!({ "status": "denied"  })));
            }
            other => {
                clear_device_auth_if_current(&state, &query.flow_id, &auth.device_code).await;
                return Ok(Json(json!({ "status": "error", "message": other })));
            }
        }
    }

    if let Some(token) = data["access_token"].as_str() {
        let token = token.to_string();
        let username = fetch_github_username(&token).await.map_err(|error| {
            ApiError::internal(format!("GitHub identity lookup failed: {error}"))
        })?;
        let mut auth_guard = state.ai_device_auth.lock().await;
        if !auth_guard
            .get(&query.flow_id)
            .is_some_and(|current| current.device_code == auth.device_code)
        {
            return Ok(Json(json!({ "status": "idle" })));
        }
        let _config_guard = state.config_mutation_lock.lock().await;
        let mut new_settings = load_settings().await?;
        new_settings.github_token = token;
        new_settings.github_username = username.clone();
        persist_settings(&new_settings).await?;
        auth_guard.remove(&query.flow_id);
        return Ok(Json(json!({ "status": "complete", "username": username })));
    }

    Ok(Json(
        json!({ "status": "pending", "interval": auth.interval_secs }),
    ))
}

async fn clear_device_auth_if_current(state: &DaemonState, flow_id: &str, device_code: &str) {
    let mut guard = state.ai_device_auth.lock().await;
    if guard
        .get(flow_id)
        .is_some_and(|current| current.device_code == device_code)
    {
        guard.remove(flow_id);
    }
}

// @group APIEndpoints > AI : DELETE /ai/auth — disconnect GitHub account
async fn auth_logout(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    // Keep the same lock order as auth_status: device state, then config.
    let mut auth_guard = state.ai_device_auth.lock().await;
    let _config_guard = state.config_mutation_lock.lock().await;
    let mut settings = load_settings().await?;
    settings.github_token = String::new();
    settings.github_username = String::new();
    persist_settings(&settings).await?;
    auth_guard.clear();
    Ok(Json(json!({ "success": true })))
}

#[derive(serde::Deserialize)]
struct ListModelsQuery {
    provider: Option<String>,
}

// @group APIEndpoints > AI : GET /ai/models — list models without mutating settings
async fn list_models(Query(query): Query<ListModelsQuery>) -> Result<Json<Value>, ApiError> {
    let mut s = load_settings().await?;
    if let Some(provider) = query.provider {
        if !matches!(
            provider.as_str(),
            "copilot" | "github" | "claude" | "openai" | "ollama"
        ) {
            return Err(ApiError::bad_request("Unknown AI provider"));
        }
        s.provider = provider;
    }
    let models = match s.provider.as_str() {
        "copilot" => list_copilot_models(&s).await?,
        "github" => list_github_models(&s).await?,
        "claude" => list_claude_models(),
        "openai" => list_openai_models(&s).await?,
        "ollama" => list_ollama_models(&s).await?,
        other => return Err(ApiError::bad_request(format!("Unknown provider: {other}"))),
    };
    Ok(Json(json!({ "models": models })))
}

// @group Utilities > AI > Copilot : Exchange GitHub PAT for a short-lived Copilot API token
async fn get_copilot_api_token(github_token: &str) -> anyhow::Result<String> {
    let client = trusted_provider_client()?;
    let resp = client
        .get("https://api.github.com/copilot_internal/v2/token")
        .header("Authorization", format!("Token {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "alter-pm2")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Copilot token request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            anyhow::bail!("GitHub Copilot is not active on this account, or your token has insufficient permissions.");
        }
        anyhow::bail!("Failed to get Copilot API token: HTTP {status}");
    }

    let data = read_bounded_json(resp, "Copilot token").await?;
    data["token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Copilot token response missing 'token' field"))
}

// @group Utilities > AI > Copilot : Resolve GitHub token — stored token first, then gh CLI config
fn resolve_github_token(stored: &str) -> anyhow::Result<String> {
    if !stored.is_empty() {
        return Ok(stored.to_string());
    }
    copilot_client::get_github_token().map_err(|_| {
        anyhow::anyhow!(
            "No GitHub token found. Sign in via Settings → AI Assistant (GitHub provider) \
             or ensure GitHub CLI / VS Code Copilot is installed."
        )
    })
}

// @group Utilities > AI > Copilot : List models from GitHub Copilot API
async fn list_copilot_models(s: &AiSettings) -> Result<Vec<Value>, ApiError> {
    let github_token =
        resolve_github_token(&s.github_token).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let copilot_token = get_copilot_api_token(&github_token)
        .await
        .map_err(|e| ApiError::internal(format!("GitHub Copilot unavailable: {e}")))?;
    let client = trusted_provider_client().map_err(ApiError::from)?;
    let response = client
        .get("https://api.githubcopilot.com/models")
        .header("Authorization", format!("Bearer {copilot_token}"))
        .header("Accept", "application/json")
        .header("Editor-Version", "alter/1.0.0")
        .header("Editor-Plugin-Version", "alter/1.0.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .header("User-Agent", "alter-pm2")
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to fetch Copilot models: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::internal(format!(
            "Failed to fetch Copilot models: HTTP {}",
            response.status()
        )));
    }
    let payload = read_bounded_json(response, "Copilot models")
        .await
        .map_err(ApiError::from)?;
    let models = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::internal("Copilot models response did not contain a data array")
        })?;

    Ok(models
        .iter()
        .take(MAX_MODEL_COUNT)
        .map(|m| {
            let id = m.get("id").and_then(Value::as_str).unwrap_or_default();
            let label = m
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            json!({ "id": id, "label": label, "publisher": "GitHub Copilot" })
        })
        .filter(|model| model["id"].as_str().is_some_and(|id| !id.is_empty()))
        .collect())
}

// @group BusinessLogic > AI > Copilot : Stream chat via GitHub Copilot API (OpenAI-compatible SSE)
async fn stream_copilot(
    github_token: String,
    model: String,
    messages: Vec<Value>,
    tx: mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> anyhow::Result<()> {
    let copilot_token = get_copilot_api_token(&github_token).await?;

    let client = trusted_provider_client()?;
    let resp = client
        .post("https://api.githubcopilot.com/chat/completions")
        .header("Authorization",         format!("Bearer {copilot_token}"))
        .header("Content-Type",          "application/json")
        .header("Accept",                "application/json")
        .header("Editor-Version",        "alter/1.0.0")
        .header("Editor-Plugin-Version", "alter/1.0.0")
        .header("Copilot-Integration-Id","vscode-chat")
        .header("User-Agent",            "alter-pm2")
        .json(&json!({ "model": model, "messages": messages, "stream": true, "max_tokens": 1024, "temperature": 0.7 }))
        .send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = if status.as_u16() == 401 || status.as_u16() == 403 {
            "GitHub Copilot subscription required or token expired. Re-authenticate in Settings → AI Assistant.".to_string()
        } else if status.as_u16() == 429 {
            "GitHub Copilot rate limit hit. Please wait a moment.".to_string()
        } else {
            format!("Copilot API request failed with HTTP {status}")
        };
        anyhow::bail!("{msg}");
    }

    // Reuse the OpenAI-compat SSE parser — Copilot uses the same delta format
    let mut stream = resp.bytes_stream();
    let mut buf = BytesMut::new();
    let mut total_output_bytes = 0;
    let mut line_count = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        append_bounded_stream_chunk(&mut buf, &chunk)?;
        while let Some(line) = take_stream_line(&mut buf)? {
            yield_during_large_stream(&mut line_count).await;
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }
                let v: Value = serde_json::from_str(data).map_err(|error| {
                    anyhow::anyhow!("Copilot returned invalid SSE JSON: {error}")
                })?;
                if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                    if !delta.is_empty() {
                        send_bounded_delta(&tx, delta, &mut total_output_bytes).await?;
                    }
                }
            }
        }
    }
    if !buf.iter().all(u8::is_ascii_whitespace) {
        anyhow::bail!("Copilot ended with an unterminated SSE event");
    }
    Ok(())
}

// @group Utilities > AI > GitHub : Fetch GitHub Models catalog
async fn list_github_models(s: &AiSettings) -> Result<Vec<Value>, ApiError> {
    if s.github_token.is_empty() {
        return Err(ApiError::bad_request(
            "No GitHub token. Sign in via Settings → AI Assistant.",
        ));
    }
    let client = trusted_provider_client().map_err(ApiError::from)?;
    let resp = client
        .get("https://models.github.ai/catalog/models")
        .header("Authorization", format!("Bearer {}", s.github_token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("GitHub Models catalog request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(ApiError::internal(format!(
            "GitHub Models catalog request failed with HTTP {status}"
        )));
    }

    let catalog = read_bounded_json(resp, "GitHub Models catalog").await?;

    let catalog = catalog.as_array().ok_or_else(|| {
        ApiError::internal("GitHub Models catalog returned an unexpected response schema")
    })?;
    let models = catalog
        .iter()
        .filter(|m| {
            m["task"]
                .as_str()
                .map(|t| t.contains("chat") || t.contains("completion"))
                .unwrap_or(false)
                || m["capabilities"]["chat_completion"]
                    .as_bool()
                    .unwrap_or(false)
                || m["supported_languages"].is_array()
        })
        .take(MAX_MODEL_COUNT)
        .map(|m| {
            let id = m["id"]
                .as_str()
                .or_else(|| m["name"].as_str())
                .unwrap_or("")
                .to_string();
            let label = m["friendly_name"]
                .as_str()
                .or_else(|| m["display_name"].as_str())
                .or_else(|| m["name"].as_str())
                .unwrap_or(&id)
                .to_string();
            let publisher = m["publisher"].as_str().unwrap_or("").to_string();
            json!({ "id": id, "label": label, "publisher": publisher })
        })
        .filter(|m| !m["id"].as_str().unwrap_or("").is_empty())
        .collect();
    Ok(models)
}

// @group Utilities > AI > Claude : Hardcoded current Anthropic models
fn list_claude_models() -> Vec<Value> {
    vec![
        json!({ "id": "claude-opus-4-6",            "label": "Claude Opus 4.6",            "publisher": "Anthropic" }),
        json!({ "id": "claude-sonnet-4-6",           "label": "Claude Sonnet 4.6",           "publisher": "Anthropic" }),
        json!({ "id": "claude-haiku-4-5-20251001",   "label": "Claude Haiku 4.5",            "publisher": "Anthropic" }),
        json!({ "id": "claude-3-5-sonnet-20241022",  "label": "Claude 3.5 Sonnet",           "publisher": "Anthropic" }),
        json!({ "id": "claude-3-5-haiku-20241022",   "label": "Claude 3.5 Haiku",            "publisher": "Anthropic" }),
        json!({ "id": "claude-3-opus-20240229",      "label": "Claude 3 Opus",               "publisher": "Anthropic" }),
    ]
}

// @group Utilities > AI > OpenAI : Fetch available chat models from OpenAI-compatible endpoint
async fn list_openai_models(s: &AiSettings) -> Result<Vec<Value>, ApiError> {
    use crate::utils::outbound::{client_for_url, validate_url, OutboundPolicy};
    if s.openai_key.is_empty() {
        return Err(ApiError::bad_request(
            "No OpenAI API key. Add one in Settings → AI Assistant.",
        ));
    }
    let base = s.openai_base_url.trim_end_matches('/');
    let url = validate_url(&format!("{base}/models"), OutboundPolicy::PublicHttps)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let client = client_for_url(&url, OutboundPolicy::PublicHttps)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", s.openai_key))
        .send()
        .await
        .map_err(|e| {
            ApiError::internal(format!("OpenAI models request failed: {}", e.without_url()))
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(ApiError::internal(format!("OpenAI models error {status}")));
    }

    let data = read_bounded_json(resp, "OpenAI models").await?;

    let chat_prefixes = ["gpt-", "o1", "o3", "chatgpt"];
    let model_data = data["data"].as_array().ok_or_else(|| {
        ApiError::internal("OpenAI models endpoint returned an unexpected response schema")
    })?;
    let models = model_data
        .iter()
        .take(MAX_MODEL_COUNT)
        .filter(|m| {
            let id = m["id"].as_str().unwrap_or("");
            chat_prefixes.iter().any(|p| id.starts_with(p))
        })
        .map(|m| {
            let id = m["id"].as_str().unwrap_or("").to_string();
            json!({ "id": id.clone(), "label": id, "publisher": "OpenAI" })
        })
        .collect();
    Ok(models)
}

// @group Utilities > AI > Ollama : Fetch locally installed models from Ollama
async fn list_ollama_models(s: &AiSettings) -> Result<Vec<Value>, ApiError> {
    use crate::utils::outbound::{client_for_url, validate_url, OutboundPolicy};

    let base = s.ollama_base_url.trim_end_matches('/');
    let url = validate_url(&format!("{base}/api/tags"), OutboundPolicy::LoopbackHttp)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let client = client_for_url(&url, OutboundPolicy::LoopbackHttp)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let resp = client.get(url).send().await.map_err(|e| {
        ApiError::internal(format!(
            "Ollama request failed — is Ollama running? {}",
            e.without_url()
        ))
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(ApiError::internal(format!("Ollama tags error {status}")));
    }

    let data = read_bounded_json(resp, "Ollama tags").await?;

    let model_data = data["models"].as_array().ok_or_else(|| {
        ApiError::internal("Ollama tags endpoint returned an unexpected response schema")
    })?;
    let models = model_data
        .iter()
        .take(MAX_MODEL_COUNT)
        .map(|m| {
            let id = m["name"].as_str().unwrap_or("").to_string();
            json!({ "id": id.clone(), "label": id, "publisher": "Ollama" })
        })
        .collect();
    Ok(models)
}

// @group BusinessLogic > AI : POST /ai/chat — streaming SSE response, dispatches per provider
async fn chat(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, ApiError> {
    let settings = load_settings().await?;

    if req.message.trim().is_empty() || req.message.len() > 16_384 {
        return Err(ApiError::bad_request(
            "message must contain 1 to 16384 bytes",
        ));
    }
    if req.history.len() > 50
        || req
            .history
            .iter()
            .any(|message| message.content.len() > 16_384)
        || req
            .history
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            > 65_536
    {
        return Err(ApiError::bad_request("chat history is too large"));
    }
    validate_settings(&settings)?;

    if !settings.enabled {
        return Err(ApiError::bad_request(
            "AI assistant is disabled. Enable it in Settings → AI Assistant.",
        ));
    }

    let provider = req.provider.clone().unwrap_or(settings.provider.clone());
    let model = req.model.clone().unwrap_or(settings.model.clone());
    if !matches!(
        provider.as_str(),
        "ollama" | "github" | "copilot" | "claude" | "openai"
    ) {
        return Err(ApiError::bad_request("unsupported AI provider"));
    }
    if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "model must contain 1 to 200 printable characters",
        ));
    }

    let permit = Arc::clone(&state.ai_stream_limit)
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many concurrent AI requests; wait for one to finish".into(),
        })?;
    let system_content =
        super::ai_context::build_system_prompt(&state, req.process_id.as_deref()).await;
    let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": system_content })];
    for msg in &req.history {
        messages.push(json!({ "role": msg.role, "content": msg.content }))
    }
    messages.push(json!({ "role": "user", "content": req.message }));

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::convert::Infallible>>(64);
    // @group BusinessLogic > AI : Validate provider credentials before spawning stream task
    match provider.as_str() {
        "copilot" => {
            // Resolve token now so we can return a friendly error before spawning the stream
            resolve_github_token(&settings.github_token)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
        }
        "github" if settings.github_token.is_empty() => {
            return Err(ApiError::bad_request(
                "No GitHub token. Sign in via Settings → AI Assistant.",
            ))
        }
        "claude" if settings.anthropic_key.is_empty() => {
            return Err(ApiError::bad_request(
                "No Anthropic API key. Add one in Settings → AI Assistant.",
            ))
        }
        "openai" if settings.openai_key.is_empty() => {
            return Err(ApiError::bad_request(
                "No OpenAI API key. Add one in Settings → AI Assistant.",
            ))
        }
        _ => {}
    }
    let github_token = settings.github_token.clone();
    let anthropic_key = settings.anthropic_key.clone();
    let openai_key = settings.openai_key.clone();
    let openai_base = settings.openai_base_url.clone();
    let ollama_base = settings.ollama_base_url.clone();

    tokio::spawn(async move {
        let _permit = permit;
        let operation = async {
            match provider.as_str() {
                "copilot" => match resolve_github_token(&github_token) {
                    Ok(tok) => stream_copilot(tok, model, messages, tx.clone()).await,
                    Err(e) => Err(anyhow::anyhow!("{e}")),
                },
                "github" => {
                    stream_openai_compat(
                        github_token,
                        "https://models.github.ai/inference/chat/completions".to_string(),
                        model,
                        messages,
                        tx.clone(),
                    )
                    .await
                }
                "claude" => stream_claude(anthropic_key, model, messages, tx.clone()).await,
                "openai" => {
                    let base = openai_base.trim_end_matches('/').to_string();
                    stream_openai_compat(
                        openai_key,
                        format!("{base}/chat/completions"),
                        model,
                        messages,
                        tx.clone(),
                    )
                    .await
                }
                "ollama" => {
                    let base = ollama_base.trim_end_matches('/').to_string();
                    stream_ollama(base, model, messages, tx.clone()).await
                }
                other => Err(anyhow::anyhow!("Unknown provider: {other}")),
            }
        };
        let cancellation = tx.clone();
        let result = tokio::select! {
            result = tokio::time::timeout(MAX_AI_STREAM_DURATION, operation) => {
                result.unwrap_or_else(|_| Err(anyhow::anyhow!(
                    "AI stream exceeded the five-minute time limit"
                )))
            },
            _ = cancellation.closed() => return,
        };
        let success = result.is_ok();
        if let Err(e) = result {
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "data: {}\n\n",
                    json!({ "error": e.to_string() })
                ))))
                .await;
        }
        let _ = tx
            .send(Ok(Bytes::from(format!(
                "data: {}\n\n",
                json!({ "done": true, "ok": success })
            ))))
            .await;
    });

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "text/event-stream".parse().unwrap());
    headers.insert("Cache-Control", "no-cache".parse().unwrap());
    headers.insert("X-Accel-Buffering", "no".parse().unwrap());
    Ok((
        StatusCode::OK,
        headers,
        axum::body::Body::from_stream(ReceiverStream::new(rx)),
    )
        .into_response())
}

// @group BusinessLogic > AI > OpenAI-compat : Stream deltas from GitHub Models or OpenAI (same SSE format)
async fn stream_openai_compat(
    token: String,
    endpoint: String,
    model: String,
    messages: Vec<Value>,
    tx: mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> anyhow::Result<()> {
    use crate::utils::outbound::{client_for_url, validate_url, OutboundPolicy};

    let endpoint = validate_url(&endpoint, OutboundPolicy::PublicHttps)?;
    let client = client_for_url(&endpoint, OutboundPolicy::PublicHttps).await?;
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&json!({ "model": model, "messages": messages, "stream": true, "max_tokens": 1024, "temperature": 0.7 }))
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("AI request failed: {}", error.without_url()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = if status.as_u16() == 429 {
            "Rate limit hit. Please wait a moment before sending another message.".to_string()
        } else if status.as_u16() == 401 {
            "API token rejected. Check your credentials in Settings → AI Assistant.".to_string()
        } else if status.as_u16() == 403 {
            "The AI provider rejected this request. Check account access, quota, and model permissions.".to_string()
        } else {
            format!("AI provider request failed with HTTP {status}")
        };
        anyhow::bail!("{msg}");
    }

    let mut stream = resp.bytes_stream();
    let mut buf = BytesMut::new();
    let mut total_output_bytes = 0;
    let mut line_count = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        append_bounded_stream_chunk(&mut buf, &chunk)?;
        while let Some(line) = take_stream_line(&mut buf)? {
            yield_during_large_stream(&mut line_count).await;
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }
                let v: Value = serde_json::from_str(data).map_err(|error| {
                    anyhow::anyhow!("AI provider returned invalid SSE JSON: {error}")
                })?;
                if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                    if !delta.is_empty() {
                        send_bounded_delta(&tx, delta, &mut total_output_bytes).await?;
                    }
                }
            }
        }
    }
    if !buf.iter().all(u8::is_ascii_whitespace) {
        anyhow::bail!("AI provider ended with an unterminated SSE event");
    }
    Ok(())
}

// @group BusinessLogic > AI > Claude : Stream deltas from Anthropic Messages API
async fn stream_claude(
    api_key: String,
    model: String,
    messages: Vec<Value>,
    tx: mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> anyhow::Result<()> {
    // Separate system message from the rest
    let system_content = messages
        .first()
        .filter(|m| m["role"].as_str() == Some("system"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    let chat_messages: Vec<Value> = messages.iter().skip(1).cloned().collect();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&json!({
            "model": model,
            "max_tokens": 1024,
            "system": system_content,
            "messages": chat_messages,
            "stream": true,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = if status.as_u16() == 401 {
            "Anthropic API key invalid. Check your key in Settings → AI Assistant.".to_string()
        } else if status.as_u16() == 429 {
            "Anthropic rate limit hit. Please wait before sending another message.".to_string()
        } else {
            format!("Anthropic API request failed with HTTP {status}")
        };
        anyhow::bail!("{msg}");
    }

    let mut stream = resp.bytes_stream();
    let mut buf = BytesMut::new();
    let mut total_output_bytes = 0;
    let mut line_count = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        append_bounded_stream_chunk(&mut buf, &chunk)?;
        while let Some(line) = take_stream_line(&mut buf)? {
            yield_during_large_stream(&mut line_count).await;
            if let Some(data) = line.strip_prefix("data: ") {
                let v: Value = serde_json::from_str(data).map_err(|error| {
                    anyhow::anyhow!("Anthropic returned invalid SSE JSON: {error}")
                })?;
                if v["type"].as_str() == Some("content_block_delta") {
                    if let Some(text) = v["delta"]["text"].as_str() {
                        if !text.is_empty() {
                            send_bounded_delta(&tx, text, &mut total_output_bytes).await?;
                        }
                    }
                }
            }
        }
    }
    if !buf.iter().all(u8::is_ascii_whitespace) {
        anyhow::bail!("Anthropic ended with an unterminated SSE event");
    }
    Ok(())
}

// @group BusinessLogic > AI > Ollama : Stream deltas from local Ollama instance (NDJSON)
async fn stream_ollama(
    base_url: String,
    model: String,
    messages: Vec<Value>,
    tx: mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> anyhow::Result<()> {
    use crate::utils::outbound::{client_for_url, validate_url, OutboundPolicy};
    // Many small local models (Gemma, Llama, Mistral) ignore both role:system in the messages array
    // and the top-level `system` field. The most reliable approach is to inject the system context
    // directly into the first user message so the model actually sees and uses it.
    let system_content = messages
        .first()
        .filter(|m| m["role"].as_str() == Some("system"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();

    // Build chat messages without the system entry, injecting context into the first user turn
    let mut chat_messages: Vec<Value> = messages
        .iter()
        .filter(|m| m["role"].as_str() != Some("system"))
        .cloned()
        .collect();

    // Inject context into the LAST user message so it's always in the model's immediate window
    if !system_content.is_empty() {
        if let Some(last_user) = chat_messages
            .iter_mut()
            .rfind(|m| m["role"].as_str() == Some("user"))
        {
            let original = last_user["content"].as_str().unwrap_or("").to_string();
            last_user["content"] = serde_json::Value::String(format!(
                "[Context]\n{system_content}\n\n[Question]\n{original}"
            ));
        }
    }

    let endpoint = validate_url(
        &format!("{base_url}/api/chat"),
        OutboundPolicy::LoopbackHttp,
    )?;
    let client = client_for_url(&endpoint, OutboundPolicy::LoopbackHttp).await?;
    let resp = client
        .post(endpoint)
        .json(&json!({
            "model": model,
            "messages": chat_messages,
            "stream": true,
            "options": { "num_predict": 8192 }
        }))
        .send()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Ollama request failed — is Ollama running? {}",
                e.without_url()
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("Ollama request failed with HTTP {status}");
    }

    let mut stream = resp.bytes_stream();
    let mut buf = BytesMut::new();
    let mut total_output_bytes = 0;
    let mut line_count = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        append_bounded_stream_chunk(&mut buf, &chunk)?;
        while let Some(line) = take_stream_line(&mut buf)? {
            yield_during_large_stream(&mut line_count).await;
            if line.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .map_err(|error| anyhow::anyhow!("Ollama returned invalid NDJSON: {error}"))?;
            if let Some(text) = v["message"]["content"].as_str() {
                if !text.is_empty() {
                    send_bounded_delta(&tx, text, &mut total_output_bytes).await?;
                }
            }
            if v["done"].as_bool().unwrap_or(false) {
                return Ok(());
            }
        }
    }
    if !buf.iter().all(u8::is_ascii_whitespace) {
        anyhow::bail!("Ollama ended with an unterminated NDJSON record");
    }
    Ok(())
}

// @group Utilities > AI > GitHub : Fetch the authenticated user's GitHub username
async fn fetch_github_username(token: &str) -> anyhow::Result<String> {
    let client = trusted_provider_client()?;
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "alter-pm2")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub user lookup failed with HTTP {}", resp.status());
    }
    let data = read_bounded_json(resp, "GitHub user").await?;
    data["login"]
        .as_str()
        .filter(|login| !login.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("GitHub user response is missing login"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_buffer_rejects_oversized_provider_event() {
        let mut buffer = BytesMut::new();
        append_bounded_stream_chunk(&mut buffer, &[b'x'; 1024]).unwrap();
        let oversized = vec![b'y'; MAX_AI_STREAM_BUFFER_BYTES];
        assert!(append_bounded_stream_chunk(&mut buffer, &oversized).is_err());
        assert_eq!(buffer.len(), 1024);
    }

    #[test]
    fn streaming_lines_are_consumed_without_copying_the_remainder() {
        let mut buffer = BytesMut::from("first\r\nsecond\npartial");
        assert_eq!(
            take_stream_line(&mut buffer).unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(
            take_stream_line(&mut buffer).unwrap().as_deref(),
            Some("second")
        );
        assert!(take_stream_line(&mut buffer).unwrap().is_none());
        assert_eq!(&buffer[..], b"partial");
    }

    #[tokio::test]
    async fn streaming_delta_rejects_total_output_over_limit() {
        let (tx, _rx) = mpsc::channel(1);
        let mut total = MAX_AI_TOTAL_OUTPUT_BYTES;
        assert!(send_bounded_delta(&tx, "x", &mut total).await.is_err());
        assert_eq!(total, MAX_AI_TOTAL_OUTPUT_BYTES);
    }

    #[test]
    fn secret_hints_are_utf8_safe() {
        assert_eq!(secret_hint("密钥甲乙丙丁戊己庚辛壬"), "密钥甲乙…己庚辛壬");
        assert_eq!(secret_hint("短密钥"), "****");
    }

    #[test]
    fn device_flow_response_requires_bounded_trusted_fields() {
        let valid = json!({
            "device_code": "device-secret",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        assert!(parse_device_flow_response(&valid).is_ok());

        let mut zero_interval = valid.clone();
        zero_interval["interval"] = json!(0);
        assert!(parse_device_flow_response(&zero_interval).is_err());
        let mut huge_expiry = valid.clone();
        huge_expiry["expires_in"] = json!(u64::MAX);
        assert!(parse_device_flow_response(&huge_expiry).is_err());
        let mut untrusted_uri = valid;
        untrusted_uri["verification_uri"] = json!("https://example.test/device");
        assert!(parse_device_flow_response(&untrusted_uri).is_err());
    }

    #[test]
    fn device_flow_poll_credentials_are_strict_hex_tokens() {
        assert!(valid_device_flow_poll_token(&"a".repeat(64)));
        assert!(!valid_device_flow_poll_token(&"a".repeat(63)));
        assert!(!valid_device_flow_poll_token(&format!(
            "{}g",
            "a".repeat(63)
        )));
    }
}
