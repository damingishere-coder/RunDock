// @group Authentication : Auth endpoints -- login, logout, PIN, change-password, lock settings

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{atomic::Ordering, Arc};

use crate::{
    api::error::ApiError,
    config::auth_config::{self, generate_token, AuthConfig},
    daemon::state::DaemonState,
};

const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_ACTIVE_SESSIONS: usize = 64;
const MAX_STREAM_TICKETS: usize = 256;

fn validate_password_length(password: &str) -> Result<(), ApiError> {
    if password.len() < 8 || password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::bad_request(
            "Password must contain 8 to 1024 bytes",
        ));
    }
    Ok(())
}

async fn acquire_auth_verification(
    state: &Arc<DaemonState>,
) -> Result<tokio::sync::OwnedSemaphorePermit, ApiError> {
    state
        .auth_verify_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Authentication capacity is busy; retry shortly".into(),
        })
}

async fn verify_password_blocking(
    state: &Arc<DaemonState>,
    config: AuthConfig,
    password: String,
) -> Result<bool, ApiError> {
    let _permit = acquire_auth_verification(state).await?;
    tokio::task::spawn_blocking(move || config.verify_password(&password))
        .await
        .map_err(|error| ApiError::internal(format!("authentication worker failed: {error}")))
}

async fn verify_pin_blocking(
    state: &Arc<DaemonState>,
    config: AuthConfig,
    pin: String,
) -> Result<bool, ApiError> {
    let _permit = acquire_auth_verification(state).await?;
    tokio::task::spawn_blocking(move || config.verify_pin(&pin))
        .await
        .map_err(|error| ApiError::internal(format!("authentication worker failed: {error}")))
}

async fn set_password_and_persist(
    mut candidate: AuthConfig,
    password: String,
) -> Result<AuthConfig, ApiError> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<AuthConfig> {
        candidate.set_password(&password)?;
        auth_config::save(&candidate)?;
        Ok(candidate)
    })
    .await
    .map_err(|error| ApiError::internal(format!("authentication worker failed: {error}")))?
    .map_err(|error| {
        tracing::error!(%error, "failed to persist password settings");
        ApiError::internal("failed to persist authentication settings")
    })
}

async fn set_pin_and_persist(
    mut candidate: AuthConfig,
    pin: String,
) -> Result<AuthConfig, ApiError> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<AuthConfig> {
        candidate.set_pin(&pin)?;
        auth_config::save(&candidate)?;
        Ok(candidate)
    })
    .await
    .map_err(|error| ApiError::internal(format!("authentication worker failed: {error}")))?
    .map_err(|error| {
        tracing::error!(%error, "failed to persist PIN settings");
        ApiError::internal("failed to persist authentication settings")
    })
}

async fn persist_auth(candidate: AuthConfig) -> Result<AuthConfig, ApiError> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<AuthConfig> {
        auth_config::save(&candidate)?;
        Ok(candidate)
    })
    .await
    .map_err(|error| ApiError::internal(format!("authentication worker failed: {error}")))?
    .map_err(|error| {
        tracing::error!(%error, "failed to persist authentication settings");
        ApiError::internal("failed to persist authentication settings")
    })
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/setup", post(setup_password))
        .route("/login", post(login))
        .route("/pin/login", post(pin_login))
        .route("/session", get(validate_session).delete(logout))
        .route("/password", delete(disable_password))
        .route("/change-password", post(change_password))
        .route("/pin", post(set_pin).delete(remove_pin))
        .route("/settings", patch(update_settings))
        .with_state(state)
}

// @group Authentication > Status : Report whether a password / PIN is configured
#[derive(Serialize)]
struct AuthStatus {
    password_configured: bool,
    pin_configured: bool,
    lock_timeout_mins: Option<u32>,
}

async fn get_status(State(state): State<Arc<DaemonState>>) -> Json<AuthStatus> {
    let auth = state.auth.read().await;
    Json(AuthStatus {
        password_configured: auth.password_hash.is_some(),
        pin_configured: auth.pin_hash.is_some(),
        lock_timeout_mins: auth.lock_timeout_mins,
    })
}

// @group Authentication > Setup : First-time password setup (only works once)
#[derive(Deserialize)]
struct SetupRequest {
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    session_token: String,
    expires_at: String,
}

async fn setup_password(
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if !crate::api::middleware::is_direct_loopback_request(peer, &headers, state.config.port) {
        return Err(ApiError::unauthorized(
            "Initial password setup is only available from a direct local connection",
        ));
    }
    validate_password_length(&body.password)?;
    let _config_guard = state.config_mutation_lock.lock().await;
    let session_guard = state.session_lock.lock().await;
    let candidate = state.auth.read().await.clone();
    if candidate.password_hash.is_some() {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: "Password already configured".into(),
        });
    }
    let candidate = set_password_and_persist(candidate, body.password).await?;
    *state.auth.write().await = candidate;
    let generation = state.auth_generation.fetch_add(1, Ordering::AcqRel) + 1;
    drop(session_guard);

    let (token, expires_at) = create_session(&state, generation).await?;
    Ok(Json(LoginResponse {
        session_token: token,
        expires_at: expires_at.to_rfc3339(),
    }))
}

// @group Authentication > Login : Password-based login -- returns session token
#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

async fn login(
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    enforce_login_rate_limit(&state, peer.ip()).await?;
    if body.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::unauthorized("Invalid password"));
    }
    let generation = state.auth_generation.load(Ordering::Acquire);
    let auth = state.auth.read().await.clone();
    if !verify_password_blocking(&state, auth, body.password).await? {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid password".into(),
        });
    }
    let (token, expires_at) = create_session(&state, generation).await?;
    Ok(Json(LoginResponse {
        session_token: token,
        expires_at: expires_at.to_rfc3339(),
    }))
}

// @group Authentication > PIN Login : PIN-based login (quick unlock / lock screen)
#[derive(Deserialize)]
struct PinLoginRequest {
    pin: String,
}

async fn pin_login(
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<PinLoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    enforce_login_rate_limit(&state, peer.ip()).await?;
    if !matches!(body.pin.len(), 4 | 6) || !body.pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ApiError::unauthorized("Invalid PIN"));
    }
    let generation = state.auth_generation.load(Ordering::Acquire);
    let auth = state.auth.read().await.clone();
    if !verify_pin_blocking(&state, auth, body.pin).await? {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid PIN".into(),
        });
    }
    let (token, expires_at) = create_session(&state, generation).await?;
    Ok(Json(LoginResponse {
        session_token: token,
        expires_at: expires_at.to_rfc3339(),
    }))
}

#[derive(Deserialize)]
pub(crate) struct StreamTicketRequest {
    path: String,
    query: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct StreamTicketResponse {
    ticket: String,
    expires_at: String,
}

pub(crate) async fn create_stream_ticket(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(body): Json<StreamTicketRequest>,
) -> Result<Json<StreamTicketResponse>, ApiError> {
    if !is_allowed_stream_path(&body.path) {
        return Err(ApiError::bad_request("unsupported stream ticket path"));
    }
    if body.query.as_ref().is_some_and(|query| {
        query.len() > 1_024
            || query.contains('#')
            || query.split('&').any(|pair| pair.starts_with("ticket="))
    }) {
        return Err(ApiError::bad_request("unsupported stream ticket query"));
    }

    let now = Utc::now();
    let _ticket_guard = state.stream_ticket_lock.lock().await;
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed before the stream ticket was issued",
        ));
    }
    state
        .stream_tickets
        .retain(|_, ticket| ticket.expires_at > now);
    if state.stream_tickets.len() >= MAX_STREAM_TICKETS {
        return Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many active stream tickets; retry after existing tickets expire".into(),
        });
    }
    let ticket = generate_token();
    let expires_at = now + Duration::seconds(30);
    state.stream_tickets.insert(
        ticket.clone(),
        crate::daemon::state::StreamTicket {
            method: axum::http::Method::GET,
            path: body.path,
            query: body.query.filter(|query| !query.is_empty()),
            expires_at,
        },
    );
    Ok(Json(StreamTicketResponse {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

fn is_allowed_stream_path(path: &str) -> bool {
    if path.len() > 512 || path.contains('?') || path.contains('#') || path.contains("..") {
        return false;
    }
    path == "/tunnels/settings/install/stream"
        || path == "/terminals/ws"
        || (path.starts_with("/processes/") && path.ends_with("/logs/stream"))
        || (path.starts_with("/scripts/") && path.ends_with("/run"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_ticket_paths_are_allowlisted_and_path_only() {
        assert!(is_allowed_stream_path("/terminals/ws"));
        assert!(is_allowed_stream_path("/processes/123/logs/stream"));
        assert!(!is_allowed_stream_path("/system/shutdown"));
        assert!(!is_allowed_stream_path("/terminals/ws?cwd=other"));
        assert!(!is_allowed_stream_path("/scripts/../secret/run"));
    }

    #[tokio::test]
    async fn active_session_store_is_bounded() {
        let state =
            DaemonState::new_isolated(crate::config::daemon_config::DaemonConfig::default());
        for _ in 0..MAX_ACTIVE_SESSIONS {
            create_session(&state, 0).await.unwrap();
        }
        let error = create_session(&state, 0).await.unwrap_err();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(state.sessions.len(), MAX_ACTIVE_SESSIONS);
    }

    #[tokio::test]
    async fn concurrent_session_admission_cannot_exceed_quota() {
        let state = Arc::new(DaemonState::new_isolated(
            crate::config::daemon_config::DaemonConfig::default(),
        ));
        let mut tasks = Vec::new();
        for _ in 0..(MAX_ACTIVE_SESSIONS * 2) {
            let state = Arc::clone(&state);
            tasks.push(tokio::spawn(async move { create_session(&state, 0).await }));
        }
        let mut successful = 0usize;
        for task in tasks {
            if task.await.unwrap().is_ok() {
                successful += 1;
            }
        }
        assert_eq!(successful, MAX_ACTIVE_SESSIONS);
        assert_eq!(state.sessions.len(), MAX_ACTIVE_SESSIONS);
    }

    #[tokio::test]
    async fn stale_authentication_work_cannot_issue_a_session() {
        let state =
            DaemonState::new_isolated(crate::config::daemon_config::DaemonConfig::default());
        state.auth_generation.store(2, Ordering::Release);

        let error = create_session(&state, 1).await.unwrap_err();

        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
        assert!(state.sessions.is_empty());
    }

    #[tokio::test]
    async fn authentication_work_is_rejected_before_blocking_pool_when_capacity_is_closed() {
        let state = Arc::new(DaemonState::new_isolated(
            crate::config::daemon_config::DaemonConfig::default(),
        ));
        state.auth_verify_limit.close();

        let error = acquire_auth_verification(&state).await.unwrap_err();

        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    }
}

async fn enforce_login_rate_limit(
    state: &DaemonState,
    peer_ip: std::net::IpAddr,
) -> Result<(), ApiError> {
    const WINDOW_SECS: i64 = 60;
    const MAX_ATTEMPTS: usize = 10;
    const MAX_PEER_BUCKETS: usize = 1_024;

    let now = Utc::now();
    let cutoff = now - Duration::seconds(WINDOW_SECS);
    let mut peers = state.login_attempts.lock().await;
    for attempts in peers.values_mut() {
        while attempts.front().is_some_and(|attempt| *attempt <= cutoff) {
            attempts.pop_front();
        }
    }
    peers.retain(|_, attempts| !attempts.is_empty());
    if !peers.contains_key(&peer_ip) && peers.len() >= MAX_PEER_BUCKETS {
        return Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many login sources are active; try again in one minute".into(),
        });
    }
    let attempts = peers.entry(peer_ip).or_default();
    if attempts.len() >= MAX_ATTEMPTS {
        return Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many login attempts; try again in one minute".into(),
        });
    }
    attempts.push_back(now);
    Ok(())
}

// @group Authentication > Logout : Invalidate the current session token
async fn logout(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    let _session_guard = state.session_lock.lock().await;
    if let Some(token) = crate::api::middleware::extract_bearer(&headers) {
        state.sessions.remove(&token);
    }
    Json(serde_json::json!({ "success": true }))
}

async fn validate_session(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "valid": is_session_valid(&state, &headers).await
    }))
}

// @group Authentication > DisablePassword : Remove all browser authentication
// settings while preserving the CLI master token. Requires an authenticated
// session/master token when a password is currently configured.
async fn disable_password(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state
        .config
        .host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
    {
        return Err(ApiError::conflict(
            "dashboard authentication cannot be disabled while the daemon is bound to a non-loopback address",
        ));
    }
    let already_disabled = {
        let auth = state.auth.read().await;
        !auth.web_auth_enabled()
    };

    if !already_disabled && !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized("Not authenticated"));
    }

    let _config_guard = state.config_mutation_lock.lock().await;
    let _session_guard = state.session_lock.lock().await;
    let _ticket_guard = state.stream_ticket_lock.lock().await;
    let mut candidate = state.auth.read().await.clone();
    if candidate.web_auth_enabled() && !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed while the request was waiting; authenticate again",
        ));
    }
    candidate.disable_web_auth();
    let candidate = persist_auth(candidate).await?;
    *state.auth.write().await = candidate;
    state.auth_generation.fetch_add(1, Ordering::AcqRel);
    state.sessions.clear();
    state.stream_tickets.clear();

    Ok(Json(serde_json::json!({ "success": true })))
}

// @group Authentication > ChangePassword : Update password (requires current password)
#[derive(Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized("Not authenticated"));
    }
    validate_password_length(&body.new_password)?;
    if body.current_password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Current password is incorrect".into(),
        });
    }
    let _config_guard = state.config_mutation_lock.lock().await;
    let _session_guard = state.session_lock.lock().await;
    let _ticket_guard = state.stream_ticket_lock.lock().await;
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed while the request was waiting; authenticate again",
        ));
    }
    let candidate = state.auth.read().await.clone();
    if !verify_password_blocking(&state, candidate.clone(), body.current_password).await? {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Current password is incorrect".into(),
        });
    }
    let candidate = set_password_and_persist(candidate, body.new_password).await?;
    *state.auth.write().await = candidate;
    state.auth_generation.fetch_add(1, Ordering::AcqRel);
    state.sessions.clear();
    state.stream_tickets.clear();
    Ok(Json(serde_json::json!({ "success": true })))
}

// @group Authentication > PIN : Set or update the dashboard PIN (4 or 6 digits)
#[derive(Deserialize)]
struct SetPinRequest {
    pin: String,
}

async fn set_pin(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(body): Json<SetPinRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized("Not authenticated"));
    }
    if !matches!(body.pin.len(), 4 | 6) || !body.pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ApiError::bad_request(
            "PIN must contain exactly 4 or 6 digits",
        ));
    }
    let _config_guard = state.config_mutation_lock.lock().await;
    let _session_guard = state.session_lock.lock().await;
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed while the request was waiting; authenticate again",
        ));
    }
    let candidate = state.auth.read().await.clone();
    let candidate = set_pin_and_persist(candidate, body.pin).await?;
    *state.auth.write().await = candidate;
    state.auth_generation.fetch_add(1, Ordering::AcqRel);
    Ok(Json(serde_json::json!({ "success": true })))
}

// @group Authentication > PIN : Remove the configured PIN
async fn remove_pin(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized("Not authenticated"));
    }
    let _config_guard = state.config_mutation_lock.lock().await;
    let _session_guard = state.session_lock.lock().await;
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed while the request was waiting; authenticate again",
        ));
    }
    let mut candidate = state.auth.read().await.clone();
    candidate.clear_pin();
    let candidate = persist_auth(candidate).await?;
    *state.auth.write().await = candidate;
    state.auth_generation.fetch_add(1, Ordering::AcqRel);
    Ok(Json(serde_json::json!({ "success": true })))
}

// @group Authentication > Settings : Update auto-lock timeout
#[derive(Deserialize)]
struct UpdateSettingsRequest {
    lock_timeout_mins: Option<u32>,
}

async fn update_settings(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized("Not authenticated"));
    }
    if body
        .lock_timeout_mins
        .is_some_and(|minutes| minutes > 24 * 60)
    {
        return Err(ApiError::bad_request(
            "lock timeout cannot exceed 1440 minutes",
        ));
    }
    let _config_guard = state.config_mutation_lock.lock().await;
    let _session_guard = state.session_lock.lock().await;
    if !is_session_valid(&state, &headers).await {
        return Err(ApiError::unauthorized(
            "Authentication changed while the request was waiting; authenticate again",
        ));
    }
    let mut candidate = state.auth.read().await.clone();
    candidate.lock_timeout_mins = body.lock_timeout_mins;
    let candidate = persist_auth(candidate).await?;
    *state.auth.write().await = candidate;
    Ok(Json(serde_json::json!({ "success": true })))
}

// @group Utilities : Create a 24-hour browser session and register it in the session store
async fn create_session(
    state: &DaemonState,
    expected_auth_generation: u64,
) -> Result<(String, chrono::DateTime<chrono::Utc>), ApiError> {
    let now = Utc::now();
    let _session_guard = state.session_lock.lock().await;
    if state.auth_generation.load(Ordering::Acquire) != expected_auth_generation {
        return Err(ApiError::unauthorized(
            "Authentication settings changed; submit credentials again",
        ));
    }
    state.sessions.retain(|_, expires_at| *expires_at > now);
    if state.sessions.len() >= MAX_ACTIVE_SESSIONS {
        return Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many active sessions; sign out another session or retry later".into(),
        });
    }
    let token = generate_token();
    let expires_at = now + Duration::hours(24);
    state.sessions.insert(token.clone(), expires_at);
    Ok((token, expires_at))
}

// @group Utilities : Check if the request carries a valid session or master token
async fn is_session_valid(state: &DaemonState, headers: &HeaderMap) -> bool {
    let Some(token) = crate::api::middleware::extract_bearer(headers) else {
        return false;
    };
    let auth = state.auth.read().await;
    if auth.master_token == token {
        return true;
    }
    drop(auth);
    if let Some(exp) = state.sessions.get(&token) {
        return *exp > Utc::now();
    }
    false
}
