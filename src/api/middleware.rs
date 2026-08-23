// @group Authentication : Bearer token validation middleware

use axum::http::StatusCode;
use axum::Json;
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use std::sync::Arc;

use crate::daemon::state::DaemonState;

/// Axum middleware that enforces bearer-token authentication on all protected routes.
///
/// Accepts tokens from two sources (priority order):
///   1. `Authorization: Bearer <token>` header — standard for all fetch/XHR calls
///   2. `?token=<token>` query parameter — fallback for EventSource (cannot set headers)
///
/// Valid tokens:
///   - The **master token** read from `auth.json` by the CLI — never expires
///   - A **session token** issued on login — expires after 24 h
pub async fn require_auth(
    State(state): State<Arc<DaemonState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let token = extract_token(&req);

    // No configured password is the explicit passwordless mode. This matches the
    // public API contract and keeps a fresh local installation usable without setup.
    {
        let auth = state.auth.read().await;
        if !auth.web_auth_enabled() {
            drop(auth);
            return next.run(req).await;
        }

        // Master token check (CLI — never expires)
        if token.as_deref() == Some(auth.master_token.as_str()) {
            drop(auth);
            return next.run(req).await;
        }
    }

    if let Some(token) = token {
        // Session token check
        if let Some(entry) = state.sessions.get(&token) {
            if *entry > Utc::now() {
                return next.run(req).await;
            }
            // Expired — clean up
            drop(entry);
            state.sessions.remove(&token);
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Unauthorized" })),
    )
        .into_response()
}

/// Extract the bearer token from Authorization header or ?token= query param.
fn extract_token<B>(req: &Request<B>) -> Option<String> {
    // Authorization: Bearer <token>
    if let Some(val) = req.headers().get("Authorization") {
        if let Ok(s) = val.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    // ?token=<token> (for EventSource which cannot set request headers)
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(val) = pair.strip_prefix("token=") {
                return Some(val.to_string());
            }
        }
    }
    None
}
