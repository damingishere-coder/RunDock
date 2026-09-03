// @group Configuration : Ecosystem and app configuration types

use crate::config::daemon_config::DaemonConfig;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

pub const MAX_ECOSYSTEM_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_ECOSYSTEM_APPS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EcosystemConfig {
    pub daemon: Option<DaemonConfig>,
    pub apps: Vec<AppConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: String,
    /// Stable logical project membership. Older state files omit this field;
    /// in that case the process UUID is used as the effective project ID.
    #[serde(default)]
    pub project_id: Option<Uuid>,
    pub script: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default = "default_instances")]
    pub instances: u32,
    #[serde(default = "default_true")]
    pub autorestart: bool,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    #[serde(default = "default_restart_delay_ms")]
    pub restart_delay_ms: u64,
    #[serde(default)]
    pub watch: bool,
    #[serde(default)]
    pub watch_paths: Vec<String>,
    #[serde(default)]
    pub watch_ignore: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub log_file: Option<String>,
    pub error_file: Option<String>,
    #[serde(default = "default_max_log_size_mb")]
    pub max_log_size_mb: u64,
    /// Cron expression for scheduled execution (e.g. "0 * * * *")
    pub cron: Option<String>,
    pub cron_last_run: Option<DateTime<Utc>>,
    pub cron_next_run: Option<DateTime<Utc>>,
    /// Process-level notification override (takes priority over namespace and global)
    #[serde(default)]
    pub notify: Option<crate::models::notification::NotificationConfig>,
    /// Process-level log alert override (takes priority over namespace and global)
    #[serde(default)]
    pub log_alert: Option<crate::config::log_alert_config::LogAlertOverride>,

    // @group Configuration > EnvFile : Path to a .env file — vars merged with env (env wins on conflict)
    #[serde(default)]
    pub env_file: Option<String>,

    // @group Configuration > HealthCheck : HTTP or TCP probe URL (e.g. "http://localhost:8080/health" or "localhost:8080")
    #[serde(default)]
    pub health_check_url: Option<String>,
    #[serde(default = "default_health_interval")]
    pub health_check_interval_secs: u64,
    #[serde(default = "default_health_timeout")]
    pub health_check_timeout_secs: u64,
    #[serde(default = "default_health_retries")]
    pub health_check_retries: u32,

    // @group Configuration > Hooks : Shell commands run at process lifecycle events
    #[serde(default)]
    pub pre_start: Option<String>,
    #[serde(default)]
    pub post_start: Option<String>,
    #[serde(default)]
    pub pre_stop: Option<String>,

    // @group Configuration > Enabled : Whether this process is included in bulk start operations
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_namespace() -> String {
    "default".to_string()
}
fn default_instances() -> u32 {
    1
}
fn default_true() -> bool {
    true
}
fn default_max_restarts() -> u32 {
    10
}
fn default_restart_delay_ms() -> u64 {
    1000
}
fn default_max_log_size_mb() -> u64 {
    10
}
fn default_health_interval() -> u64 {
    30
}
fn default_health_timeout() -> u64 {
    5
}
fn default_health_retries() -> u32 {
    3
}

impl EcosystemConfig {
    pub fn from_file(path: &Path) -> Result<Self> {
        // Inspect and read the same open handle so a concurrent path swap
        // cannot bypass the size/type check between metadata and read.
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open config file: {}", path.display()))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("failed to inspect config file: {}", path.display()))?;
        if !metadata.is_file() {
            anyhow::bail!("ecosystem config is not a regular file");
        }
        if metadata.len() > MAX_ECOSYSTEM_FILE_BYTES {
            anyhow::bail!(
                "ecosystem config exceeds the {} byte limit",
                MAX_ECOSYSTEM_FILE_BYTES
            );
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_ECOSYSTEM_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_ECOSYSTEM_FILE_BYTES,
            "ecosystem config exceeds the {} byte limit",
            MAX_ECOSYSTEM_FILE_BYTES
        );
        let content = std::str::from_utf8(&bytes)
            .with_context(|| format!("config file is not valid UTF-8: {}", path.display()))?;

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let config: Self = match ext {
            "json" => serde_json::from_str(content).with_context(|| "failed to parse JSON config"),
            _ => toml::from_str(content).with_context(|| "failed to parse TOML config"),
        }?;
        if config.apps.len() > MAX_ECOSYSTEM_APPS {
            anyhow::bail!(
                "ecosystem config contains {} apps; the limit is {}",
                config.apps.len(),
                MAX_ECOSYSTEM_APPS
            );
        }
        for app in &config.apps {
            app.validate()?;
        }
        Ok(config)
    }

    // @group Utilities : Parse an EcosystemConfig directly from a JSON string (test helper)
    #[cfg(test)]
    fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).with_context(|| "failed to parse JSON")
    }

    // @group Utilities : Parse an EcosystemConfig directly from a TOML string (test helper)
    #[cfg(test)]
    fn from_toml(s: &str) -> Result<Self> {
        toml::from_str(s).with_context(|| "failed to parse TOML")
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "process name must contain between 1 and 128 bytes"
        );
        anyhow::ensure!(
            !self.script.trim().is_empty() && self.script.len() <= 4_096,
            "process script must contain between 1 and 4096 bytes"
        );
        anyhow::ensure!(
            self.args.len() <= 256,
            "process args cannot exceed 256 entries"
        );
        anyhow::ensure!(
            self.args.iter().all(|argument| argument.len() <= 4_096),
            "process args cannot exceed 4096 bytes each"
        );
        anyhow::ensure!(
            self.cwd.as_deref().is_none_or(|cwd| cwd.len() <= 4_096),
            "process cwd cannot exceed 4096 bytes"
        );
        anyhow::ensure!(
            (1..=128).contains(&self.instances),
            "instances must be between 1 and 128"
        );
        anyhow::ensure!(
            self.max_restarts <= 10_000,
            "max_restarts cannot exceed 10000"
        );
        anyhow::ensure!(
            self.restart_delay_ms <= 86_400_000,
            "restart_delay_ms cannot exceed one day"
        );
        anyhow::ensure!(
            !self.namespace.trim().is_empty() && self.namespace.len() <= 128,
            "namespace must contain between 1 and 128 bytes"
        );
        anyhow::ensure!(
            self.watch_paths.len() <= 256 && self.watch_ignore.len() <= 256,
            "watch path lists cannot exceed 256 entries"
        );
        anyhow::ensure!(
            self.watch_paths
                .iter()
                .chain(&self.watch_ignore)
                .all(|path| path.len() <= 4_096),
            "watch paths cannot exceed 4096 bytes"
        );
        anyhow::ensure!(
            self.env.len() <= 1_024,
            "environment cannot exceed 1024 entries"
        );
        let mut env_bytes = 0usize;
        for (key, value) in &self.env {
            anyhow::ensure!(
                !key.is_empty()
                    && key.len() <= 256
                    && !key.chars().any(|character| character.is_control() || character == '='),
                "environment variable names must be 1-256 bytes and contain no control characters or '='"
            );
            anyhow::ensure!(
                value.len() <= 64 * 1024,
                "environment values cannot exceed 64 KiB"
            );
            env_bytes = env_bytes
                .saturating_add(key.len())
                .saturating_add(value.len());
        }
        anyhow::ensure!(
            env_bytes <= 1024 * 1024,
            "environment cannot exceed 1 MiB in total"
        );
        anyhow::ensure!(
            (1..=1024).contains(&self.max_log_size_mb),
            "max_log_size_mb must be between 1 and 1024"
        );
        anyhow::ensure!(
            self.cron.as_deref().is_none_or(|cron| cron.len() <= 512),
            "cron expression cannot exceed 512 bytes"
        );
        if let Some(config) = &self.notify {
            config.validate()?;
        }
        if let Some(config) = &self.log_alert {
            config.validate()?;
        }
        if let Some(filename) = &self.env_file {
            anyhow::ensure!(
                crate::config::env_file::is_safe_env_filename(filename),
                "env_file must be a safe env filename"
            );
        }
        anyhow::ensure!(
            self.health_check_url
                .as_deref()
                .is_none_or(|url| !url.is_empty() && url.len() <= 2_048),
            "health check URL must contain between 1 and 2048 bytes"
        );
        anyhow::ensure!(
            (1..=86_400).contains(&self.health_check_interval_secs),
            "health check interval must be between 1 second and one day"
        );
        anyhow::ensure!(
            (1..=300).contains(&self.health_check_timeout_secs),
            "health check timeout must be between 1 and 300 seconds"
        );
        anyhow::ensure!(
            (1..=100).contains(&self.health_check_retries),
            "health check retries must be between 1 and 100"
        );
        for hook in [&self.pre_start, &self.post_start, &self.pre_stop] {
            anyhow::ensure!(
                hook.as_deref().is_none_or(|command| command.len() <= 8_192),
                "lifecycle hook commands cannot exceed 8192 bytes"
            );
        }
        Ok(())
    }
}

// @group UnitTests : EcosystemConfig — JSON + TOML parsing and default field values
#[cfg(test)]
mod tests {
    use super::*;

    // @group UnitTests > JSON : Minimal valid JSON config round-trips correctly
    #[test]
    fn test_parse_json_minimal() {
        let cfg =
            EcosystemConfig::from_json(r#"{"apps":[{"name":"api","script":"node index.js"}]}"#)
                .unwrap();
        assert_eq!(cfg.apps.len(), 1);
        assert_eq!(cfg.apps[0].name, "api");
        assert_eq!(cfg.apps[0].script, "node index.js");
    }

    // @group UnitTests > JSON : Default field values are applied when fields are absent
    #[test]
    fn test_json_defaults() {
        let cfg =
            EcosystemConfig::from_json(r#"{"apps":[{"name":"svc","script":"run.sh"}]}"#).unwrap();
        let app = &cfg.apps[0];
        assert_eq!(app.instances, 1);
        assert!(app.autorestart);
        assert_eq!(app.max_restarts, 10);
        assert_eq!(app.restart_delay_ms, 1000);
        assert!(!app.watch);
        assert_eq!(app.namespace, "default");
        assert_eq!(app.max_log_size_mb, 10);
        assert!(app.args.is_empty());
        assert!(app.env.is_empty());
        assert!(app.cwd.is_none());
        assert!(app.cron.is_none());
    }

    // @group UnitTests > JSON : Explicit field values override defaults
    #[test]
    fn test_json_explicit_fields() {
        let json = r#"{
            "apps": [{
                "name": "worker",
                "script": "python worker.py",
                "instances": 4,
                "autorestart": false,
                "max_restarts": 3,
                "namespace": "jobs",
                "watch": true
            }]
        }"#;
        let app = &EcosystemConfig::from_json(json).unwrap().apps[0];
        assert_eq!(app.instances, 4);
        assert!(!app.autorestart);
        assert_eq!(app.max_restarts, 3);
        assert_eq!(app.namespace, "jobs");
        assert!(app.watch);
    }

    // @group UnitTests > JSON : Empty apps list is valid
    #[test]
    fn test_json_empty_apps() {
        let cfg = EcosystemConfig::from_json(r#"{"apps":[]}"#).unwrap();
        assert!(cfg.apps.is_empty());
        assert!(cfg.daemon.is_none());
    }

    // @group UnitTests > JSON : Multiple apps are all parsed
    #[test]
    fn test_json_multiple_apps() {
        let json = r#"{"apps":[{"name":"a","script":"a.js"},{"name":"b","script":"b.js"}]}"#;
        let cfg = EcosystemConfig::from_json(json).unwrap();
        assert_eq!(cfg.apps.len(), 2);
        assert_eq!(cfg.apps[0].name, "a");
        assert_eq!(cfg.apps[1].name, "b");
    }

    // @group UnitTests > TOML : Minimal valid TOML config round-trips correctly
    #[test]
    fn test_parse_toml_minimal() {
        let toml = r#"
[[apps]]
name   = "api"
script = "node index.js"
"#;
        let cfg = EcosystemConfig::from_toml(toml).unwrap();
        assert_eq!(cfg.apps.len(), 1);
        assert_eq!(cfg.apps[0].name, "api");
    }

    // @group UnitTests > TOML : Default field values are applied when fields are absent
    #[test]
    fn test_toml_defaults() {
        let toml = "[[apps]]\nname = \"svc\"\nscript = \"run.sh\"\n";
        let app = &EcosystemConfig::from_toml(toml).unwrap().apps[0];
        assert_eq!(app.instances, 1);
        assert!(app.autorestart);
        assert_eq!(app.namespace, "default");
        assert_eq!(app.project_id, None);
    }

    // @group UnitTests > TOML : Env vars are captured as a map
    #[test]
    fn test_toml_env_vars() {
        let toml = r#"
[[apps]]
name   = "api"
script = "node server.js"
[apps.env]
PORT = "3000"
NODE_ENV = "production"
"#;
        let app = &EcosystemConfig::from_toml(toml).unwrap().apps[0];
        assert_eq!(app.env.get("PORT").map(|s| s.as_str()), Some("3000"));
        assert_eq!(
            app.env.get("NODE_ENV").map(|s| s.as_str()),
            Some("production")
        );
    }

    // @group UnitTests > EdgeCases : Missing required field "script" returns an error
    #[test]
    fn test_json_missing_required_field() {
        let result = EcosystemConfig::from_json(r#"{"apps":[{"name":"oops"}]}"#);
        assert!(result.is_err());
    }

    // @group UnitTests > EdgeCases : Malformed JSON returns an error
    #[test]
    fn test_json_malformed() {
        assert!(EcosystemConfig::from_json("not json at all").is_err());
    }

    #[test]
    fn file_loader_rejects_oversized_files_and_app_sets() {
        let directory = std::env::temp_dir().join(format!("alter-ecosystem-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();

        let oversized = directory.join("oversized.json");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_ECOSYSTEM_FILE_BYTES + 1).unwrap();
        assert!(EcosystemConfig::from_file(&oversized).is_err());

        let too_many = directory.join("too-many.json");
        let apps = (0..=MAX_ECOSYSTEM_APPS)
            .map(|index| serde_json::json!({ "name": format!("app-{index}"), "script": "noop" }))
            .collect::<Vec<_>>();
        std::fs::write(
            &too_many,
            serde_json::to_vec(&serde_json::json!({ "apps": apps })).unwrap(),
        )
        .unwrap();
        assert!(EcosystemConfig::from_file(&too_many).is_err());

        std::fs::remove_dir_all(directory).unwrap();
    }
}
