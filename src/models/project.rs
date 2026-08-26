// @group Types : Logical project metadata and aggregate API projections

use crate::models::process_status::ProcessStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_PROJECT_CATEGORY: &str = "常用";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    #[default]
    Managed,
    Desktop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: Uuid,
    #[serde(default)]
    pub kind: ProjectKind,
    pub display_name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub web_port: Option<u16>,
    #[serde(default)]
    pub launch_uri: Option<String>,
}

fn default_category() -> String {
    DEFAULT_PROJECT_CATEGORY.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Desktop,
    Running,
    Partial,
    Stopped,
    Errored,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemberInfo {
    pub id: Uuid,
    pub name: String,
    pub status: ProcessStatus,
    pub pid: Option<u32>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: Uuid,
    pub kind: ProjectKind,
    pub display_name: String,
    pub note: String,
    pub category: String,
    pub web_port: Option<u16>,
    pub launch_uri: Option<String>,
    pub enabled: bool,
    pub status: ProjectStatus,
    pub process_count: usize,
    pub active_process_count: usize,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub members: Vec<ProjectMemberInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectPatch {
    pub kind: Option<ProjectKind>,
    pub display_name: Option<String>,
    pub note: Option<String>,
    pub category: Option<String>,
    pub web_port: Option<u16>,
    pub launch_uri: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectActionMemberResult {
    pub process_id: Uuid,
    pub name: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectActionResponse {
    pub project_id: Uuid,
    pub action: String,
    pub success: bool,
    pub persistence_error: Option<String>,
    pub results: Vec<ProjectActionMemberResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssignProjectRequest {
    pub project_id: Uuid,
}
