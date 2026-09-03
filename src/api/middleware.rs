// @group Authentication : Bearer token validation middleware

use axum::http::header::{HOST, ORIGIN};
use axum::http::StatusCode;
use axum::Json;
use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use std::sync::Arc;

use crate::daemon::state::DaemonState;

/// Axum middleware that enforces bearer-token authentication on all protected routes.
///
/// Accepts a bearer token for normal requests. SSE/WebSocket handshakes may
/// instead use a one-time, path-bound `?ticket=` credential that expires after
/// 30 seconds; long-lived session tokens are never placed in URLs.
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
    let request_path = req
        .uri()
        .path()
        .strip_prefix("/api/v1")
        .unwrap_or_else(|| req.uri().path());
    // Browsers cannot attach an Authorization header to EventSource/WebSocket
    // handshakes.  These executable/long-lived GET endpoints therefore require
    // either a real bearer token (CLI) or a path-bound one-time ticket; merely
    // omitting Origin must never turn them into passwordless cross-site actions.
    let stream_request = is_stream_request_path(request_path);

    let master_authenticated = {
        let auth = state.auth.read().await;
        token.as_deref() == Some(auth.master_token.as_str())
    };
    if master_authenticated {
        return next.run(req).await;
    }

    if let Some(token) = token.as_deref() {
        // Session token check
        let expires_at = state.sessions.get(token).map(|entry| *entry);
        if expires_at.is_some_and(|expires_at| expires_at > Utc::now()) {
            // Never hold a DashMap shard guard across a long-lived SSE or
            // WebSocket request; logout must remain able to revoke sessions.
            return next.run(req).await;
        }
        if expires_at.is_some() {
            // Expired — clean up after the shard guard has been dropped.
            state.sessions.remove(token);
        }
    }

    // No configured password is the explicit passwordless mode for ordinary
    // loopback API calls. Executable/long-lived GET streams are excluded and
    // require either a valid credential above or a one-time ticket below.
    {
        let auth = state.auth.read().await;
        let direct_local = req
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .is_some_and(|ConnectInfo(peer)| {
                is_direct_loopback_request(*peer, req.headers(), state.config.port)
            });
        if !stream_request && !auth.web_auth_enabled() && direct_local {
            drop(auth);
            return next.run(req).await;
        }
    }

    if req.headers().get(ORIGIN).is_some_and(|origin| {
        crate::daemon::server::is_trusted_browser_origin(origin, state.config.port)
    }) {
        if let Some(ticket) = extract_query_value(req.uri().query(), "ticket") {
            let request_path = request_path.to_string();
            let request_query = query_without_ticket(req.uri().query()).to_string();
            // Invalid probes do not burn a legitimate ticket. remove_if is an
            // atomic check-and-consume, so two valid handshakes still cannot
            // reuse the same credential.
            if consume_stream_ticket(
                &state.stream_tickets,
                &ticket,
                req.method(),
                &request_path,
                &request_query,
            ) {
                return next.run(req).await;
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Unauthorized" })),
    )
        .into_response()
}

pub(crate) fn is_direct_loopback_request(
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
    daemon_port: u16,
) -> bool {
    if !peer.ip().is_loopback()
        || [
            "forwarded",
            "x-forwarded-for",
            "x-forwarded-host",
            "x-forwarded-proto",
            "via",
        ]
        .iter()
        .any(|name| headers.contains_key(*name))
    {
        return false;
    }
    let Some(authority) = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
    else {
        return false;
    };
    if authority.port_u16() != Some(daemon_port) {
        return false;
    }
    let host = authority.host();
    let trusted_host = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    trusted_host
        && headers
            .get(ORIGIN)
            .map(|origin| crate::daemon::server::is_trusted_browser_origin(origin, daemon_port))
            .unwrap_or(true)
}

fn is_stream_request_path(path: &str) -> bool {
    path == "/tunnels/settings/install/stream"
        || path == "/terminals/ws"
        || (path.starts_with("/processes/") && path.ends_with("/logs/stream"))
        || (path.starts_with("/scripts/") && path.ends_with("/run"))
}

fn consume_stream_ticket(
    tickets: &dashmap::DashMap<String, crate::daemon::state::StreamTicket>,
    ticket: &str,
    request_method: &axum::http::Method,
    request_path: &str,
    request_query: &str,
) -> bool {
    tickets
        .remove_if(ticket, |_, entry| {
            entry.expires_at > Utc::now()
                && entry.method == *request_method
                && entry.path == request_path
                && entry.query.as_deref().unwrap_or_default() == request_query
        })
        .is_some()
}

/// Extract a bearer token. Session/master tokens are deliberately header-only.
fn extract_token<B>(req: &Request<B>) -> Option<String> {
    extract_bearer(req.headers())
}

pub(crate) fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn extract_query_value(query: Option<&str>, key: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

fn query_without_ticket(query: Option<&str>) -> String {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty() && !pair.starts_with("ticket="))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::state::StreamTicket;

    fn direct_headers() -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(HOST, "127.0.0.1:2999".parse().unwrap());
        headers
    }

    #[test]
    fn passwordless_bypass_requires_a_direct_loopback_request() {
        let peer: std::net::SocketAddr = "127.0.0.1:54321".parse().unwrap();
        assert!(is_direct_loopback_request(peer, &direct_headers(), 2999));

        let mut proxied = direct_headers();
        proxied.insert("x-forwarded-for", "203.0.113.10".parse().unwrap());
        assert!(!is_direct_loopback_request(peer, &proxied, 2999));

        let remote: std::net::SocketAddr = "192.0.2.10:54321".parse().unwrap();
        assert!(!is_direct_loopback_request(remote, &direct_headers(), 2999));

        let mut external_host = direct_headers();
        external_host.insert(HOST, "control.example.com".parse().unwrap());
        assert!(!is_direct_loopback_request(peer, &external_host, 2999));
    }

    #[test]
    fn stream_ticket_query_is_removed_without_changing_bound_parameters() {
        assert_eq!(
            query_without_ticket(Some("cwd=C%3A%5Cwork&ticket=one&cols=80")),
            "cwd=C%3A%5Cwork&cols=80"
        );
        assert_eq!(
            extract_query_value(Some("cwd=x&ticket=single-use"), "ticket").as_deref(),
            Some("single-use")
        );
    }

    #[test]
    fn stream_ticket_query_keeps_order_and_duplicates_for_exact_matching() {
        assert_eq!(query_without_ticket(Some("a=1&a=2&ticket=x")), "a=1&a=2");
        assert_ne!(query_without_ticket(Some("a=2&a=1&ticket=x")), "a=1&a=2");
    }

    #[test]
    fn executable_get_streams_are_never_plain_passwordless_requests() {
        assert!(is_stream_request_path("/scripts/backup/run"));
        assert!(is_stream_request_path("/terminals/ws"));
        assert!(is_stream_request_path("/processes/123/logs/stream"));
        assert!(!is_stream_request_path("/processes"));
    }

    #[tokio::test]
    async fn invalid_request_does_not_consume_ticket_but_valid_request_does() {
        let tickets = dashmap::DashMap::new();
        tickets.insert(
            "one-time".to_string(),
            StreamTicket {
                method: axum::http::Method::GET,
                path: "/processes/id/logs/stream".to_string(),
                query: Some("lines=20".to_string()),
                expires_at: Utc::now() + chrono::Duration::seconds(30),
            },
        );

        assert!(!consume_stream_ticket(
            &tickets,
            "one-time",
            &axum::http::Method::GET,
            "/wrong",
            "lines=20"
        ));
        assert!(tickets.contains_key("one-time"));
        assert!(!consume_stream_ticket(
            &tickets,
            "one-time",
            &axum::http::Method::POST,
            "/processes/id/logs/stream",
            "lines=20"
        ));
        assert!(tickets.contains_key("one-time"));
        assert!(consume_stream_ticket(
            &tickets,
            "one-time",
            &axum::http::Method::GET,
            "/processes/id/logs/stream",
            "lines=20"
        ));
        assert!(!tickets.contains_key("one-time"));
    }
}
