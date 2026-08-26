// @group Configuration : Log alert store — stored at %APPDATA%\alter-pm2\log_alerts.json

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_NAMESPACE_OVERRIDES: usize = 1000;
const MAX_NAMESPACE_LENGTH: usize = 128;
const MAX_STDERR_THRESHOLD: u64 = 1_000_000;
const MAX_COOLDOWN_MINS: u32 = 7 * 24 * 60;
const MAX_CHECK_INTERVAL_MINS: u32 = 24 * 60;

// @group Types > LogAlertOverride : Partial override applied at namespace or process scope
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogAlertOverride {
    /// None = inherit from parent scope
    pub enabled: Option<bool>,
    /// None = inherit from parent scope
    pub stderr_threshold: Option<u64>,
    /// None = inherit from parent scope
    pub cooldown_mins: Option<u32>,
}

impl LogAlertOverride {
    pub fn validate(&self) -> Result<()> {
        if self.stderr_threshold.is_some_and(|value| value == 0) {
            anyhow::bail!("stderr threshold must be greater than zero");
        }
        if self
            .stderr_threshold
            .is_some_and(|value| value > MAX_STDERR_THRESHOLD)
        {
            anyhow::bail!("stderr threshold exceeds the supported limit");
        }
        if self.cooldown_mins.is_some_and(|value| value == 0) {
            anyhow::bail!("cooldown must be greater than zero");
        }
        if self
            .cooldown_mins
            .is_some_and(|value| value > MAX_COOLDOWN_MINS)
        {
            anyhow::bail!("cooldown exceeds the supported limit");
        }
        Ok(())
    }
}

// @group Types > LogAlertConfig : Global log alert settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAlertConfig {
    /// Whether log-spike alerts are active globally
    pub enabled: bool,
    /// Fire an alert when stderr lines in a bucket reach or exceed this count
    pub stderr_threshold: u64,
    /// Minimum minutes between repeated alerts for the same process (spam guard)
    pub cooldown_mins: u32,
    /// How often the alert check loop runs (in minutes)
    #[serde(default = "default_check_interval")]
    pub check_interval_mins: u32,
}

fn default_check_interval() -> u32 {
    5
}

impl Default for LogAlertConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stderr_threshold: 10,
            cooldown_mins: 15,
            check_interval_mins: 5,
        }
    }
}

impl LogAlertConfig {
    fn validate(&self) -> Result<()> {
        LogAlertOverride {
            enabled: Some(self.enabled),
            stderr_threshold: Some(self.stderr_threshold),
            cooldown_mins: Some(self.cooldown_mins),
        }
        .validate()?;
        if self.check_interval_mins == 0 {
            anyhow::bail!("check interval must be greater than zero");
        }
        if self.check_interval_mins > MAX_CHECK_INTERVAL_MINS {
            anyhow::bail!("check interval exceeds the supported limit");
        }
        Ok(())
    }
}

pub fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.trim().is_empty() {
        anyhow::bail!("namespace must not be empty");
    }
    if namespace.len() > MAX_NAMESPACE_LENGTH {
        anyhow::bail!("namespace exceeds the supported length");
    }
    if namespace.chars().any(char::is_control) {
        anyhow::bail!("namespace contains control characters");
    }
    Ok(())
}

// @group Types > LogAlertStore : Global config + per-namespace overrides
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogAlertStore {
    #[serde(default)]
    pub global: LogAlertConfig,
    /// Per-namespace threshold / cooldown / enabled overrides
    #[serde(default)]
    pub namespaces: HashMap<String, LogAlertOverride>,
}

impl LogAlertStore {
    pub fn validate(&self) -> Result<()> {
        self.global.validate()?;
        if self.namespaces.len() > MAX_NAMESPACE_OVERRIDES {
            anyhow::bail!("too many namespace overrides");
        }
        for (namespace, config) in &self.namespaces {
            validate_namespace(namespace)?;
            config.validate()?;
        }
        Ok(())
    }

    // @group BusinessLogic > LogAlerts : Resolve effective (enabled, threshold, cooldown) for a process
    // Priority: process override → namespace override → global
    pub fn resolve(
        &self,
        namespace: &str,
        proc_override: Option<&LogAlertOverride>,
    ) -> (bool, u64, u32) {
        let g = &self.global;
        let ns = self.namespaces.get(namespace);

        let enabled = proc_override
            .and_then(|o| o.enabled)
            .or_else(|| ns.and_then(|o| o.enabled))
            .unwrap_or(g.enabled);

        let threshold = proc_override
            .and_then(|o| o.stderr_threshold)
            .or_else(|| ns.and_then(|o| o.stderr_threshold))
            .unwrap_or(g.stderr_threshold);

        let cooldown = proc_override
            .and_then(|o| o.cooldown_mins)
            .or_else(|| ns.and_then(|o| o.cooldown_mins))
            .unwrap_or(g.cooldown_mins);

        (enabled, threshold, cooldown)
    }
}

// @group Configuration : Load log alert store from disk. Missing is first-run;
// corruption requires a valid backup and is never silently overwritten.
pub fn load() -> Result<LogAlertStore> {
    let path = crate::config::paths::data_dir().join("log_alerts.json");
    crate::config::atomic_file::load_json_with_backup_validated(&path, LogAlertStore::validate)
}

/// Background alert evaluation fails closed if configuration cannot be read.
pub fn load_fail_closed() -> LogAlertStore {
    match load() {
        Ok(store) => store,
        Err(error) => {
            tracing::error!("log alert config is unreadable; alerts disabled: {error}");
            LogAlertStore::default()
        }
    }
}

// @group Configuration : Atomically persist log alert store to disk
pub fn save(store: &LogAlertStore) -> Result<()> {
    store.validate()?;
    let path = crate::config::paths::data_dir().join("log_alerts.json");
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        store,
        LogAlertStore::validate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_and_unbounded_alert_values() {
        let mut store = LogAlertStore::default();
        store.global.stderr_threshold = 0;
        assert!(store.validate().is_err());

        store.global.stderr_threshold = 10;
        store.global.cooldown_mins = MAX_COOLDOWN_MINS + 1;
        assert!(store.validate().is_err());
    }

    #[test]
    fn rejects_invalid_namespace_keys() {
        assert!(validate_namespace("").is_err());
        assert!(validate_namespace("bad\nnamespace").is_err());
        assert!(validate_namespace(&"x".repeat(MAX_NAMESPACE_LENGTH + 1)).is_err());
    }
}
