use crate::config::desktop_access::DesktopAccess;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

fn browser_request(headers: &HeaderMap) -> bool {
    ["sec-fetch-mode", "sec-fetch-site", "sec-ch-ua", "origin"]
        .iter()
        .any(|name| headers.contains_key(*name))
}

pub async fn enforce(
    State(config): State<DesktopAccess>,
    request: Request,
    next: Next,
) -> Response {
    if !config.desktop_only {
        return next.run(request).await;
    }
    let desktop = config.accepts_user_agent(
        request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
    );
    // Block every SPA route and asset, plus API/stream requests from previously
    // loaded browser pages. Existing CLI and local API clients retain their
    // normal authentication. This preference is not a substitute for API auth.
    let local_api_client =
        request.uri().path().starts_with("/api/v1/") && !browser_request(request.headers());
    let mut response = if desktop || local_api_client {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "RunDock 网页端已停用，请使用 RunDock 软件窗口。",
        )
            .into_response()
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("User-Agent"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{middleware, routing::get, Router};

    #[tokio::test]
    async fn desktop_only_blocks_browser_routes_assets_and_cached_api_requests() {
        let config: DesktopAccess = serde_json::from_value(serde_json::json!({
            "desktop_only": true,
            "desktop_token": "a".repeat(64),
        }))
        .unwrap();
        let agent = config.user_agent().unwrap();
        let app = Router::new()
            .fallback(get(|| async { "available" }))
            .layer(middleware::from_fn_with_state(config, enforce));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for path in [
            "/",
            "/processes",
            "/assets/index.js",
            "/api/v1/processes",
            "/api/v1/terminals/ws",
        ] {
            let response = client
                .get(format!("{base}{path}"))
                .header(header::USER_AGENT, "Mozilla/5.0")
                .header("sec-fetch-site", "same-origin")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let response = client
                .get(format!("{base}{path}"))
                .header(header::USER_AGENT, &agent)
                .header("sec-fetch-site", "same-origin")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
        let api = format!("{base}/api/v1/system/health");
        assert_eq!(
            client.get(&api).send().await.unwrap().status(),
            StatusCode::OK
        );
        // PowerShell's native HTTP client also uses a Mozilla-style user agent.
        // Only browser-controlled request metadata should block local API tools.
        assert_eq!(
            client
                .get(&api)
                .header(header::USER_AGENT, "Mozilla/5.0 PowerShell/7.6.5")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .get(&api)
                .header("sec-fetch-mode", "cors")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .get(format!("{base}/"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        task.abort();
    }

    #[tokio::test]
    async fn web_default_stays_compatible() {
        let app = Router::new().fallback(get(|| async { "available" })).layer(
            middleware::from_fn_with_state(DesktopAccess::default(), enforce),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/processes", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .header(header::USER_AGENT, "Mozilla/5.0")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        task.abort();
    }
}
