// @group APIEndpoints : Logical project aggregation, metadata, and lifecycle actions

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use crate::models::process_info::ProcessInfo;
use crate::models::process_status::ProcessStatus;
use crate::models::project::{
    ProjectActionMemberResult, ProjectActionResponse, ProjectInfo, ProjectKind, ProjectMemberInfo,
    ProjectPatch, ProjectRecord, ProjectStatus, DEFAULT_PROJECT_CATEGORY,
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(list_projects))
        .route("/{id}", get(get_project).patch(update_project))
        .route("/{id}/start", post(start_project))
        .route("/{id}/stop", post(stop_project))
        .route("/{id}/restart", post(restart_project))
        .with_state(state)
}

fn is_active(status: &ProcessStatus) -> bool {
    matches!(
        status,
        ProcessStatus::Starting
            | ProcessStatus::Running
            | ProcessStatus::Watching
            | ProcessStatus::Sleeping
    )
}

fn effective_project_id(process: &ProcessInfo) -> Uuid {
    process.project_id.unwrap_or(process.id)
}

fn group_processes(processes: Vec<ProcessInfo>) -> HashMap<Uuid, Vec<ProcessInfo>> {
    let mut grouped: HashMap<Uuid, Vec<ProcessInfo>> = HashMap::new();
    for process in processes {
        grouped
            .entry(effective_project_id(&process))
            .or_default()
            .push(process);
    }
    grouped
}

fn aggregate_status(
    members: &[ProcessInfo],
    enabled: bool,
    active_process_count: usize,
) -> ProjectStatus {
    let has_error = members.iter().any(|process| {
        matches!(
            process.status,
            ProcessStatus::Crashed | ProcessStatus::Errored
        )
    });
    if !enabled && active_process_count == 0 {
        ProjectStatus::Disabled
    } else if has_error {
        ProjectStatus::Errored
    } else if active_process_count == 0 {
        ProjectStatus::Stopped
    } else if active_process_count == members.len() {
        ProjectStatus::Running
    } else {
        ProjectStatus::Partial
    }
}

fn desktop_project_info(record: &ProjectRecord) -> ProjectInfo {
    ProjectInfo {
        id: record.id,
        kind: ProjectKind::Desktop,
        display_name: record.display_name.clone(),
        note: record.note.clone(),
        category: record.category.clone(),
        web_port: None,
        launch_uri: record.launch_uri.clone(),
        enabled: true,
        status: ProjectStatus::Desktop,
        process_count: 0,
        active_process_count: 0,
        cpu_percent: 0.0,
        memory_bytes: 0,
        members: Vec::new(),
    }
}

async fn collect_projects(state: &DaemonState) -> Vec<ProjectInfo> {
    let processes = state.manager.list().await;
    let store = state.projects.read().await.clone();
    let grouped = group_processes(processes);

    let mut seen = HashSet::new();
    let mut projects: Vec<ProjectInfo> = grouped
        .into_iter()
        .map(|(id, mut members)| {
            seen.insert(id);
            members.sort_by(|a, b| a.name.cmp(&b.name));
            let fallback_name = members
                .first()
                .map(|process| process.name.clone())
                .unwrap_or_else(|| "Project".to_string());
            let record = store.projects.get(&id);
            if let Some(record) = record.filter(|item| item.kind == ProjectKind::Desktop) {
                return desktop_project_info(record);
            }
            let enabled = members.iter().all(|process| process.enabled);
            let active_process_count = members
                .iter()
                .filter(|process| is_active(&process.status))
                .count();
            let status = aggregate_status(&members, enabled, active_process_count);

            ProjectInfo {
                id,
                kind: ProjectKind::Managed,
                display_name: record
                    .map(|item| item.display_name.clone())
                    .unwrap_or(fallback_name),
                note: record.map(|item| item.note.clone()).unwrap_or_default(),
                category: record
                    .map(|item| item.category.clone())
                    .unwrap_or_else(|| DEFAULT_PROJECT_CATEGORY.to_string()),
                web_port: record.and_then(|item| item.web_port),
                launch_uri: None,
                enabled,
                status,
                process_count: members.len(),
                active_process_count,
                cpu_percent: members
                    .iter()
                    .map(|process| process.cpu_percent.unwrap_or_default())
                    .sum(),
                memory_bytes: members
                    .iter()
                    .map(|process| process.memory_bytes.unwrap_or_default())
                    .sum(),
                members: members
                    .into_iter()
                    .map(|process| ProjectMemberInfo {
                        id: process.id,
                        name: process.name,
                        status: process.status,
                        pid: process.pid,
                        enabled: process.enabled,
                    })
                    .collect(),
            }
        })
        .collect();

    projects.extend(
        store
            .projects
            .values()
            .filter(|record| record.kind == ProjectKind::Desktop && !seen.contains(&record.id))
            .map(desktop_project_info),
    );

    projects.sort_by(|a, b| {
        category_rank(&a.category)
            .cmp(&category_rank(&b.category))
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    projects
}

fn category_rank(category: &str) -> u8 {
    match category {
        "常用" => 0,
        "待定" => 2,
        _ => 1,
    }
}

async fn find_project(state: &DaemonState, id: Uuid) -> Result<ProjectInfo, ApiError> {
    collect_projects(state)
        .await
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| ApiError::not_found(format!("project not found: {id}")))
}

async fn list_projects(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    Json(json!({ "projects": collect_projects(&state).await }))
}

async fn get_project(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(find_project(&state, id).await?)))
}

fn validate_patch(patch: &ProjectPatch) -> Result<(), ApiError> {
    if let Some(name) = &patch.display_name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(ApiError::bad_request(
                "display_name must contain 1 to 80 characters",
            ));
        }
    }
    if let Some(note) = &patch.note {
        if note.chars().count() > 500 {
            return Err(ApiError::bad_request("note must not exceed 500 characters"));
        }
    }
    if let Some(category) = &patch.category {
        let category = category.trim();
        if category.is_empty() || category.chars().count() > 40 {
            return Err(ApiError::bad_request(
                "category must contain 1 to 40 characters",
            ));
        }
    }
    if patch.web_port == Some(0) {
        return Err(ApiError::bad_request(
            "web_port must be between 1 and 65535",
        ));
    }
    if let Some(uri) = &patch.launch_uri {
        if !is_valid_desktop_launch_uri(uri) {
            return Err(ApiError::bad_request(
                "launch_uri must be a safe non-HTTP custom protocol URI",
            ));
        }
    }
    Ok(())
}

fn is_valid_desktop_launch_uri(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || !value.is_ascii() {
        return false;
    }
    if value.chars().any(|character| {
        character.is_ascii_control()
            || character.is_ascii_whitespace()
            || matches!(character, '\\' | '"' | '\'' | '<' | '>' | '`')
    }) {
        return false;
    }
    let Some((scheme, target)) = value.split_once("://") else {
        return false;
    };
    let mut scheme_chars = scheme.chars();
    if !scheme_chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        || !scheme_chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
        || target.is_empty()
        || target.starts_with('@')
        || target
            .split('/')
            .next()
            .is_some_and(|authority| authority.is_empty() || authority.contains('@'))
    {
        return false;
    }
    !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "file" | "javascript" | "data"
    )
}

async fn update_project(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<Uuid>,
    Json(patch): Json<ProjectPatch>,
) -> Result<Json<Value>, ApiError> {
    validate_patch(&patch)?;
    let project = find_project(&state, id).await?;
    let effective_kind = patch.kind.unwrap_or(project.kind);
    let effective_launch_uri = patch
        .launch_uri
        .as_ref()
        .map(|uri| uri.trim().to_string())
        .or_else(|| project.launch_uri.clone());

    if effective_kind == ProjectKind::Desktop {
        if effective_launch_uri.is_none() {
            return Err(ApiError::bad_request("desktop project requires launch_uri"));
        }
        if patch.web_port.is_some() {
            return Err(ApiError::bad_request(
                "desktop project does not support web_port",
            ));
        }
        if patch.enabled.is_some() {
            return Err(ApiError::conflict(
                "desktop project does not support enable or disable",
            ));
        }
    } else {
        if patch.launch_uri.is_some() {
            return Err(ApiError::bad_request(
                "managed project does not support launch_uri",
            ));
        }
        if project.kind == ProjectKind::Desktop && project.members.is_empty() {
            return Err(ApiError::conflict(
                "managed project requires at least one process member",
            ));
        }
    }

    {
        let mut store = state.projects.write().await;
        let record = store.ensure(id, &project.display_name);
        record.kind = effective_kind;
        if let Some(display_name) = patch.display_name {
            record.display_name = display_name.trim().to_string();
        }
        if let Some(note) = patch.note {
            record.note = note.trim().to_string();
        }
        if let Some(category) = patch.category {
            record.category = category.trim().to_string();
        }
        match effective_kind {
            ProjectKind::Desktop => {
                record.web_port = None;
                record.launch_uri = effective_launch_uri;
            }
            ProjectKind::Managed => {
                record.launch_uri = None;
                if let Some(web_port) = patch.web_port {
                    record.web_port = Some(web_port);
                }
            }
        }
    }

    if let Some(enabled) = patch.enabled {
        for member in &project.members {
            state
                .manager
                .set_enabled(member.id, enabled)
                .await
                .map_err(ApiError::from)?;
        }
        state.save_to_disk().await.map_err(ApiError::from)?;
    }

    state.save_projects().await.map_err(ApiError::from)?;
    Ok(Json(json!(find_project(&state, id).await?)))
}

#[derive(Clone, Copy)]
enum ProjectAction {
    Start,
    Stop,
    Restart,
}

impl ProjectAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

async fn run_project_action(
    state: &DaemonState,
    project_id: Uuid,
    action: ProjectAction,
) -> Result<ProjectActionResponse, ApiError> {
    let project = find_project(state, project_id).await?;
    if project.kind == ProjectKind::Desktop {
        return Err(ApiError::conflict(
            "desktop project cannot be started, stopped, or restarted",
        ));
    }
    if matches!(action, ProjectAction::Start | ProjectAction::Restart) && !project.enabled {
        return Err(ApiError::bad_request(
            "project is disabled; enable it before starting",
        ));
    }

    let mut results = Vec::with_capacity(project.members.len());
    for member in project.members {
        let active = is_active(&member.status);
        let result = match action {
            ProjectAction::Start if active => Ok(()),
            ProjectAction::Start => state.manager.start_existing(member.id).await.map(|_| ()),
            ProjectAction::Stop if !active => Ok(()),
            ProjectAction::Stop => state.manager.stop(member.id).await.map(|_| ()),
            ProjectAction::Restart => state.manager.restart(member.id).await.map(|_| ()),
        };
        results.push(ProjectActionMemberResult {
            process_id: member.id,
            name: member.name,
            success: result.is_ok(),
            error: result.err().map(|error| error.to_string()),
        });
    }

    state.save_to_disk().await.map_err(ApiError::from)?;
    let success = results.iter().all(|result| result.success);
    Ok(ProjectActionResponse {
        project_id,
        action: action.as_str().to_string(),
        success,
        results,
    })
}

async fn start_project(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(
        run_project_action(&state, id, ProjectAction::Start).await?
    )))
}

async fn stop_project(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(
        run_project_action(&state, id, ProjectAction::Stop).await?
    )))
}

async fn restart_project(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(
        run_project_action(&state, id, ProjectAction::Restart).await?
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ecosystem::AppConfig;
    use crate::process::instance::ManagedProcess;

    fn process(
        name: &str,
        project_id: Option<Uuid>,
        status: ProcessStatus,
        enabled: bool,
    ) -> ProcessInfo {
        let config: AppConfig = serde_json::from_value(json!({
            "name": name,
            "project_id": project_id,
            "script": "test",
            "cwd": null,
            "enabled": enabled
        }))
        .unwrap();
        let mut process = ManagedProcess::new(config);
        process.status = status;
        process.to_info()
    }

    #[test]
    fn category_order_places_common_first_and_pending_last() {
        assert!(category_rank("常用") < category_rank("开发"));
        assert!(category_rank("开发") < category_rank("待定"));
    }

    #[test]
    fn project_patch_validation_keeps_display_fields_bounded() {
        assert!(validate_patch(&ProjectPatch {
            kind: None,
            display_name: Some("项目".to_string()),
            note: Some("备注".to_string()),
            category: Some("常用".to_string()),
            web_port: Some(6866),
            launch_uri: None,
            enabled: None,
        })
        .is_ok());
        assert!(validate_patch(&ProjectPatch {
            kind: None,
            display_name: Some(String::new()),
            note: None,
            category: None,
            web_port: None,
            launch_uri: None,
            enabled: None,
        })
        .is_err());
        assert!(validate_patch(&ProjectPatch {
            kind: None,
            display_name: None,
            note: None,
            category: None,
            web_port: Some(0),
            launch_uri: None,
            enabled: None,
        })
        .is_err());
    }

    #[test]
    fn desktop_launch_uri_allows_custom_protocols_only() {
        assert!(is_valid_desktop_launch_uri("wanmotai://open"));
        assert!(is_valid_desktop_launch_uri(
            "my-app+beta://launch/workspace"
        ));
        assert!(!is_valid_desktop_launch_uri("https://example.com"));
        assert!(!is_valid_desktop_launch_uri(
            "file:///C:/Windows/System32/calc.exe"
        ));
        assert!(!is_valid_desktop_launch_uri("javascript://alert(1)"));
        assert!(!is_valid_desktop_launch_uri("wanmotai://user@open"));
        assert!(!is_valid_desktop_launch_uri("wanmotai:open"));
    }

    #[test]
    fn desktop_projection_has_no_process_or_web_metrics() {
        let id = Uuid::new_v4();
        let record = ProjectRecord {
            id,
            kind: ProjectKind::Desktop,
            display_name: "万模台".to_string(),
            note: "桌面入口".to_string(),
            category: "常用".to_string(),
            web_port: Some(3000),
            launch_uri: Some("wanmotai://open".to_string()),
        };

        let project = desktop_project_info(&record);
        assert_eq!(project.kind, ProjectKind::Desktop);
        assert_eq!(project.status, ProjectStatus::Desktop);
        assert_eq!(project.launch_uri.as_deref(), Some("wanmotai://open"));
        assert_eq!(project.web_port, None);
        assert_eq!(project.process_count, 0);
        assert!(project.members.is_empty());
    }

    #[test]
    fn explicit_project_id_merges_multiple_components() {
        let project_id = Uuid::new_v4();
        let groups = group_processes(vec![
            process(
                "Zhihu-Backend",
                Some(project_id),
                ProcessStatus::Running,
                true,
            ),
            process(
                "Zhihu-Frontend",
                Some(project_id),
                ProcessStatus::Stopped,
                true,
            ),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.get(&project_id).unwrap().len(), 2);
        assert_eq!(
            aggregate_status(groups.get(&project_id).unwrap(), true, 1),
            ProjectStatus::Partial
        );
    }

    #[test]
    fn disabled_stopped_components_report_disabled_project() {
        let members = vec![process(
            "QQ-Study-Bridge",
            None,
            ProcessStatus::Stopped,
            false,
        )];
        assert_eq!(
            aggregate_status(&members, false, 0),
            ProjectStatus::Disabled
        );
    }
}
