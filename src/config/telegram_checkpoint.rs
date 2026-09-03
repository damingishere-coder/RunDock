use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

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
    load_at(&crate::config::paths::data_dir().join("telegram-offset.json"))
}

fn load_at(path: &Path) -> Result<TelegramCheckpoint> {
    crate::config::atomic_file::load_json_with_backup_validated(path, TelegramCheckpoint::validate)
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
    save_at(
        &crate::config::paths::data_dir().join("telegram-offset.json"),
        checkpoint,
    )
}

fn save_at(path: &Path, checkpoint: &TelegramCheckpoint) -> Result<()> {
    checkpoint.validate()?;
    let next = checkpoint.clone();
    crate::config::atomic_file::update_json_with_backup_validated(
        path,
        TelegramCheckpoint::validate,
        move |current| {
            ensure_monotonic(current, &next)?;
            *current = next;
            Ok(())
        },
    )
    .map(|_| ())
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

    fn checkpoint_test_path(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "rundock-telegram-checkpoint-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("telegram-offset.json")
    }

    #[test]
    fn smaller_offset_cannot_overwrite_persisted_checkpoint() {
        let path = checkpoint_test_path("monotonic");
        let token = token_fingerprint("123:secret");
        save_at(
            &path,
            &TelegramCheckpoint {
                token_fingerprint: token.clone(),
                next_update_id: 100,
            },
        )
        .unwrap();
        load_at(&path).unwrap();
        let primary_before = std::fs::read(&path).unwrap();
        let backup_before = std::fs::read(path.with_extension("json.bak")).unwrap();

        let error = save_at(
            &path,
            &TelegramCheckpoint {
                token_fingerprint: token,
                next_update_id: 99,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("backwards"));
        assert_eq!(std::fs::read(&path).unwrap(), primary_before);
        assert_eq!(
            std::fs::read(path.with_extension("json.bak")).unwrap(),
            backup_before
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_checkpoint_saves_finish_at_the_maximum_offset() {
        let path = checkpoint_test_path("concurrent");
        let token = token_fingerprint("123:secret");
        let mut writers = Vec::new();
        for offset in [12, 40, 25, 100, 70, 99] {
            let writer_path = path.clone();
            let writer_token = token.clone();
            writers.push(std::thread::spawn(move || {
                save_at(
                    &writer_path,
                    &TelegramCheckpoint {
                        token_fingerprint: writer_token,
                        next_update_id: offset,
                    },
                )
            }));
        }
        for writer in writers {
            let _ = writer.join().unwrap();
        }

        let final_checkpoint = load_at(&path).unwrap();
        assert_eq!(final_checkpoint.next_update_id, 100);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
