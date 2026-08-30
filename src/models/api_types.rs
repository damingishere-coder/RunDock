// @group Types : REST API request and response structs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Presence-aware PATCH field: missing preserves the current value, JSON null
/// clears it, and a concrete value replaces it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchField<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T> PatchField<T> {
    pub fn resolve_optional(self, current: Option<T>) -> Option<T> {
        match self {
            Self::Missing => current,
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }

    pub fn resolve_value(self, current: T, cleared: T) -> T {
        match self {
            Self::Missing => current,
            Self::Null => cleared,
            Self::Value(value) => value,
        }
    }
}

impl<'de, T> Deserialize<'de> for PatchField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

// @group Types > Request : Start a new process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRequest {
    pub name: Option<String>,
    pub project_id: Option<Uuid>,
    pub script: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub autorestart: Option<bool>,
    pub max_restarts: Option<u32>,
    pub restart_delay_ms: Option<u64>,
    pub namespace: Option<String>,
    pub watch: Option<bool>,
    pub watch_paths: Option<Vec<String>>,
    pub watch_ignore: Option<Vec<String>>,
    pub max_log_size_mb: Option<u64>,
    /// Cron expression for scheduled execution (e.g. "0 * * * *")
    pub cron: Option<String>,
    /// Process-level notification override
    pub notify: Option<crate::models::notification::NotificationConfig>,
    /// Process-level log alert override
    pub log_alert: Option<crate::config::log_alert_config::LogAlertOverride>,
}

// @group Types > Request : Presence-aware partial process update
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateProcessRequest {
    pub name: Option<String>,
    pub project_id: Option<Uuid>,
    pub script: Option<String>,
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: PatchField<String>,
    #[serde(default)]
    pub env: PatchField<HashMap<String, String>>,
    pub autorestart: Option<bool>,
    pub max_restarts: Option<u32>,
    pub restart_delay_ms: Option<u64>,
    pub namespace: Option<String>,
    pub watch: Option<bool>,
    pub watch_paths: Option<Vec<String>>,
    pub watch_ignore: Option<Vec<String>>,
    pub max_log_size_mb: Option<u64>,
    #[serde(default)]
    pub cron: PatchField<String>,
    #[serde(default)]
    pub notify: PatchField<crate::models::notification::NotificationConfig>,
    #[serde(default)]
    pub log_alert: PatchField<crate::config::log_alert_config::LogAlertOverride>,
}

// @group Types > Request : Update only the per-process notification override
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessNotificationRequest {
    pub notify: Option<crate::models::notification::NotificationConfig>,
}

// @group Types > Request : Load an ecosystem config file
#[derive(Debug, Deserialize)]
pub struct EcosystemRequest {
    pub path: String,
}

// @group Types > Response : Generic operation response
#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
}

// @group Types > Response : Daemon health check
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub process_count: usize,
}

// @group Types > Response : Log lines response
#[derive(Debug, Serialize, Deserialize)]
pub struct LogsResponse {
    pub lines: Vec<LogLineDto>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogLineDto {
    pub timestamp: String,
    pub stream: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::{PatchField, UpdateProcessRequest};

    #[test]
    fn update_request_distinguishes_missing_null_and_value() {
        let missing: UpdateProcessRequest = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(missing.cwd, PatchField::Missing);

        let cleared: UpdateProcessRequest =
            serde_json::from_value(serde_json::json!({ "cwd": null, "cron": null })).unwrap();
        assert_eq!(cleared.cwd, PatchField::Null);
        assert_eq!(cleared.cron, PatchField::Null);

        let replaced: UpdateProcessRequest = serde_json::from_value(serde_json::json!({
            "cwd": "C:/work",
            "cron": "0 * * * * *"
        }))
        .unwrap();
        assert_eq!(replaced.cwd, PatchField::Value("C:/work".to_string()));
        assert_eq!(replaced.cron, PatchField::Value("0 * * * * *".to_string()));

        assert_eq!(
            PatchField::Missing.resolve_optional(Some("old".to_string())),
            Some("old".to_string())
        );
        assert_eq!(
            PatchField::<String>::Null.resolve_optional(Some("old".to_string())),
            None
        );
        assert_eq!(
            PatchField::Value("new".to_string()).resolve_optional(Some("old".to_string())),
            Some("new".to_string())
        );
    }
}
