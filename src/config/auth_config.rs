// @group Authentication : Auth configuration -- password hash, master token, passkeys

use anyhow::Result;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use rand_core::OsRng;
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
#[derive(Serialize, Deserialize, Clone)]
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

impl Default for AuthConfig {
    fn default() -> Self {
        Self::new_unconfigured()
    }
}

// @group Authentication : Password operations
impl AuthConfig {
    /// Create an in-memory, passwordless configuration without touching disk.
    /// This is used by isolated test harnesses that must never read or create
    /// the operator's real authentication file.
    pub fn new_unconfigured() -> Self {
        Self {
            password_hash: None,
            master_token: generate_token(),
            passkeys: vec![],
            passkey_user_id: Uuid::new_v4(),
            pin_hash: None,
            lock_timeout_mins: None,
        }
    }

    /// Whether browser/API authentication is currently enabled.
    /// A missing password means the local dashboard is intentionally passwordless.
    pub fn web_auth_enabled(&self) -> bool {
        self.password_hash.is_some()
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.master_token.len() == 64
                && self
                    .master_token
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "authentication master token is invalid"
        );
        anyhow::ensure!(self.passkeys.len() <= 64, "too many registered passkeys");
        anyhow::ensure!(
            self.lock_timeout_mins
                .is_none_or(|minutes| minutes <= 24 * 60),
            "lock timeout cannot exceed 1440 minutes"
        );
        for hash in [&self.password_hash, &self.pin_hash].into_iter().flatten() {
            anyhow::ensure!(hash.len() <= 1_024, "authentication hash is too long");
            PasswordHash::new(hash)
                .map_err(|error| anyhow::anyhow!("authentication hash is invalid: {error}"))?;
        }
        let mut credential_bytes = 0usize;
        for passkey in &self.passkeys {
            anyhow::ensure!(
                !passkey.name.trim().is_empty() && passkey.name.len() <= 128,
                "passkey name must contain between 1 and 128 bytes"
            );
            let bytes = serde_json::to_vec(&passkey.credential)?;
            anyhow::ensure!(bytes.len() <= 64 * 1024, "passkey credential is too large");
            credential_bytes = credential_bytes.saturating_add(bytes.len());
        }
        anyhow::ensure!(
            credential_bytes <= 1024 * 1024,
            "passkey credentials exceed the total size limit"
        );
        Ok(())
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
    let backup_path = path.with_extension("json.bak");
    let has_existing_config = match auth_path_is_regular_file(&path)
        .and_then(|primary| auth_path_is_regular_file(&backup_path).map(|backup| primary || backup))
    {
        Ok(present) => present,
        Err(error) => {
            tracing::error!(
                "authentication config metadata is unsafe or unreadable; dashboard is locked: {error}"
            );
            return fail_closed_config();
        }
    };
    if has_existing_config {
        match crate::config::atomic_file::load_json_with_backup_validated::<AuthConfig, _>(
            &path,
            AuthConfig::validate,
        ) {
            Ok(config) => return config,
            Err(error) => {
                tracing::error!(
                    "authentication config is unreadable; dashboard is locked until the file is repaired: {error}"
                );
                return fail_closed_config();
            }
        }
    }
    let cfg = AuthConfig::new_unconfigured();
    if let Err(error) = save(&cfg) {
        tracing::error!("failed to create authentication config; dashboard is locked: {error}");
        return fail_closed_config();
    }
    cfg
}

fn auth_path_is_regular_file(path: &std::path::Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
                "authentication path is not a regular file: {}",
                path.display()
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                anyhow::ensure!(
                    metadata.uid() == unsafe { libc::geteuid() },
                    "authentication file is not owned by the current user: {}",
                    path.display()
                );
                anyhow::ensure!(
                    metadata.mode() & 0o077 == 0,
                    "authentication file permissions are too broad (expected 0600): {}",
                    path.display()
                );
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn fail_closed_config() -> AuthConfig {
    AuthConfig {
        password_hash: Some("invalid-locked-config".to_string()),
        master_token: generate_token(),
        passkeys: vec![],
        passkey_user_id: Uuid::new_v4(),
        pin_hash: None,
        lock_timeout_mins: None,
    }
}

// @group Configuration : Atomically persist auth config to disk
pub fn save(config: &AuthConfig) -> Result<()> {
    config.validate()?;
    let path = auth_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        config,
        AuthConfig::validate,
    )
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

    #[test]
    fn auth_path_probe_rejects_non_file_metadata() {
        let directory = std::env::temp_dir().join(format!("alter-auth-path-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();

        assert!(auth_path_is_regular_file(&directory).is_err());
        assert!(!auth_path_is_regular_file(&directory.join("missing.json")).unwrap());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn auth_path_probe_rejects_group_or_world_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("alter-auth-permissions-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("auth.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(auth_path_is_regular_file(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(auth_path_is_regular_file(&path).unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
