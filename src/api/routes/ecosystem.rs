// @group APIEndpoints : Ecosystem config file loading endpoint

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use crate::models::api_types::EcosystemRequest;
use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

const MAX_ECOSYSTEM_IMPORT_DURATION: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", post(load_ecosystem))
        .with_state(state)
}

// @group APIEndpoints > Ecosystem : POST /ecosystem
async fn load_ecosystem(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<EcosystemRequest>,
) -> Result<Json<Value>, ApiError> {
    if req.path.is_empty() || req.path.len() > 4096 {
        return Err(ApiError::bad_request(
            "ecosystem path must contain between 1 and 4096 bytes",
        ));
    }
    let path = PathBuf::from(req.path);
    let import_deadline = tokio::time::Instant::now() + MAX_ECOSYSTEM_IMPORT_DURATION;
    let config = tokio::time::timeout(
        MAX_ECOSYSTEM_IMPORT_DURATION,
        tokio::task::spawn_blocking(move || {
            crate::config::ecosystem::EcosystemConfig::from_file(&path)
        }),
    )
    .await
    .map_err(|_| ApiError::bad_request("ecosystem parsing timed out"))?
    .map_err(|error| ApiError::internal(format!("ecosystem parse task failed: {error}")))?
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let _mutation_guard = state.state_mutation_lock.lock().await;

    let total = config.apps.len();
    let mut started = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut started_ids = Vec::new();
    let projects_before = state.projects.read().await.clone();

    for app in config.apps {
        if import_deadline.saturating_duration_since(tokio::time::Instant::now())
            < std::time::Duration::from_secs(30)
        {
            errors.push(format!(
                "ecosystem import stopped after {} seconds; remaining apps were not started",
                MAX_ECOSYSTEM_IMPORT_DURATION.as_secs()
            ));
            break;
        }
        if app
            .env
            .values()
            .any(|value| value == crate::models::notification::MASKED_SECRET)
        {
            errors.push(format!(
                "{}: reserved masked secret value is not allowed",
                app.name
            ));
            continue;
        }
        if let Some(notification) = app.notify.as_ref() {
            if let Err(error) = notification.validate() {
                errors.push(format!("{}: {error}", app.name));
                continue;
            }
        }
        match state.manager.start(app).await {
            Ok(mut info) => {
                let project_id = info.project_id.unwrap_or(info.id);
                if info.project_id.is_none() {
                    info = match state.manager.assign_project(info.id, project_id).await {
                        Ok(info) => info,
                        Err(error) => {
                            let message = match state.manager.delete(info.id).await {
                                Ok(_) => error.to_string(),
                                Err(cleanup_error) => {
                                    let persistence = match state.save_to_disk().await {
                                        Ok(()) => "the still-running orphan process was persisted for later recovery".to_string(),
                                        Err(persist_error) => format!(
                                            "persisting the orphan process also failed: {persist_error}"
                                        ),
                                    };
                                    format!(
                                        "{}; started process cleanup also failed: {}; {}",
                                        error, cleanup_error, persistence
                                    )
                                }
                            };
                            errors.push(message);
                            continue;
                        }
                    };
                }
                state.projects.write().await.ensure(project_id, &info.name);
                started_ids.push(info.id);
                started += 1;
            }
            Err(e) => errors.push(e.to_string()),
        }
    }

    if started > 0 {
        let persist_result = state.save_state_and_projects().await;
        if let Err(error) = persist_result {
            let mut rollback_errors = Vec::new();
            for id in started_ids.into_iter().rev() {
                if let Err(rollback_error) = state.manager.delete(id).await {
                    rollback_errors.push(format!("{id}: {rollback_error}"));
                }
            }
            *state.projects.write().await = projects_before;
            if let Err(rollback_error) = state.save_state_and_projects_rollback().await {
                rollback_errors.push(format!("state/project rollback: {rollback_error}"));
            }
            let rollback = if rollback_errors.is_empty() {
                "rollback completed".to_string()
            } else {
                format!("rollback errors: {}", rollback_errors.join("; "))
            };
            if !rollback_errors.is_empty() {
                *state.background_persistence_error.write().await = Some(format!(
                    "ecosystem import persistence failed ({error}); {rollback}"
                ));
            }
            return Err(ApiError::internal(format!(
                "ecosystem import could not be persisted and was rolled back ({rollback}): {error}"
            )));
        }
    }

    let status = if started == total {
        "complete"
    } else if started == 0 && errors.is_empty() {
        "empty"
    } else if started == 0 {
        "failed"
    } else {
        "partial"
    };

    Ok(Json(json!({
        "status": status,
        "total": total,
        "started": started,
        "errors": errors,
        "persistence": if started > 0 { "committed" } else { "unchanged" },
    })))
}
