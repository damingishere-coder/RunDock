// @group Configuration : Notification store — load and persist notifications.json

use crate::models::notification::NotificationConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_NOTIFICATION_NAMESPACES: usize = 1000;
const MAX_NAMESPACE_LENGTH: usize = 128;

// @group Types > NotificationsStore : Global + per-namespace notification configs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationsStore {
    #[serde(default)]
    pub global: NotificationConfig,
    #[serde(default)]
    pub namespaces: HashMap<String, NotificationConfig>,
}

pub fn validate_namespace(namespace: &str) -> anyhow::Result<()> {
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

impl NotificationsStore {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.global.validate()?;
        if self.namespaces.len() > MAX_NOTIFICATION_NAMESPACES {
            anyhow::bail!("too many notification namespace overrides");
        }
        for (namespace, config) in &self.namespaces {
            validate_namespace(namespace)?;
            config.validate()?;
        }
        Ok(())
    }
}

// @group DatabaseOperations : Load and validate the notification store.
pub fn load() -> anyhow::Result<NotificationsStore> {
    let path = crate::config::paths::data_dir().join("notifications.json");
    crate::config::atomic_file::load_json_with_backup_validated(&path, NotificationsStore::validate)
}

// @group DatabaseOperations : Atomically write a validated notification store.
pub fn save(store: &NotificationsStore) -> anyhow::Result<()> {
    store.validate()?;
    let path = crate::config::paths::data_dir().join("notifications.json");
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        store,
        NotificationsStore::validate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_keys_are_bounded() {
        assert!(validate_namespace("").is_err());
        assert!(validate_namespace("bad\nnamespace").is_err());
        assert!(validate_namespace(&"x".repeat(MAX_NAMESPACE_LENGTH + 1)).is_err());
    }
}
