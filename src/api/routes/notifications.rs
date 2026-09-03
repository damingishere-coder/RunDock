// @group APIEndpoints : Notification settings CRUD endpoints

use crate::api::error::ApiError;
use crate::config::notification_store;
use crate::daemon::state::DaemonState;
use crate::models::notification::NotificationConfig;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(get_notifications))
        .route("/global", put(update_global))
        .route(
            "/namespace/{ns}",
            put(update_namespace).delete(delete_namespace),
        )
        .route("/test", post(test_notification))
        .with_state(state)
}

// @group APIEndpoints > Notifications : GET /notifications — return full NotificationsStore
async fn get_notifications(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let store = state.notifications.read().await;
    let mut redacted = store.clone();
    redacted.global = redacted.global.redacted();
    for config in redacted.namespaces.values_mut() {
        *config = config.redacted();
    }
    Json(json!(redacted))
}

// @group APIEndpoints > Notifications : PUT /notifications/global — update global config and persist
async fn update_global(
    State(state): State<Arc<DaemonState>>,
    Json(mut config): Json<NotificationConfig>,
) -> Result<Json<Value>, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    let current = state.notifications.read().await.clone();
    config.preserve_masked_secrets(&current.global);
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let mut candidate = current;
    candidate.global = config;
    let candidate_for_save = candidate.clone();
    tokio::task::spawn_blocking(move || notification_store::save(&candidate_for_save))
        .await
        .map_err(|error| ApiError::internal(format!("notification save task failed: {error}")))?
        .map_err(|e| ApiError::internal(format!("failed to save notifications: {e}")))?;
    *state.notifications.write().await = candidate;
    Ok(Json(
        json!({ "success": true, "message": "global notifications updated" }),
    ))
}

// @group APIEndpoints > Notifications : PUT /notifications/namespace/:ns — update namespace config and persist
async fn update_namespace(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
    Json(mut config): Json<NotificationConfig>,
) -> Result<Json<Value>, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    notification_store::validate_namespace(&ns)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let current = state.notifications.read().await.clone();
    if let Some(current) = current.namespaces.get(&ns) {
        config.preserve_masked_secrets(current);
    }
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !current.namespaces.contains_key(&ns)
        && current.namespaces.len() >= notification_store::MAX_NOTIFICATION_NAMESPACES
    {
        return Err(ApiError::bad_request(
            "too many notification namespace overrides",
        ));
    }
    let mut candidate = current;
    candidate.namespaces.insert(ns.clone(), config);
    let candidate_for_save = candidate.clone();
    tokio::task::spawn_blocking(move || notification_store::save(&candidate_for_save))
        .await
        .map_err(|error| ApiError::internal(format!("notification save task failed: {error}")))?
        .map_err(|e| ApiError::internal(format!("failed to save notifications: {e}")))?;
    *state.notifications.write().await = candidate;
    Ok(Json(
        json!({ "success": true, "message": format!("namespace '{ns}' notifications updated") }),
    ))
}

// @group APIEndpoints > Notifications : DELETE /notifications/namespace/:ns — remove namespace override
async fn delete_namespace(
    State(state): State<Arc<DaemonState>>,
    Path(ns): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    notification_store::validate_namespace(&ns)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let current = state.notifications.read().await.clone();
    if !current.namespaces.contains_key(&ns) {
        return Err(ApiError::not_found(format!("namespace '{ns}' not found")));
    }
    let mut candidate = current;
    candidate.namespaces.remove(&ns);
    let candidate_for_save = candidate.clone();
    tokio::task::spawn_blocking(move || notification_store::save(&candidate_for_save))
        .await
        .map_err(|error| ApiError::internal(format!("notification save task failed: {error}")))?
        .map_err(|e| ApiError::internal(format!("failed to save notifications: {e}")))?;
    *state.notifications.write().await = candidate;
    Ok((
        StatusCode::OK,
        Json(json!({ "success": true, "message": format!("namespace '{ns}' removed") })),
    ))
}

// @group APIEndpoints > Notifications : POST /notifications/test — fire a test notification using the supplied config
async fn test_notification(
    State(state): State<Arc<DaemonState>>,
    Json(mut config): Json<NotificationConfig>,
) -> Result<Json<Value>, ApiError> {
    use crate::config::notification_store::NotificationsStore;
    use crate::models::notification::NotificationConfig as NC;
    use crate::models::process_info::ProcessInfo;
    use crate::models::process_status::ProcessStatus;
    use crate::notifications::sender::{fire_event_report, ProcessEvent};
    use chrono::Utc;
    use std::collections::HashMap;
    use uuid::Uuid;

    let current = state.notifications.read().await.global.clone();
    config.preserve_masked_secrets(&current);
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    // Build a minimal fake NotificationsStore where the provided config is the global config
    // so fire_event picks it up unconditionally
    let mut test_events = config.events.clone();
    // Force all events on for the test so it fires regardless of toggle state
    test_events.on_start = true;

    let effective_config = NC {
        events: test_events,
        events_override: true,
        ..config
    };

    let store = NotificationsStore {
        global: effective_config,
        namespaces: HashMap::new(),
    };

    // Minimal synthetic ProcessInfo for the test payload
    let proc = ProcessInfo {
        id: Uuid::new_v4(),
        name: "test-process".to_string(),
        project_id: None,
        script: "test.js".to_string(),
        args: vec![],
        cwd: None,
        status: ProcessStatus::Running,
        pid: None,
        restart_count: 0,
        uptime_secs: None,
        last_exit_code: None,
        autorestart: false,
        max_restarts: 0,
        watch: false,
        namespace: state
            .notifications
            .read()
            .await
            .namespaces
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "default".to_string()),
        created_at: Utc::now(),
        started_at: None,
        stopped_at: None,
        cron: None,
        cron_next_run: None,
        cron_run_history: vec![],
        cpu_percent: None,
        memory_bytes: None,
        env: HashMap::new(),
        notify: None,
        log_alert: None,
        health_status: None,
        git_branch: None,
        enabled: true,
    };

    let report = fire_event_report(&store, &proc, ProcessEvent::Started).await;
    if report.attempted == 0 {
        return Err(ApiError::bad_request(
            "no enabled notification target was provided",
        ));
    }
    if !report.errors.is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!(
                "test notification failed for {} of {} target(s): {}",
                report.errors.len(),
                report.attempted,
                report.errors.join("; ")
            ),
        });
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("test notification delivered to {} target(s)", report.delivered)
    })))
}
