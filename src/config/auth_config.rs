// @group Authentication : Auth configuration -- password hash, master token, passkeys

use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// @group Types : Stored WebAuthn passkey credential (raw JSON for portability)
#[derive(Serialize, Deserialize, Clone)]
pub struct StoredPasskey {
    pub name: String,
    /// Serialised WebAuthn passkey -- populated when a real WebAuthn backend is wired up.
    pub credential: serde_json::Value,
    pub registered_at: DateTime<Utc>,
}

// @group Types : Auth configuration persisted to auth.json
#[derive(Serialize, Deserialize)]
pub struct AuthConfig {
    /// Argon2id hash of the dashboard password. None = not yet configured.
    pub password_hash: Option<String>,
    /// Random 64-char hex token used by the CLI to authenticate.
    /// Never expires. Never sent to the browser -- read from disk by the CLI only.
    pub master_token: String,
    /// Registered WebAuthn passkeys (Windows Hello, Touch ID, etc.)
    #[serde(default)]
    pub passkeys: Vec<StoredPasskey>,
    /// Stable user UUID for the WebAuthn user handle.
    pub passkey_user_id: Uuid,
    /// Argon2id hash of the dashboard PIN (4 or 6 digits). None = not configured.
    #[serde(default)]
    pub pin_hash: Option<String>,
    /// Auto-lock timeout in minutes. None = disabled.
    #[serde(default)]
    pub lock_timeout_mins: Option<u32>,
}

// @group Authentication : Password operations
impl AuthConfig {
    /// Whether browser/API authentication is currently enabled.
    /// A missing password means the local dashboard is intentionally passwordless.
    pub fn web_auth_enabled(&self) -> bool {
        self.password_hash.is_some()
    }

    pub fn verify_password(&self, password: &str) -> bool {
        let Some(hash) = &self.password_hash else {
            return false;
        };
        let Ok(parsed) = PasswordHash::new(hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    pub fn set_password(&mut self, password: &str) -> Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("argon2 hash error: {e}"))?
            .to_string();
        self.password_hash = Some(hash);
        Ok(())
    }

    // @group Authentication > PIN : Set a 4 or 6 digit PIN
    pub fn set_pin(&mut self, pin: &str) -> Result<()> {
        if pin.len() != 4 && pin.len() != 6 {
            return Err(anyhow::anyhow!("PIN must be exactly 4 or 6 digits"));
        }
        if !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err(anyhow::anyhow!("PIN must contain only digits"));
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("argon2 hash error: {e}"))?
            .to_string();
        self.pin_hash = Some(hash);
        Ok(())
    }

    // @group Authentication > PIN : Verify a PIN against stored hash
    pub fn verify_pin(&self, pin: &str) -> bool {
        let Some(hash) = &self.pin_hash else {
            return false;
        };
        let Ok(parsed) = PasswordHash::new(hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(pin.as_bytes(), &parsed)
            .is_ok()
    }

    // @group Authentication > PIN : Remove configured PIN
    pub fn clear_pin(&mut self) {
        self.pin_hash = None;
    }

    /// Disable browser authentication while preserving the CLI master token.
    /// PINs, passkeys, and auto-lock are meaningless without a dashboard password,
    /// so clear them at the same time to prevent a stale lock screen.
    pub fn disable_web_auth(&mut self) {
        self.password_hash = None;
        self.pin_hash = None;
        self.passkeys.clear();
        self.lock_timeout_mins = None;
    }
}

// @group Utilities : Generate a random 64-char hex token (256-bit entropy via two UUID v4s)
pub fn generate_token() -> String {
    format!(
        "{}{}",
        Uuid::new_v4().to_string().replace('-', ""),
        Uuid::new_v4().to_string().replace('-', "")
    )
}

// @group Configuration : Load auth config from disk or initialise fresh
pub fn load() -> AuthConfig {
    let path = auth_config_file();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<AuthConfig>(&content) {
                return cfg;
            }
        }
    }
    let cfg = AuthConfig {
        password_hash: None,
        master_token: generate_token(),
        passkeys: vec![],
        passkey_user_id: Uuid::new_v4(),
        pin_hash: None,
        lock_timeout_mins: None,
    };
    let _ = save(&cfg);
    cfg
}

// @group Configuration : Atomically persist auth config to disk
pub fn save(config: &AuthConfig) -> Result<()> {
    let path = auth_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &content)?;
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(&path, &content)?;
    }
    Ok(())
}

pub fn auth_config_file() -> std::path::PathBuf {
    crate::config::paths::data_dir().join("auth.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_web_auth_clears_browser_credentials_but_keeps_master_token() {
        let mut config = AuthConfig {
            password_hash: None,
            master_token: generate_token(),
            passkeys: vec![StoredPasskey {
                name: "test passkey".to_string(),
                credential: serde_json::json!({ "id": "test" }),
                registered_at: Utc::now(),
            }],
            passkey_user_id: Uuid::new_v4(),
            pin_hash: None,
            lock_timeout_mins: Some(15),
        };
        config.set_password("correct-horse-battery-staple").unwrap();
        config.set_pin("1234").unwrap();
        let master_token = config.master_token.clone();
        assert!(config.web_auth_enabled());

        config.disable_web_auth();

        assert!(!config.web_auth_enabled());
        assert!(config.password_hash.is_none());
        assert!(config.pin_hash.is_none());
        assert!(config.passkeys.is_empty());
        assert!(config.lock_timeout_mins.is_none());
        assert_eq!(config.master_token, master_token);
    }
}
