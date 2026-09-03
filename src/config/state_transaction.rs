// @group Configuration > Persistence : Crash-recoverable state/projects transaction

use crate::config::atomic_file::{self, MAX_JSON_DOCUMENT_BYTES};
use crate::config::project_store::ProjectStore;
use crate::daemon::state::SavedState;
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    CommitNext,
    RollBack,
}

#[derive(Serialize, Deserialize)]
struct TransactionMarker {
    version: u32,
    phase: TransactionPhase,
}

struct TransactionPaths {
    state_primary: PathBuf,
    projects_primary: PathBuf,
    marker: PathBuf,
    state_next: PathBuf,
    projects_next: PathBuf,
    state_previous: PathBuf,
    projects_previous: PathBuf,
}

impl TransactionPaths {
    fn new() -> Self {
        let root = crate::config::paths::data_dir();
        Self::from_root(&root)
    }

    fn from_root(root: &Path) -> Self {
        Self {
            state_primary: root.join("state.json"),
            projects_primary: root.join("projects.json"),
            marker: root.join("state-project-transaction.json"),
            state_next: root.join("state.next.json"),
            projects_next: root.join("projects.next.json"),
            state_previous: root.join("state.previous.json"),
            projects_previous: root.join("projects.previous.json"),
        }
    }

    fn cleanup(&self) -> Result<()> {
        // Durably rename the active marker out of the well-known path before
        // deleting stages. On Unix the helper fsyncs the directory; on Windows
        // MoveFileEx uses WRITE_THROUGH. A resurrected cleanup artifact is inert.
        let inactive_marker = self.marker.with_file_name(format!(
            ".state-project-transaction.{}.completed",
            Uuid::new_v4()
        ));
        let inactive_marker = match std::fs::symlink_metadata(&self.marker) {
            Ok(_) => {
                atomic_file::move_file_durably(&self.marker, &inactive_marker).with_context(
                    || {
                        format!(
                            "failed to deactivate completed transaction marker: {}",
                            self.marker.display()
                        )
                    },
                )?;
                Some(inactive_marker)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        for path in [
            &self.state_next,
            &self.projects_next,
            &self.state_previous,
            &self.projects_previous,
        ] {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "failed to remove completed transaction file");
                }
            }
        }
        if let Some(path) = inactive_marker {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "failed to remove inert transaction marker");
                }
            }
        }
        Ok(())
    }
}

fn write_stage<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_JSON_DOCUMENT_BYTES,
        "transaction stage exceeds the JSON size limit"
    );
    atomic_file::write_with_backup(path, &bytes, None)
}

fn read_stage<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = atomic_file::read_bounded(path)
        .with_context(|| format!("failed to read transaction stage: {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("transaction stage is invalid: {}", path.display()))
}

fn write_marker(paths: &TransactionPaths, phase: TransactionPhase) -> Result<()> {
    write_stage(&paths.marker, &TransactionMarker { version: 1, phase })
}

fn marker_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
                "persistence transaction marker is not a regular file: {}",
                path.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to inspect persistence transaction marker: {}",
                path.display()
            )
        }),
    }
}

fn refresh_pair_backups(paths: &TransactionPaths) -> Result<()> {
    atomic_file::refresh_backup_from_primary_validated::<SavedState, _>(
        &paths.state_primary,
        SavedState::validate,
    )?;
    atomic_file::refresh_backup_from_primary_validated::<ProjectStore, _>(
        &paths.projects_primary,
        ProjectStore::validate,
    )?;
    Ok(())
}

fn apply_pair(paths: &TransactionPaths, state: &SavedState, projects: &ProjectStore) -> Result<()> {
    state.validate()?;
    projects.validate()?;
    atomic_file::write_json_with_backup_validated(
        &paths.state_primary,
        state,
        SavedState::validate,
    )?;
    atomic_file::write_json_with_backup_validated(
        &paths.projects_primary,
        projects,
        ProjectStore::validate,
    )?;
    Ok(())
}

/// Complete an interrupted transaction before either store is loaded.
pub fn recover_pending() -> Result<bool> {
    let paths = TransactionPaths::new();
    recover_paths(&paths)
}

fn recover_paths(paths: &TransactionPaths) -> Result<bool> {
    if !marker_exists(&paths.marker)? {
        return Ok(false);
    }
    let marker: TransactionMarker = read_stage(&paths.marker)?;
    anyhow::ensure!(
        marker.version == 1,
        "unsupported persistence transaction version"
    );
    let (state_path, projects_path) = match marker.phase {
        TransactionPhase::CommitNext => (&paths.state_next, &paths.projects_next),
        TransactionPhase::RollBack => (&paths.state_previous, &paths.projects_previous),
    };
    let state: SavedState = read_stage(state_path)?;
    let projects: ProjectStore = read_stage(projects_path)?;
    apply_pair(paths, &state, &projects)?;
    if marker.phase == TransactionPhase::RollBack {
        refresh_pair_backups(paths)?;
    }
    paths.cleanup()?;
    Ok(true)
}

/// Commit both logical stores together. A durable marker is written before the
/// first primary changes, so startup can deterministically finish or roll back.
pub fn commit(next_state: &SavedState, next_projects: &ProjectStore) -> Result<()> {
    next_state.validate()?;
    next_projects.validate()?;
    let paths = TransactionPaths::new();
    if marker_exists(&paths.marker)? {
        recover_pending().context("failed to recover the previous state transaction")?;
    }

    let previous_state: SavedState =
        atomic_file::load_json_with_backup_validated(&paths.state_primary, SavedState::validate)?;
    let previous_projects: ProjectStore = atomic_file::load_json_with_backup_validated(
        &paths.projects_primary,
        ProjectStore::validate,
    )?;

    let prepare = (|| -> Result<()> {
        write_stage(&paths.state_previous, &previous_state)?;
        write_stage(&paths.projects_previous, &previous_projects)?;
        write_stage(&paths.state_next, next_state)?;
        write_stage(&paths.projects_next, next_projects)?;
        // Rollback is the only new-transaction active phase. A crash or write
        // failure at any point before marker removal deterministically restores
        // the previous pair. CommitNext remains readable for compatibility
        // with transactions staged by older releases.
        write_marker(&paths, TransactionPhase::RollBack)
    })();
    if let Err(error) = prepare {
        let _ = paths.cleanup();
        return Err(error).context("failed to prepare state transaction");
    }

    if let Err(commit_error) = apply_pair(&paths, next_state, next_projects) {
        match apply_pair(&paths, &previous_state, &previous_projects) {
            Ok(()) => {
                refresh_pair_backups(&paths)
                    .context("state transaction rolled back but backup repair failed")?;
                paths.cleanup()?;
                return Err(commit_error).context("state transaction was rolled back");
            }
            Err(rollback_error) => {
                anyhow::bail!(
                    "state transaction failed ({commit_error}); rollback remains pending ({rollback_error})"
                );
            }
        }
    }

    // Persist a roll-forward marker before removing the rollback marker. If a
    // power loss makes the deletion reappear, recovery reapplies the committed
    // pair instead of reverting a transaction that already returned success.
    if let Err(marker_error) = write_marker(&paths, TransactionPhase::CommitNext) {
        match apply_pair(&paths, &previous_state, &previous_projects) {
            Ok(()) => {
                refresh_pair_backups(&paths)
                    .context("state transaction rolled back but backup repair failed")?;
                paths.cleanup()?;
                return Err(marker_error)
                    .context("state transaction commit marker failed and was rolled back");
            }
            Err(rollback_error) => {
                anyhow::bail!(
                    "state transaction commit marker failed ({marker_error}); rollback remains pending ({rollback_error})"
                );
            }
        }
    }

    paths.cleanup()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn marker_probe_distinguishes_absence_and_rejects_non_files() {
        let directory = std::env::temp_dir().join(format!("alter-state-marker-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let marker = directory.join("marker.json");

        assert!(!marker_exists(&marker).unwrap());
        std::fs::create_dir(&marker).unwrap();
        assert!(marker_exists(&marker).is_err());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovers_a_durable_commit_marker_idempotently() {
        let directory =
            std::env::temp_dir().join(format!("alter-state-transaction-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let paths = TransactionPaths::from_root(&directory);
        let state = SavedState::default();
        let projects = ProjectStore::default();
        write_stage(&paths.state_next, &state).unwrap();
        write_stage(&paths.projects_next, &projects).unwrap();
        write_marker(&paths, TransactionPhase::CommitNext).unwrap();

        assert!(recover_paths(&paths).unwrap());
        assert!(!paths.marker.exists());
        assert!(paths.state_primary.exists());
        assert!(paths.projects_primary.exists());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn active_rollback_marker_restores_primaries_and_recovery_copies() {
        let directory =
            std::env::temp_dir().join(format!("alter-state-rollback-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let paths = TransactionPaths::from_root(&directory);
        let previous_state = SavedState::default();
        let next_state = SavedState {
            saved_at: Some(chrono::Utc::now()),
            ..SavedState::default()
        };
        let projects = ProjectStore::default();

        apply_pair(&paths, &previous_state, &projects).unwrap();
        write_stage(&paths.state_previous, &previous_state).unwrap();
        write_stage(&paths.projects_previous, &projects).unwrap();
        write_stage(&paths.state_next, &next_state).unwrap();
        write_stage(&paths.projects_next, &projects).unwrap();
        write_marker(&paths, TransactionPhase::RollBack).unwrap();

        // Simulate a partial commit. The rollback intent was durable before
        // either primary changed, so the failure path needs no new write.
        apply_pair(&paths, &next_state, &projects).unwrap();
        assert!(recover_paths(&paths).unwrap());

        let primary: SavedState = atomic_file::load_json_with_backup(&paths.state_primary).unwrap();
        let backup: SavedState = serde_json::from_slice(
            &std::fs::read(paths.state_primary.with_extension("json.bak")).unwrap(),
        )
        .unwrap();
        assert!(primary.saved_at.is_none());
        assert!(backup.saved_at.is_none());
        assert!(!paths.marker.exists());

        std::fs::remove_dir_all(directory).unwrap();
    }
}
