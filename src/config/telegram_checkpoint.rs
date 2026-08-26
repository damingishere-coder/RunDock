use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramCheckpoint {
    pub token_fingerprint: String,
    pub next_update_id: i64,
}

impl TelegramCheckpoint {
    fn validate(&self) -> Result<()> {
        if self.token_fingerprint.is_empty() {
            anyhow::ensure!(
                self.next_update_id == 0,
                "an empty Telegram checkpoint fingerprint requires offset zero"
            );
        } else {
            anyhow::ensure!(
                self.token_fingerprint.len() == 64
                    && self
                        .token_fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
                "Telegram checkpoint fingerprint must be a lowercase SHA-256 digest"
            );
        }
        anyhow::ensure!(
            self.next_update_id >= 0,
            "Telegram checkpoint update ID cannot be negative"
        );
        Ok(())
    }
}

pub fn token_fingerprint(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn load() -> Result<TelegramCheckpoint> {
    crate::config::atomic_file::load_json_with_backup_validated(
        &crate::config::paths::data_dir().join("telegram-offset.json"),
        TelegramCheckpoint::validate,
    )
}

fn ensure_monotonic(current: &TelegramCheckpoint, next: &TelegramCheckpoint) -> Result<()> {
    if current.token_fingerprint == next.token_fingerprint {
        anyhow::ensure!(
            next.next_update_id >= current.next_update_id,
            "refusing to move the Telegram checkpoint backwards"
        );
    }
    Ok(())
}

pub fn save(checkpoint: &TelegramCheckpoint) -> Result<()> {
    checkpoint.validate()?;
    let current = load()?;
    ensure_monotonic(&current, checkpoint)?;
    crate::config::atomic_file::write_json_with_backup_validated(
        &crate::config::paths::data_dir().join("telegram-offset.json"),
        checkpoint,
        TelegramCheckpoint::validate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_fingerprint_is_stable_without_storing_the_token() {
        let fingerprint = token_fingerprint("123:secret");
        assert_eq!(fingerprint, token_fingerprint("123:secret"));
        assert!(!fingerprint.contains("secret"));
    }

    #[test]
    fn checkpoint_rejects_invalid_fingerprint_and_offset() {
        assert!(TelegramCheckpoint::default().validate().is_ok());
        assert!(TelegramCheckpoint {
            token_fingerprint: token_fingerprint("123:secret"),
            next_update_id: -1,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn checkpoint_requires_monotonic_offsets_for_the_same_token() {
        let current = TelegramCheckpoint {
            token_fingerprint: token_fingerprint("123:secret"),
            next_update_id: 42,
        };
        let older = TelegramCheckpoint {
            token_fingerprint: current.token_fingerprint.clone(),
            next_update_id: 41,
        };

        assert!(ensure_monotonic(&current, &older).is_err());
        assert!(ensure_monotonic(&older, &current).is_ok());
        let different_token = TelegramCheckpoint {
            token_fingerprint: token_fingerprint("456:other"),
            next_update_id: 0,
        };
        assert!(ensure_monotonic(&current, &different_token).is_ok());
    }
}
