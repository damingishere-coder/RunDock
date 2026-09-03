// @group APIEndpoints : UI / app settings — persisted to data dir, not the browser

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::{api::error::ApiError, config::paths::data_dir, daemon::state::DaemonState};

// @group Utilities > UiSettings : Path to the UI settings file
fn settings_file() -> std::path::PathBuf {
    data_dir().join("ui-settings.json")
}

// @group Utilities > UiSettings : Load raw JSON blob from disk (returns empty object on missing / corrupt)
fn load_raw() -> Result<Value, String> {
    let loaded: Value = crate::config::atomic_file::load_json_with_backup_validated(
        &settings_file(),
        validate_ui_settings,
    )
    .map_err(|error| error.to_string())?;
    Ok(if loaded.is_null() {
        Value::Object(Default::default())
    } else {
        loaded
    })
}

// @group Utilities > UiSettings : Persist raw JSON blob to disk
fn save_raw(val: &Value) -> Result<(), String> {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let encoded = serde_json::to_vec(val).map_err(|error| error.to_string())?;
    if encoded.len() > 1024 * 1024 {
        return Err("UI settings cannot exceed 1 MiB".to_string());
    }
    crate::config::atomic_file::write_json_with_backup_validated(&path, val, validate_ui_settings)
        .map_err(|e| e.to_string())
}

fn validate_ui_settings(value: &Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.is_null() || value.is_object(),
        "UI settings must be a JSON object"
    );
    anyhow::ensure!(
        serde_json::to_vec(value)?.len() <= 1024 * 1024,
        "UI settings cannot exceed 1 MiB"
    );
    Ok(())
}

// @group Types : View-mode wrapper (table | card)
#[derive(Serialize, Deserialize)]
struct ViewModeBody {
    view_mode: String,
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/ui-settings", get(get_settings).put(put_settings))
        .route("/ui-settings/view-mode", put(put_view_mode))
        .with_state(state)
}

// @group APIEndpoints > UiSettings : GET /system/ui-settings — returns full blob
async fn get_settings() -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(load_raw)
        .await
        .map_err(|error| ApiError::internal(format!("UI settings task failed: {error}")))?
        .map(Json)
        .map_err(|error| ApiError::internal(format!("failed to load UI settings: {error}")))
}

// @group APIEndpoints > UiSettings : PUT /system/ui-settings — replace full blob
async fn put_settings(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<Value>,
) -> StatusCode {
    let _config_guard = state.config_mutation_lock.lock().await;
    if !body.is_object() {
        return StatusCode::BAD_REQUEST;
    }
    let save = tokio::task::spawn_blocking(move || save_raw(&body)).await;
    match save {
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        Ok(Ok(_)) => StatusCode::NO_CONTENT,
        Ok(Err(_)) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// @group APIEndpoints > UiSettings : PUT /system/ui-settings/view-mode — quick updater for view-mode
async fn put_view_mode(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<ViewModeBody>,
) -> StatusCode {
    let _config_guard = state.config_mutation_lock.lock().await;
    if body.view_mode.len() > 32 || !matches!(body.view_mode.as_str(), "table" | "card") {
        return StatusCode::BAD_REQUEST;
    }
    let Ok(Ok(mut val)) = tokio::task::spawn_blocking(load_raw).await else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    if let Value::Object(ref mut map) = val {
        map.insert("viewMode".to_string(), Value::String(body.view_mode));
    }
    match tokio::task::spawn_blocking(move || save_raw(&val)).await {
        Ok(Ok(_)) => StatusCode::NO_CONTENT,
        Ok(Err(_)) | Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
