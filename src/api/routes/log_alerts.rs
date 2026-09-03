// @group APIEndpoints : Log alert store — GET/PUT /log-alerts, namespace overrides

use crate::config::log_alert_config::{self, LogAlertOverride, LogAlertStore};
use crate::daemon::state::DaemonState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/log-alerts", get(get_store).put(put_store))
        .route(
            "/log-alerts/namespace/{ns}",
            axum::routing::put(put_namespace).delete(delete_namespace),
        )
        .with_state(state)
}

async fn load_store_async() -> Result<LogAlertStore, String> {
    tokio::task::spawn_blocking(log_alert_config::load)
        .await
        .map_err(|error| format!("log alert load task failed: {error}"))?
        .map_err(|error| error.to_string())
}

async fn save_store_async(store: LogAlertStore) -> Result<(), String> {
    tokio::task::spawn_blocking(move || log_alert_config::save(&store))
        .await
        .map_err(|error| format!("log alert save task failed: {error}"))?
        .map_err(|error| error.to_string())
}

// @group APIEndpoints > LogAlerts : GET /log-alerts — return full store (global + namespace overrides)
async fn get_store() -> (StatusCode, Json<Value>) {
    match load_store_async().await {
        Ok(store) => (StatusCode::OK, Json(json!(store))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("log alert settings are unreadable: {error}") })),
        ),
    }
}

// @group APIEndpoints > LogAlerts : PUT /log-alerts — replace the full store
async fn put_store(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<LogAlertStore>,
) -> (StatusCode, Json<Value>) {
    let _config_guard = state.config_mutation_lock.lock().await;
    if let Err(error) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        );
    }
    match save_store_async(body.clone()).await {
        Ok(_) => (StatusCode::OK, Json(json!(body))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

// @group APIEndpoints > LogAlerts : PUT /log-alerts/namespace/:ns — upsert a namespace override
async fn put_namespace(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
    Json(body): Json<LogAlertOverride>,
) -> (StatusCode, Json<Value>) {
    let _config_guard = state.config_mutation_lock.lock().await;
    if let Err(error) = log_alert_config::validate_namespace(&ns) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        );
    }
    if let Err(error) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        );
    }
    let mut store = match load_store_async().await {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("log alert settings are unreadable: {error}") })),
            )
        }
    };
    if !store.namespaces.contains_key(&ns)
        && store.namespaces.len() >= log_alert_config::MAX_NAMESPACE_OVERRIDES
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "too many namespace overrides" })),
        );
    }
    store.namespaces.insert(ns, body.clone());
    match save_store_async(store).await {
        Ok(_) => (StatusCode::OK, Json(json!(body))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

// @group APIEndpoints > LogAlerts : DELETE /log-alerts/namespace/:ns — remove a namespace override
async fn delete_namespace(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
) -> (StatusCode, Json<Value>) {
    let _config_guard = state.config_mutation_lock.lock().await;
    if let Err(error) = log_alert_config::validate_namespace(&ns) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        );
    }
    let mut store = match load_store_async().await {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("log alert settings are unreadable: {error}") })),
            )
        }
    };
    store.namespaces.remove(&ns);
    match save_store_async(store).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "deleted": ns }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}
