// @group APIEndpoints : Axum HTTP server — route registration and shared state injection

use crate::api;
use crate::config::daemon_config::DaemonConfig;
use crate::daemon::state::DaemonState;
use crate::web;
use anyhow::Result;
use axum::http::{
    header::{HeaderName, AUTHORIZATION, CONTENT_TYPE},
    request::Parts,
    HeaderValue, Method,
};
use axum::Router;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

pub(crate) fn loopback_socket_addr(host: &str, port: u16) -> Result<SocketAddr> {
    if port == 0 {
        anyhow::bail!("daemon port 0 is not supported; choose an explicit port");
    }
    let host = host.trim();
    let address = if host.eq_ignore_ascii_case("localhost") {
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    } else {
        host.trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .map_err(|error| anyhow::anyhow!("invalid daemon bind address {host}: {error}"))?
    };
    if !address.is_loopback() {
        anyhow::bail!(
            "refusing plaintext daemon on non-loopback address {address}; bind to loopback and use an HTTPS reverse proxy or SSH tunnel"
        );
    }
    Ok(SocketAddr::new(address, port))
}

#[cfg(windows)]
fn enable_exclusive_address_use(socket: &socket2::Socket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{
        setsockopt, SOCKET, SOCKET_ERROR, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
    };

    let enabled = 1u32.to_ne_bytes();
    // SAFETY: the socket remains alive for the call and `enabled` is a valid BOOL-sized buffer.
    let result = unsafe {
        setsockopt(
            SOCKET(socket.as_raw_socket() as usize),
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            Some(&enabled),
        )
    };
    if result == SOCKET_ERROR {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn loopback_port_is_available(addr: SocketAddr) -> std::io::Result<bool> {
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        None,
    )?;
    #[cfg(windows)]
    enable_exclusive_address_use(&socket)?;
    match socket.bind(&addr.into()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => Ok(false),
        Err(error) => Err(error),
    }
}

pub async fn run(state: Arc<DaemonState>, config: DaemonConfig) -> Result<()> {
    let addr = loopback_socket_addr(&config.host, config.port)?;

    let daemon_port = config.port;
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _request: &Parts| {
                is_trusted_browser_origin(origin, daemon_port)
            },
        ))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("x-rundock-device-token"),
        ]);

    let app = Router::new()
        .merge(web::router())
        .nest("/api/v1", api::router(Arc::clone(&state)))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    tracing::info!("HTTP server listening on http://{addr}");
    let mut shutdown = state.subscribe_shutdown();
    let mut forced_shutdown = state.subscribe_shutdown();

    // @group Configuration : Windows must prevent another local process from sharing the control port.
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        None,
    )?;
    #[cfg(windows)]
    enable_exclusive_address_use(&socket)?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let listener = tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket))?;
    // During a daemon restart this is the second handoff phase: the replacement
    // has loaded state, acquired the PID file, and actually owns the listener.
    crate::mark_restart_handoff_ready_from_env()?;

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        if !state.is_shutdown_requested() {
            let _ = shutdown.recv().await;
        }
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        _ = async move {
            let _ = forced_shutdown.recv().await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        } => tracing::warn!("graceful shutdown timed out after 5 seconds; closing remaining connections"),
    }
    Ok(())
}

/// Browser origins are trusted only when their host is the loopback interface.
/// Same-origin requests do not need CORS; this allowlist exists for the local
/// Vite UI and deliberately excludes arbitrary internet origins.
pub(crate) fn is_trusted_browser_origin(origin: &HeaderValue, daemon_port: u16) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    if url.scheme() != "http"
        || !url
            .port_or_known_default()
            .is_some_and(|port| port == 5173 || port == daemon_port)
    {
        return false;
    }
    match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[test]
    fn cors_origin_accepts_only_loopback_browser_hosts() {
        for trusted in [
            "http://127.0.0.1:5173",
            "http://localhost:5173",
            "http://localhost:2999",
            "http://[::1]:5173",
        ] {
            assert!(
                is_trusted_browser_origin(&origin(trusted), 2999),
                "rejected {trusted}"
            );
        }
        for untrusted in [
            "https://example.com",
            "http://localhost:8080",
            "https://localhost:2999",
            "http://192.168.1.20:5173",
            "null",
            "file:///tmp/index.html",
        ] {
            assert!(
                !is_trusted_browser_origin(&origin(untrusted), 2999),
                "accepted {untrusted}"
            );
        }
    }

    #[test]
    fn plaintext_binding_is_loopback_only() {
        assert_eq!(
            loopback_socket_addr("localhost", 2999).unwrap(),
            "127.0.0.1:2999".parse().unwrap()
        );
        assert_eq!(
            loopback_socket_addr("::1", 2999).unwrap(),
            "[::1]:2999".parse().unwrap()
        );
        assert!(loopback_socket_addr("192.168.1.20", 2999).is_err());
    }

    #[test]
    fn port_availability_distinguishes_a_listener_from_an_unused_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert!(!super::loopback_port_is_available(address).unwrap());
        drop(listener);
        assert!(super::loopback_port_is_available(address).unwrap());
    }
}
