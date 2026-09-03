// @group BusinessLogic : Embedded React SPA served via rust-embed with SPA fallback

use axum::{
    body::Body,
    http::{header, Response, StatusCode, Uri},
    response::IntoResponse,
    routing::get,
    Router,
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web-ui/dist/"]
struct Assets;

pub fn router() -> Router {
    Router::new().fallback(get(serve_spa))
}

fn response_builder(status: StatusCode, content_type: &str) -> axum::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("X-Frame-Options", "DENY")
        .header("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
        .header(
            "Content-Security-Policy",
            "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws: wss:",
        )
}

async fn serve_spa(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(asset) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            response_builder(StatusCode::OK, mime.as_ref())
                .body(Body::from(asset.data.to_vec()))
                .unwrap()
        }
        // SPA fallback — serve index.html for client-side routes
        None => match Assets::get("index.html") {
            Some(asset) => response_builder(StatusCode::OK, "text/html; charset=utf-8")
                .body(Body::from(asset.data.to_vec()))
                .unwrap(),
            None => response_builder(StatusCode::NOT_FOUND, "text/plain; charset=utf-8")
                .body(Body::from("not found"))
                .unwrap(),
        },
    }
}
