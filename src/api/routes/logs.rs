// @group APIEndpoints : Standalone log management endpoints (flush)
// Note: per-process log streaming is in processes.rs

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use axum::{
    extract::{Path, State},
    routing::delete,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/{id}/logs", delete(flush_logs))
        .with_state(state)
}

// @group APIEndpoints > Logs : DELETE /processes/:id/logs — delete log files
async fn flush_logs(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = state
        .manager
        .resolve_id(&id_str)
        .await
        .map_err(|_| ApiError::not_found(format!("process not found: {id_str}")))?;

    state
        .manager
        .clear_logs(id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to clear logs: {error}")))?;

    Ok(Json(json!({ "success": true, "message": "logs flushed" })))
}
