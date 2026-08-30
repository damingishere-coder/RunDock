// @group APIEndpoints : Process CRUD endpoints

use crate::api::error::ApiError;
use crate::config::ecosystem::AppConfig;
use crate::daemon::state::DaemonState;
use crate::models::api_types::{
    PatchField, ProcessNotificationRequest, StartRequest, UpdateProcessRequest,
};
use crate::models::process_status::ProcessStatus;
use crate::models::project::AssignProjectRequest;
use crate::process::manager::ManagedProcessSnapshot;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(list_processes).post(start_process))
        .route(
            "/{id}",
            get(get_process)
                .delete(delete_process)
                .patch(update_process),
        )
        .route("/{id}/stop", post(stop_process))
        .route("/{id}/start", post(start_stopped_process))
        .route("/{id}/restart", post(restart_process))
        .route("/{id}/reset", post(reset_process))
        .route("/{id}/terminal", post(open_terminal))
        .route("/{id}/logs", get(get_logs).delete(delete_logs))
        .route("/{id}/logs/dates", get(get_log_dates))
        .route("/{id}/logs/stream", get(stream_logs))
        .route("/{id}/metrics/history", get(get_metrics_history))
        .route("/{id}/logs/stats", get(get_log_stats))
        .route("/{id}/cron/history", get(get_cron_history))
        .route("/{id}/enabled", patch(set_process_enabled))
        .route("/{id}/notifications", patch(set_process_notifications))
        .route("/{id}/project", patch(assign_process_project))
        .route("/{id}/clone", post(clone_process))
        .route("/{id}/envfiles", get(list_envfiles))
        .route("/{id}/envfile", get(get_envfile).put(put_envfile))
        // Namespace bulk operations
        .route("/namespace/{ns}/start", post(start_namespace_processes))
        .route("/namespace/{ns}/stop", post(stop_namespace_processes))
        .route("/namespace/{ns}/restart", post(restart_namespace_processes))
        .with_state(state)
}

fn process_is_active(process: &crate::models::process_info::ProcessInfo) -> bool {
    process.pid.is_some()
        || matches!(
            process.status,
            ProcessStatus::Starting
                | ProcessStatus::Running
                | ProcessStatus::Watching
                | ProcessStatus::Sleeping
        )
}

fn validate_max_log_size_mb(value: u64) -> Result<u64, ApiError> {
    if (1..=1024).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::bad_request(
            "max_log_size_mb must be between 1 and 1024",
        ))
    }
}

fn acquire_blocking_io(state: &DaemonState) -> Result<tokio::sync::SemaphorePermit<'_>, ApiError> {
    state.blocking_io_limit.try_acquire().map_err(|_| {
        ApiError::unavailable("filesystem operation capacity is exhausted; retry later")
    })
}

async fn finish_process_rollback(
    state: &DaemonState,
    operation: &str,
    persistence_error: String,
    mut rollback_errors: Vec<String>,
) -> ApiError {
    if let Err(error) = state.save_state_rollback().await {
        rollback_errors.push(format!("rollback persistence: {error}"));
    }
    if rollback_errors.is_empty() {
        return ApiError::internal(format!(
            "{operation} could not be persisted and was rolled back: {persistence_error}"
        ));
    }
    let detail = format!(
        "{operation} persistence failed ({persistence_error}); rollback is incomplete: {}",
        rollback_errors.join("; ")
    );
    *state.background_persistence_error.write().await = Some(detail.clone());
    ApiError::internal(detail)
}

// @group APIEndpoints > Process : GET /processes
async fn list_processes(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let processes = state.manager.list().await;
    Json(json!({ "processes": processes }))
}

// @group APIEndpoints > Process : POST /processes
async fn start_process(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<StartRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let name = req.name.unwrap_or_else(|| {
        std::path::Path::new(&req.script)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app")
            .to_string()
    });

    let cron = req.cron.clone();
    // Cron jobs default to autorestart=false — the scheduler drives re-runs
    let autorestart = req.autorestart.unwrap_or(cron.is_none());

    let env = req.env.unwrap_or_default();
    if env
        .values()
        .any(|value| value == crate::models::notification::MASKED_SECRET)
    {
        return Err(ApiError::bad_request(
            "reserved masked secret value is not allowed",
        ));
    }
    if let Some(config) = req.notify.as_ref() {
        config
            .validate()
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    }

    let max_log_size_mb = validate_max_log_size_mb(req.max_log_size_mb.unwrap_or(10))?;
    let config = AppConfig {
        name,
        project_id: req.project_id,
        script: req.script,
        args: req.args.unwrap_or_default(),
        cwd: req.cwd,
        instances: 1,
        autorestart,
        max_restarts: req.max_restarts.unwrap_or(10),
        restart_delay_ms: req.restart_delay_ms.unwrap_or(1000),
        namespace: req.namespace.unwrap_or_else(|| "default".to_string()),
        watch: req.watch.unwrap_or(false),
        watch_paths: req.watch_paths.unwrap_or_default(),
        watch_ignore: req.watch_ignore.unwrap_or_default(),
        env,
        log_file: None,
        error_file: None,
        max_log_size_mb,
        cron,
        cron_last_run: None,
        cron_next_run: None,
        notify: req.notify,
        log_alert: req.log_alert,
        env_file: None,
        health_check_url: None,
        health_check_interval_secs: 30,
        health_check_timeout_secs: 5,
        health_check_retries: 3,
        pre_start: None,
        post_start: None,
        pre_stop: None,
        enabled: true,
    };
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let projects_before = state.projects.read().await.clone();
    let mut info = state.manager.start(config).await.map_err(ApiError::from)?;
    let project_id = info.project_id.unwrap_or(info.id);
    if info.project_id.is_none() {
        info = match state.manager.assign_project(info.id, project_id).await {
            Ok(info) => info,
            Err(error) => {
                return match state.manager.delete(info.id).await {
                    Ok(_) => Err(ApiError::from(error)),
                    Err(cleanup_error) => {
                        let mut detail = format!(
                            "project assignment failed ({error}); started process cleanup also failed ({cleanup_error})"
                        );
                        match state.save_to_disk().await {
                            Ok(()) => detail.push_str(
                                "; the still-running orphan process was persisted for later recovery",
                            ),
                            Err(persist_error) => detail.push_str(&format!(
                                "; persisting the orphan process also failed ({persist_error})"
                            )),
                        }
                        *state.background_persistence_error.write().await = Some(detail.clone());
                        Err(ApiError::internal(detail))
                    }
                };
            }
        };
    }
    state.projects.write().await.ensure(project_id, &info.name);

    let persist_result = state.save_state_and_projects().await;
    if let Err(error) = persist_result {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.delete(info.id).await {
            rollback_errors.push(format!("process cleanup: {rollback_error}"));
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
                "process start persistence failed ({error}); {rollback}"
            ));
        }
        return Err(ApiError::internal(format!(
            "process could not be persisted ({rollback}): {error}"
        )));
    }
    Ok((StatusCode::CREATED, Json(json!(info))))
}

// @group APIEndpoints > Process : GET /processes/:id
async fn get_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : DELETE /processes/:id
async fn delete_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state
        .manager
        .snapshot()
        .await
        .into_iter()
        .find(|snapshot| snapshot.info.id == id)
        .ok_or_else(|| ApiError::not_found("process not found"))?;
    state.manager.delete(id).await.map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process deletion",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(
        json!({ "success": true, "message": "process deleted" }),
    ))
}

// @group APIEndpoints > Process : POST /processes/:id/stop
async fn stop_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state
        .manager
        .snapshot()
        .await
        .into_iter()
        .find(|snapshot| snapshot.info.id == id)
        .ok_or_else(|| ApiError::not_found("process not found"))?;
    if !process_is_active(&before.info) {
        return Err(ApiError::conflict("process is not active"));
    }
    let info = state.manager.stop(id).await.map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process stop",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : POST /processes/:id/start
async fn start_stopped_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state
        .manager
        .snapshot_one(id)
        .await
        .map_err(ApiError::from)?;
    if !before.info.enabled {
        return Err(ApiError::conflict(
            "process is disabled; enable it before starting",
        ));
    }
    if process_is_active(&before.info) {
        return Err(ApiError::conflict("process is already active"));
    }
    before
        .config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let info = state
        .manager
        .start_existing(id)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process start",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : POST /processes/:id/restart
async fn restart_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state
        .manager
        .snapshot_one(id)
        .await
        .map_err(ApiError::from)?;
    if !before.info.enabled {
        return Err(ApiError::conflict(
            "process is disabled; enable it before restarting",
        ));
    }
    if matches!(
        before.info.status,
        ProcessStatus::Starting | ProcessStatus::Stopping
    ) {
        return Err(ApiError::conflict(
            "process is busy starting or stopping; retry the restart",
        ));
    }
    before
        .config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let info = state.manager.restart(id).await.map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process restart",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : POST /processes/:id/reset
async fn reset_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state.manager.get(id).await.map_err(ApiError::from)?;
    let info = state.manager.reset(id).await.map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state
            .manager
            .set_restart_count(id, before.restart_count)
            .await
        {
            rollback_errors.push(format!("restart counter: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "restart counter reset",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : PATCH /processes/:id/enabled — toggle enabled flag (affects Start All)
async fn set_process_enabled(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| ApiError::bad_request("missing 'enabled' boolean field"))?;
    let before = state
        .manager
        .snapshot_one(id)
        .await
        .map_err(ApiError::from)?;
    if before.info.enabled == enabled {
        return Ok(Json(json!(before.info)));
    }
    if matches!(
        before.info.status,
        ProcessStatus::Starting | ProcessStatus::Stopping
    ) {
        return Err(ApiError::conflict(
            "process is busy starting or stopping; retry the enabled-state update",
        ));
    }
    let info = state
        .manager
        .set_enabled(id, enabled)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_enabled_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "enabled flag update",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : PATCH /processes/:id/notifications — metadata-only update
async fn set_process_notifications(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(body): Json<ProcessNotificationRequest>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let previous = state
        .manager
        .get_config(id)
        .await
        .map_err(ApiError::from)?
        .notify;
    let mut notify = body.notify;
    if let (Some(candidate), Some(current)) = (notify.as_mut(), previous.as_ref()) {
        candidate.preserve_masked_secrets(current);
    }
    if let Some(config) = notify.as_ref() {
        config
            .validate()
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    }
    let info = state
        .manager
        .set_notification_config(id, notify)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.set_notification_config(id, previous).await {
            rollback_errors.push(format!("notification config: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process notification update",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Project : PATCH /processes/:id/project — metadata-only assignment
async fn assign_process_project(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(body): Json<AssignProjectRequest>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let before = state.manager.get(id).await.map_err(ApiError::from)?;
    let projects_before = state.projects.read().await.clone();
    let info = state
        .manager
        .assign_project(id, body.project_id)
        .await
        .map_err(ApiError::from)?;
    state
        .projects
        .write()
        .await
        .ensure(body.project_id, &before.name);
    let persist_result = state.save_state_and_projects().await;
    if let Err(error) = persist_result {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state
            .manager
            .set_project_assignment(id, before.project_id)
            .await
        {
            rollback_errors.push(format!("runtime assignment: {rollback_error}"));
        }
        *state.projects.write().await = projects_before;
        if let Err(rollback_error) = state.save_state_and_projects_rollback().await {
            rollback_errors.push(format!(
                "state/project rollback persistence: {rollback_error}"
            ));
        }
        if !rollback_errors.is_empty() {
            let detail = format!(
                "project assignment persistence failed ({error}); rollback is incomplete: {}",
                rollback_errors.join("; ")
            );
            *state.background_persistence_error.write().await = Some(detail.clone());
            return Err(ApiError::internal(detail));
        }
        return Err(ApiError::internal(format!(
            "project assignment could not be persisted and was rolled back: {error}"
        )));
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Logs : GET /processes/:id/logs?lines=N&type=all|stdout|stderr&date=YYYY-MM-DD
async fn get_logs(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    use crate::logging::reader::{read_merged_logs, read_merged_logs_for_date};
    use chrono::NaiveDate;

    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let lines: usize = params
        .get("lines")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .clamp(1, crate::logging::reader::MAX_LOG_LINES);
    let stream_filter = params
        .get("type")
        .cloned()
        .unwrap_or_else(|| "all".to_string());
    if !matches!(stream_filter.as_str(), "all" | "stdout" | "stderr") {
        return Err(ApiError::bad_request(
            "log type must be all, stdout, or stderr",
        ));
    }
    let date_param = match params.get("date") {
        Some(date) => Some(
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?,
        ),
        None => None,
    };

    let log_dir = crate::config::paths::process_log_dir(&info.name);
    let _io_permit = acquire_blocking_io(&state)?;

    let merged = tokio::task::spawn_blocking(move || match date_param {
        Some(date) => read_merged_logs_for_date(&log_dir, date, lines),
        None => read_merged_logs(&log_dir, lines),
    })
    .await
    .map_err(|error| ApiError::internal(format!("log read task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("failed to read process logs: {error}")))?;

    let filtered: Vec<_> = merged
        .into_iter()
        .filter(|(s, _, _)| stream_filter == "all" || s == &stream_filter)
        .map(|(stream, ts, content)| json!({ "stream": stream, "timestamp": ts, "content": content }))
        .collect();

    Ok(Json(json!({ "lines": filtered })))
}

// @group APIEndpoints > Logs : GET /processes/:id/logs/dates — list available rotated log dates + current log presence
async fn get_log_dates(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let log_dir = crate::config::paths::process_log_dir(&info.name);
    let _io_permit = acquire_blocking_io(&state)?;
    let (has_current, dates) = tokio::task::spawn_blocking(move || {
        let has_current = log_dir.join("out.log").exists() || log_dir.join("err.log").exists();
        crate::logging::reader::list_log_dates(&log_dir).map(|dates| (has_current, dates))
    })
    .await
    .map_err(|error| ApiError::internal(format!("log date task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("failed to list log dates: {error}")))?;
    let dates = dates
        .into_iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect::<Vec<_>>();
    Ok(Json(json!({ "dates": dates, "has_current": has_current })))
}

// @group APIEndpoints > Logs : GET /processes/:id/logs/stream (SSE)
async fn stream_logs(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<
    axum::response::Sse<
        impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    ApiError,
> {
    use axum::response::sse::Event;
    use axum::response::Sse;
    use tokio::time::{timeout, Duration};

    let id = resolve(&state, &id_str).await?;
    let mut rx = state
        .manager
        .subscribe_logs(id)
        .await
        .map_err(ApiError::from)?;

    let event_stream = async_stream::stream! {
        loop {
            match timeout(Duration::from_secs(15), rx.recv()).await {
                // Got a log line — send it
                Ok(Ok(line)) => {
                    let data = serde_json::json!({
                        "timestamp": line.timestamp.to_rfc3339(),
                        "stream": format!("{:?}", line.stream).to_lowercase(),
                        "content": line.content,
                    });
                    yield Ok(Event::default().data(data.to_string()));
                }
                // Broadcast channel closed (process deleted) — end stream
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                // Client is too slow — skip missed messages and continue
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    tracing::warn!("SSE client lagged by {n} messages");
                }
                // 15s timeout — send a keepalive comment to detect dead connections
                // If the client is gone, the next yield will fail and axum drops the stream
                Err(_) => {
                    yield Ok(Event::default().comment("keepalive"));
                }
            }
        }
    };

    Ok(Sse::new(event_stream))
}

// @group APIEndpoints > Process : PATCH /processes/:id — update config and apply
async fn update_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(req): Json<UpdateProcessRequest>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;

    // Preserve fields omitted by the compact edit request, including secrets
    // that are intentionally returned to the browser as a sentinel only.
    let existing = state.manager.get(id).await.map_err(ApiError::from)?;
    let existing_config = state.manager.get_config(id).await.map_err(ApiError::from)?;
    if req
        .project_id
        .is_some_and(|project_id| Some(project_id) != existing_config.project_id)
    {
        return Err(ApiError::conflict(
            "use the dedicated project assignment endpoint to change project_id",
        ));
    }
    let name = req.name.unwrap_or_else(|| existing_config.name.clone());
    let namespace = req
        .namespace
        .unwrap_or_else(|| existing_config.namespace.clone());
    let cron = req.cron.resolve_optional(existing_config.cron.clone());

    let mut env = req
        .env
        .resolve_value(existing_config.env.clone(), HashMap::new());
    for (key, value) in &mut env {
        if value == crate::models::notification::MASKED_SECRET {
            *value = existing_config.env.get(key).cloned().unwrap_or_default();
        }
    }

    let notify = match req.notify {
        PatchField::Missing => existing_config.notify.clone(),
        PatchField::Null => None,
        PatchField::Value(mut config) => {
            if let Some(current) = existing_config.notify.as_ref() {
                config.preserve_masked_secrets(current);
            }
            Some(config)
        }
    };
    if let Some(config) = notify.as_ref() {
        config
            .validate()
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    }

    let max_log_size_mb = validate_max_log_size_mb(
        req.max_log_size_mb
            .unwrap_or(existing_config.max_log_size_mb),
    )?;
    let config = AppConfig {
        name,
        project_id: existing_config.project_id,
        script: req.script.unwrap_or_else(|| existing_config.script.clone()),
        args: req.args.unwrap_or_else(|| existing_config.args.clone()),
        cwd: req.cwd.resolve_optional(existing_config.cwd.clone()),
        instances: existing_config.instances,
        autorestart: req.autorestart.unwrap_or(existing_config.autorestart),
        max_restarts: req.max_restarts.unwrap_or(existing_config.max_restarts),
        restart_delay_ms: req
            .restart_delay_ms
            .unwrap_or(existing_config.restart_delay_ms),
        namespace,
        watch: req.watch.unwrap_or(existing_config.watch),
        watch_paths: req
            .watch_paths
            .unwrap_or_else(|| existing_config.watch_paths.clone()),
        watch_ignore: req
            .watch_ignore
            .unwrap_or_else(|| existing_config.watch_ignore.clone()),
        env,
        log_file: existing_config.log_file.clone(),
        error_file: existing_config.error_file.clone(),
        max_log_size_mb,
        cron,
        cron_last_run: existing_config.cron_last_run,
        cron_next_run: existing_config.cron_next_run,
        notify,
        log_alert: req
            .log_alert
            .resolve_optional(existing_config.log_alert.clone()),
        env_file: existing_config.env_file.clone(),
        health_check_url: existing_config.health_check_url.clone(),
        health_check_interval_secs: existing_config.health_check_interval_secs,
        health_check_timeout_secs: existing_config.health_check_timeout_secs,
        health_check_retries: existing_config.health_check_retries,
        pre_start: existing_config.pre_start.clone(),
        post_start: existing_config.post_start.clone(),
        pre_stop: existing_config.pre_stop.clone(),
        enabled: existing.enabled,
    };
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if matches!(
        existing.status,
        ProcessStatus::Starting | ProcessStatus::Stopping
    ) {
        return Err(ApiError::conflict(
            "process is busy starting or stopping; retry the update",
        ));
    }
    let before = state
        .manager
        .snapshot_one(id)
        .await
        .map_err(ApiError::from)?;

    let info = state
        .manager
        .update(id, config)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.restore_snapshot(before).await {
            rollback_errors.push(format!("runtime restore: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process update",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok(Json(json!(info)))
}

// @group APIEndpoints > Process : POST /processes/:id/terminal
// Opens a new visible terminal window in the process's working directory.
// On Windows: spawns Windows Terminal (wt) falling back to cmd.exe.
// On Unix: spawns xterm as a fallback.
async fn open_terminal(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.unwrap_or_else(|| ".".to_string());

    #[cfg(target_os = "windows")]
    {
        // Try Windows Terminal first, fall back to cmd.exe
        let launched = std::process::Command::new("wt")
            .args(["--startingDirectory", &cwd])
            .spawn()
            .is_ok();
        if !launched {
            std::process::Command::new("cmd")
                .args(["/C", "start", "cmd.exe"])
                .current_dir(&cwd)
                .spawn()
                .map_err(|e| ApiError::internal(format!("failed to open terminal: {e}")))?;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xterm")
            .current_dir(&cwd)
            .spawn()
            .map_err(|e| ApiError::internal(format!("failed to open terminal: {e}")))?;
    }

    Ok(Json(
        json!({ "success": true, "message": "terminal opened" }),
    ))
}

// @group APIEndpoints > Process : GET /processes/:id/cron/history
async fn get_cron_history(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    Ok(Json(json!({ "runs": info.cron_run_history })))
}

// @group APIEndpoints > LogStats : GET /processes/:id/logs/stats — full-day 5-minute log volume buckets read from disk
async fn get_log_stats(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let log_dir = crate::config::paths::process_log_dir(&info.name);
    let _io_permit = acquire_blocking_io(&state)?;
    let buckets =
        tokio::task::spawn_blocking(move || crate::logging::reader::read_log_stats_today(&log_dir))
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!("task join error: {e}")))?
            .map_err(ApiError::from)?;
    Ok(Json(json!({ "buckets": buckets })))
}

// @group APIEndpoints > Metrics : GET /processes/:id/metrics/history — rolling CPU + memory samples
async fn get_metrics_history(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let samples = state.manager.get_metrics_history(id).await;
    Ok(Json(json!({ "samples": samples })))
}

// @group APIEndpoints > Logs : DELETE /processes/:id/logs — remove all log files for a process
async fn delete_logs(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    if process_is_active(&info) {
        return Err(ApiError::conflict(
            "stop the process before deleting its logs so active writers can close safely",
        ));
    }
    let log_dir = crate::config::paths::process_log_dir(&info.name);
    let _io_permit = acquire_blocking_io(&state)?;

    if log_dir.exists() {
        tokio::fs::remove_dir_all(&log_dir)
            .await
            .map_err(|e| ApiError::internal(format!("failed to delete logs: {e}")))?;
    }

    Ok(Json(json!({ "success": true, "message": "logs deleted" })))
}

// @group APIEndpoints > EnvFile : GET /processes/:id/envfiles — list all env files in process cwd
async fn list_envfiles(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.unwrap_or_else(|| ".".to_string());
    let _io_permit = acquire_blocking_io(&state)?;
    let files =
        tokio::task::spawn_blocking(move || crate::api::routes::system::list_env_files_in(&cwd))
            .await
            .map_err(|error| ApiError::internal(format!("env file list task failed: {error}")))?
            .map_err(|error| ApiError::internal(format!("failed to list env files: {error}")))?;
    let result: Vec<Value> = files
        .into_iter()
        .map(|(name, path)| json!({ "name": name, "path": path }))
        .collect();
    Ok(Json(json!({ "files": result })))
}

// @group APIEndpoints > EnvFile : GET /processes/:id/envfile?filename=.env — read env file from process cwd
async fn get_envfile(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.unwrap_or_else(|| ".".to_string());
    let _io_permit = acquire_blocking_io(&state)?;
    let filename = params
        .get("filename")
        .cloned()
        .unwrap_or_else(|| ".env".to_string());
    let filename_for_read = filename.clone();
    let (env_path, content) = tokio::task::spawn_blocking(move || {
        let env_path = crate::config::env_file::resolve_process_env_path(
            std::path::Path::new(&cwd),
            &filename_for_read,
        )?;
        if !env_path.exists() {
            return Ok::<_, anyhow::Error>((env_path, None));
        }
        let content = crate::config::env_file::read_env_file_text(
            &env_path,
            crate::config::env_file::MAX_ENV_FILE_BYTES,
        )?;
        Ok((env_path, Some(content)))
    })
    .await
    .map_err(|error| ApiError::internal(format!("env file read task failed: {error}")))?
    .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let Some(content) = content else {
        return Ok(Json(
            json!({ "content": "", "exists": false, "filename": filename, "path": env_path.to_string_lossy() }),
        ));
    };

    Ok(Json(
        json!({ "content": content, "exists": true, "filename": filename }),
    ))
}

// @group APIEndpoints > EnvFile : PUT /processes/:id/envfile — write env file to process cwd
// Body: { content, filename? } — filename defaults to ".env"
async fn put_envfile(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, ApiError> {
    let _state_guard = state.state_mutation_lock.lock().await;
    let _config_guard = state.config_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.as_deref().unwrap_or(".");
    let filename = match body.get("filename") {
        None | Some(Value::Null) => ".env",
        Some(Value::String(filename)) => filename.as_str(),
        Some(_) => {
            return Err(ApiError::bad_request(
                "filename must be a string when provided",
            ));
        }
    };
    let env_path =
        crate::config::env_file::resolve_process_env_path(std::path::Path::new(cwd), filename)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let content = body
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("content is required and must be a string"))?;
    if content.len() > crate::config::env_file::MAX_ENV_FILE_BYTES as usize {
        return Err(ApiError::bad_request(format!(
            "env file cannot exceed {} bytes",
            crate::config::env_file::MAX_ENV_FILE_BYTES
        )));
    }
    let content = content.as_bytes().to_vec();
    let write_path = env_path.clone();
    tokio::task::spawn_blocking(move || {
        crate::config::atomic_file::write_with_backup(&write_path, &content, None)
    })
    .await
    .map_err(|e| ApiError::internal(format!("env write task failed: {e}")))?
    .map_err(|e| ApiError::internal(format!("failed to write env file: {e}")))?;

    Ok(Json(
        json!({ "success": true, "path": env_path.to_string_lossy(), "filename": filename }),
    ))
}

// @group APIEndpoints > Namespace : POST /processes/namespace/:ns/start
async fn start_namespace_processes(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
) -> (StatusCode, Json<Value>) {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let before = namespace_runtime_baseline(&state, &ns).await;
    let result = state.manager.start_namespace(&ns).await;
    finish_namespace_operation(
        Arc::clone(&state),
        ns,
        result,
        crate::notifications::sender::ProcessEvent::Started,
        "started",
        before,
    )
    .await
}

// @group APIEndpoints > Namespace : POST /processes/namespace/:ns/stop
async fn stop_namespace_processes(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
) -> (StatusCode, Json<Value>) {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let before = namespace_runtime_baseline(&state, &ns).await;
    let result = state.manager.stop_namespace(&ns).await;
    finish_namespace_operation(
        Arc::clone(&state),
        ns,
        result,
        crate::notifications::sender::ProcessEvent::Stopped,
        "stopped",
        before,
    )
    .await
}

// @group APIEndpoints > Namespace : POST /processes/namespace/:ns/restart
async fn restart_namespace_processes(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
) -> (StatusCode, Json<Value>) {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let before = namespace_runtime_baseline(&state, &ns).await;
    let result = state.manager.restart_namespace(&ns).await;
    finish_namespace_operation(
        Arc::clone(&state),
        ns,
        result,
        crate::notifications::sender::ProcessEvent::Restarted,
        "restarted",
        before,
    )
    .await
}

async fn finish_namespace_operation(
    state: Arc<DaemonState>,
    namespace: String,
    result: crate::process::manager::BulkProcessResult,
    event: crate::notifications::sender::ProcessEvent,
    count_key: &'static str,
    before: HashMap<Uuid, ManagedProcessSnapshot>,
) -> (StatusCode, Json<Value>) {
    let persistence_error = if result.processes.is_empty() {
        None
    } else {
        match state.save_to_disk().await {
            Ok(()) => None,
            Err(error) => {
                let mut rollback_errors = Vec::new();
                for process in &result.processes {
                    let Some(snapshot) = before.get(&process.id).cloned() else {
                        continue;
                    };
                    if let Err(rollback_error) = state.manager.restore_snapshot(snapshot).await {
                        rollback_errors.push(format!("{}: {rollback_error}", process.name));
                    }
                }
                if let Err(rollback_error) = state.save_state_rollback().await {
                    rollback_errors.push(format!("rollback persistence failed: {rollback_error}"));
                }
                let detail = if rollback_errors.is_empty() {
                    format!("{error}; runtime rollback completed")
                } else {
                    format!("{error}; rollback issues: {}", rollback_errors.join("; "))
                };
                if !rollback_errors.is_empty() {
                    *state.background_persistence_error.write().await = Some(format!(
                        "namespace {namespace} persistence rollback is incomplete: {detail}"
                    ));
                }
                Some(detail)
            }
        }
    };

    if !result.processes.is_empty() && persistence_error.is_none() {
        let infos = result.processes.clone();
        let namespace_for_notification = namespace.clone();
        let notifications = Arc::clone(&state.notifications);
        tokio::spawn(async move {
            crate::telegram::commands::fire_telegram_namespace_notification(
                &namespace_for_notification,
                event,
                &infos,
            )
            .await;
            let store = notifications.read().await;
            crate::notifications::sender::fire_namespace_event(
                &store,
                &namespace_for_notification,
                &infos,
                event,
            )
            .await;
        });
    }

    let succeeded = if persistence_error.is_some() {
        0
    } else {
        result.processes.len()
    };
    let failed = result.failures.len()
        + if persistence_error.is_some() {
            result.processes.len()
        } else {
            0
        };
    let operation_status = if failed > 0 {
        "partial"
    } else if result.attempted == 0 {
        "empty"
    } else {
        "complete"
    };
    let status = if failed > 0 {
        StatusCode::MULTI_STATUS
    } else {
        StatusCode::OK
    };

    (
        status,
        Json(json!({
            "status": operation_status,
            "namespace": namespace,
            "attempted": result.attempted,
            "succeeded": succeeded,
            "failed": failed,
            (count_key): succeeded,
            "processes": result.processes,
            "failures": result.failures,
            "persistence": {
                "status": if persistence_error.is_some() { "failed" } else { "committed" },
                "error": persistence_error,
            },
        })),
    )
}

async fn namespace_runtime_baseline(
    state: &DaemonState,
    namespace: &str,
) -> HashMap<Uuid, ManagedProcessSnapshot> {
    state
        .manager
        .snapshot()
        .await
        .into_iter()
        .filter(|snapshot| snapshot.info.namespace == namespace)
        .map(|snapshot| (snapshot.info.id, snapshot))
        .collect()
}

// @group APIEndpoints > Process : POST /processes/:id/clone
// Duplicates an existing process config under a new name. Body: { name?: string }
// If name is omitted, appends "-copy" (or "-copy-2", "-copy-3", ...) to the original name.
async fn clone_process(
    State(state): State<Arc<DaemonState>>,
    Path(id_str): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let entry = state
        .manager
        .registry
        .get(&id)
        .ok_or_else(|| ApiError::not_found("process not found"))?;
    let src_config = entry.read().await.config.clone();
    drop(entry);

    // Determine a unique clone name
    let base_name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-copy", src_config.name));

    let existing_names: std::collections::HashSet<String> = state
        .manager
        .list()
        .await
        .into_iter()
        .map(|p| p.name)
        .collect();

    let clone_name = if !existing_names.contains(&base_name) {
        base_name.clone()
    } else {
        let mut n = 2u32;
        loop {
            let candidate = format!("{base_name}-{n}");
            if !existing_names.contains(&candidate) {
                break candidate;
            }
            n += 1;
        }
    };

    let clone_config = AppConfig {
        name: clone_name,
        project_id: src_config.project_id,
        script: src_config.script,
        args: src_config.args,
        cwd: src_config.cwd,
        instances: 1,
        autorestart: src_config.autorestart,
        max_restarts: src_config.max_restarts,
        restart_delay_ms: src_config.restart_delay_ms,
        namespace: src_config.namespace,
        watch: src_config.watch,
        watch_paths: src_config.watch_paths,
        watch_ignore: src_config.watch_ignore,
        env: src_config.env,
        log_file: None,
        error_file: None,
        max_log_size_mb: src_config.max_log_size_mb,
        cron: src_config.cron,
        cron_last_run: None,
        cron_next_run: None,
        notify: src_config.notify,
        log_alert: src_config.log_alert,
        env_file: None,
        health_check_url: src_config.health_check_url,
        health_check_interval_secs: src_config.health_check_interval_secs,
        health_check_timeout_secs: src_config.health_check_timeout_secs,
        health_check_retries: src_config.health_check_retries,
        pre_start: src_config.pre_start,
        post_start: src_config.post_start,
        pre_stop: src_config.pre_stop,
        enabled: src_config.enabled,
    };
    clone_config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let info = state
        .manager
        .start(clone_config)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = state.manager.delete(info.id).await {
            rollback_errors.push(format!("runtime cleanup: {rollback_error}"));
        }
        return Err(finish_process_rollback(
            &state,
            "process clone",
            error.to_string(),
            rollback_errors,
        )
        .await);
    }
    Ok((StatusCode::CREATED, Json(json!(info))))
}

async fn resolve(state: &DaemonState, id_str: &str) -> Result<Uuid, ApiError> {
    if let Ok(id) = Uuid::parse_str(id_str) {
        return state
            .manager
            .get(id)
            .await
            .map(|_| id)
            .map_err(|_| ApiError::not_found(format!("process not found: {id_str}")));
    }
    let matches = state
        .manager
        .list()
        .await
        .into_iter()
        .filter(|process| process.name == id_str)
        .map(|process| process.id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [id] => Ok(*id),
        [] => Err(ApiError::not_found(format!("process not found: {id_str}"))),
        _ => Err(ApiError::conflict(format!(
            "multiple processes are named '{id_str}'; use a UUID"
        ))),
    }
}
