// @group APIEndpoints : Terminal command history — persist per-process history to disk

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::error::ApiError;
use crate::config::paths::terminal_history_file;
use crate::daemon::state::DaemonState;

// @group Types : One command history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdEntry {
    pub cmd: String,
    pub count: u32,
}

// @group Types : Full history file — map of key → entries
type HistoryMap = HashMap<String, Vec<CmdEntry>>;
const MAX_HISTORY_KEYS: usize = 500;
const MAX_HISTORY_FILE_BYTES: usize = 2 * 1024 * 1024;

// @group Utilities > TerminalHistory : Read the full history map from disk
fn load_map() -> Result<HistoryMap, String> {
    let path = terminal_history_file();
    crate::config::atomic_file::load_json_with_backup_validated(&path, validate_history_map)
        .map_err(|error| error.to_string())
}

// @group Utilities > TerminalHistory : Persist the full history map to disk
fn save_map(map: &HistoryMap) -> Result<(), String> {
    let serialized = serde_json::to_vec(map).map_err(|error| error.to_string())?;
    if serialized.len() > MAX_HISTORY_FILE_BYTES {
        return Err("terminal history file exceeds the 2 MiB limit".to_string());
    }
    let path = terminal_history_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::config::atomic_file::write_json_with_backup_validated(&path, map, validate_history_map)
        .map_err(|e| e.to_string())
}

async fn load_map_async() -> Result<HistoryMap, ApiError> {
    tokio::task::spawn_blocking(load_map)
        .await
        .map_err(|error| ApiError::internal(format!("terminal history worker failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("failed to load terminal history: {error}")))
}

async fn save_map_async(map: HistoryMap) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || save_map(&map))
        .await
        .map_err(|error| ApiError::internal(format!("terminal history worker failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("failed to save terminal history: {error}")))
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/history/{key}", get(get_history).put(put_history))
        .with_state(state)
}

// @group APIEndpoints > TerminalHistory : GET /terminals/history/:key
async fn get_history(Path(key): Path<String>) -> Result<Json<Vec<CmdEntry>>, ApiError> {
    validate_history_key(&key)?;
    let map = load_map_async().await?;
    Ok(Json(map.get(&key).cloned().unwrap_or_default()))
}

// @group APIEndpoints > TerminalHistory : PUT /terminals/history/:key
async fn put_history(
    State(state): State<Arc<DaemonState>>,
    Path(key): Path<String>,
    Json(entries): Json<Vec<CmdEntry>>,
) -> Result<StatusCode, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    validate_history_key(&key)?;
    if entries.len() > 150 {
        return Err(ApiError::bad_request(
            "terminal history cannot contain more than 150 entries",
        ));
    }
    if entries.iter().any(|entry| entry.cmd.len() > 4_096) {
        return Err(ApiError::bad_request(
            "terminal history commands cannot exceed 4096 bytes",
        ));
    }
    let mut map = load_map_async().await?;
    if !map.contains_key(&key) && map.len() >= MAX_HISTORY_KEYS {
        return Err(ApiError::bad_request(
            "terminal history cannot contain more than 500 keys",
        ));
    }
    // Cap at 150 entries per key
    map.insert(key, entries.into_iter().take(150).collect());
    save_map_async(map).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn validate_history_key(key: &str) -> Result<(), ApiError> {
    if key.is_empty() || key.len() > 200 || key.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "terminal history key must contain 1 to 200 bytes",
        ));
    }
    Ok(())
}

fn validate_history_map(map: &HistoryMap) -> anyhow::Result<()> {
    anyhow::ensure!(
        map.len() <= MAX_HISTORY_KEYS,
        "terminal history cannot contain more than 500 keys"
    );
    for (key, entries) in map {
        anyhow::ensure!(
            !key.is_empty() && key.len() <= 200 && !key.chars().any(char::is_control),
            "terminal history key must contain 1 to 200 bytes"
        );
        anyhow::ensure!(
            entries.len() <= 150,
            "terminal history cannot contain more than 150 entries per key"
        );
        anyhow::ensure!(
            entries.iter().all(|entry| entry.cmd.len() <= 4_096),
            "terminal history commands cannot exceed 4096 bytes"
        );
    }
    anyhow::ensure!(
        serde_json::to_vec(map)?.len() <= MAX_HISTORY_FILE_BYTES,
        "terminal history file exceeds the 2 MiB limit"
    );
    Ok(())
}
