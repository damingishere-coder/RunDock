// @group Configuration : Telegram bot configuration — stored at %APPDATA%\alter-pm2\telegram.json

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const MAX_ALLOWED_CHAT_IDS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Whether the Telegram bot is active
    pub enabled: bool,
    /// Bot token from @BotFather
    pub bot_token: Option<String>,
    /// Telegram chat IDs allowed to send commands (whitelist)
    pub allowed_chat_ids: Vec<i64>,
    /// Push notification toggles
    pub notify_on_crash: bool,
    pub notify_on_start: bool,
    pub notify_on_stop: bool,
    pub notify_on_restart: bool,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: None,
            allowed_chat_ids: vec![],
            notify_on_crash: true,
            notify_on_start: false,
            notify_on_stop: false,
            notify_on_restart: true,
        }
    }
}

impl TelegramConfig {
    /// Normalize user-controlled list values before validation/persistence.
    pub fn normalize(&mut self) {
        if let Some(token) = self.bot_token.as_mut() {
            *token = token.trim().to_string();
            if token.is_empty() {
                self.bot_token = None;
            }
        }
        self.allowed_chat_ids.retain(|id| *id != 0);
        self.allowed_chat_ids.sort_unstable();
        self.allowed_chat_ids.dedup();
    }

    /// An enabled bot must always have both credentials and an explicit allowlist.
    pub fn validate(&self) -> Result<()> {
        if self.allowed_chat_ids.len() > MAX_ALLOWED_CHAT_IDS {
            bail!("Telegram allowlist cannot contain more than {MAX_ALLOWED_CHAT_IDS} chat IDs");
        }
        if self.enabled && self.bot_token.as_deref().is_none_or(str::is_empty) {
            bail!("Telegram bot token is required before enabling the bot");
        }
        if self.enabled && self.allowed_chat_ids.is_empty() {
            bail!("At least one Telegram chat ID is required before enabling the bot");
        }
        if let Some(token) = self.bot_token.as_deref() {
            if token.len() > 256 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
                bail!("Telegram bot token must be at most 256 visible ASCII characters");
            }
        }
        Ok(())
    }
}

// @group Configuration : Load Telegram config from disk (returns default if missing)
pub fn load() -> Result<TelegramConfig> {
    let path = crate::config::paths::data_dir().join("telegram.json");
    let mut config: TelegramConfig = crate::config::atomic_file::load_json_with_backup_validated(
        &path,
        |candidate: &TelegramConfig| {
            let mut normalized = candidate.clone();
            normalized.normalize();
            normalized.validate()
        },
    )?;
    config.normalize();
    config.validate()?;
    Ok(config)
}

// @group Configuration : Persist Telegram config to disk (atomic write)
pub fn save(config: &TelegramConfig) -> Result<()> {
    let path = crate::config::paths::data_dir().join("telegram.json");
    let mut normalized = config.clone();
    normalized.normalize();
    normalized.validate()?;
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        &normalized,
        |candidate: &TelegramConfig| {
            let mut canonical = candidate.clone();
            canonical.normalize();
            canonical.validate()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_bot_requires_token_and_allowlist() {
        let mut config = TelegramConfig {
            enabled: true,
            ..TelegramConfig::default()
        };
        assert!(config.validate().is_err());

        config.bot_token = Some("token".to_string());
        assert!(config.validate().is_err());

        config.allowed_chat_ids.push(123);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn normalize_trims_and_deduplicates_values() {
        let mut config = TelegramConfig {
            bot_token: Some("  token  ".to_string()),
            allowed_chat_ids: vec![3, 0, 2, 3],
            ..TelegramConfig::default()
        };

        config.normalize();

        assert_eq!(config.bot_token.as_deref(), Some("token"));
        assert_eq!(config.allowed_chat_ids, vec![2, 3]);
    }
}
