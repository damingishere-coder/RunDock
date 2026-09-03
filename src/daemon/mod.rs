// @group Exports : Daemon module re-exports

pub mod lifecycle;
pub mod server;
pub mod signals;
pub mod state;

use crate::config::daemon_config::DaemonConfig;
use crate::daemon::state::DaemonState;
use anyhow::{Context, Result};
use std::sync::Arc;

struct PidFileGuard {
    pid: u32,
    owner_token: String,
    active: bool,
}

impl PidFileGuard {
    fn new(owner_token: String) -> Self {
        Self {
            pid: std::process::id(),
            owner_token,
            active: true,
        }
    }

    fn release(&mut self) -> Result<()> {
        if self.active {
            anyhow::ensure!(
                crate::utils::pid::remove_pid_file(self.pid, &self.owner_token)?,
                "daemon PID file was not released"
            );
            self.active = false;
        }
        Ok(())
    }

    fn reacquire(&mut self) -> Result<()> {
        self.owner_token = crate::utils::pid::write_pid_file()?;
        self.active = true;
        Ok(())
    }

    fn ensure_owned(&mut self) -> Result<()> {
        match crate::utils::pid::read_pid_owner_result()? {
            Some((pid, owner_token)) if pid == self.pid && owner_token == self.owner_token => {
                self.active = true;
                Ok(())
            }
            Some((pid, _)) => anyhow::bail!(
                "daemon PID ownership was replaced by another instance for process {pid}; refusing to resume service"
            ),
            None => {
                self.active = false;
                self.reacquire()
                    .context("failed to restore daemon PID ownership")
            }
        }
    }

    async fn release_with_retry(&mut self) -> Result<()> {
        let mut last_error = None;
        for attempt in 1..=6 {
            match self.release() {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::warn!(%error, attempt, "daemon PID release attempt failed");
                    last_error = Some(error);
                    tokio::time::sleep(std::time::Duration::from_millis(100 * attempt)).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("daemon PID file was not released")))
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            tracing::error!(%error, pid = self.pid, "failed to release daemon PID ownership during cleanup");
        }
    }
}

pub(crate) async fn terminate_failed_replacement(mut child: std::process::Child) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if let Err(error) = child.kill() {
            if child.try_wait()?.is_none() {
                return Err(error).context("failed to terminate rejected replacement daemon");
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("replacement daemon did not exit within 5 seconds after termination");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    })
    .await?
}

async fn remove_restart_handoff(path: &std::path::Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "failed to remove restart handoff file");
        }
    }
}

async fn stop_replacement(child: std::process::Child, context: &'static str) -> Result<()> {
    terminate_failed_replacement(child)
        .await
        .with_context(|| format!("replacement cleanup failed during {context}"))
}

async fn wait_for_replacement_ready(
    attempt: &mut state::RestartAttempt,
    config: &DaemonConfig,
    daemon_state: &DaemonState,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        anyhow::ensure!(
            !daemon_state.external_shutdown_requested(),
            "external shutdown requested during restart handoff"
        );
        if let Some(status) = attempt.child.try_wait()? {
            anyhow::bail!("replacement daemon exited before readiness with status {status}");
        }
        match tokio::fs::read(&attempt.handoff_path).await {
            Ok(bytes) => {
                anyhow::ensure!(
                    bytes.len() <= 1_024,
                    "restart readiness handoff was oversized"
                );
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                let ready = value.get("token").and_then(serde_json::Value::as_str)
                    == Some(attempt.handoff_token.as_str())
                    && value.get("pid").and_then(serde_json::Value::as_u64)
                        == Some(attempt.child.id() as u64)
                    && value.get("phase").and_then(serde_json::Value::as_str) == Some("ready");
                if ready {
                    let client =
                        crate::client::daemon_client::DaemonClient::new(&config.host, config.port)?;
                    if client
                        .is_alive_with_timeout(std::time::Duration::from_secs(2))
                        .await
                    {
                        anyhow::ensure!(
                            !daemon_state.external_shutdown_requested(),
                            "external shutdown requested during restart handoff"
                        );
                        return Ok(());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("replacement daemon did not become healthy within 30 seconds");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Entry point for the daemon process.
/// Called when the binary is invoked with the internal --daemon flag.
pub async fn run(config: DaemonConfig) -> Result<()> {
    // Ensure data directories exist
    let data_dir = crate::config::paths::data_dir();
    let log_dir = crate::config::paths::log_dir();
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&log_dir)?;

    // Write PID file
    let pid_owner_token = crate::utils::pid::write_pid_file()?;
    let mut pid_file_guard = PidFileGuard::new(pid_owner_token);

    // @group BusinessLogic > Update : Clean up leftover .exe.old from a previous self-update (Windows only)
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let stem = exe
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let old_path = exe.with_file_name(format!("{stem}.exe.old"));
        if old_path.exists() {
            let _ = std::fs::remove_file(&old_path);
        }
    }

    // @group Configuration > Tracing : tokio-console mode (feature-gated, requires --cfg tokio_unstable)
    // Enable with: RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console
    // Then run `tokio-console` in a separate terminal to inspect async tasks live.
    #[cfg(feature = "tokio-console")]
    {
        console_subscriber::init();
        tracing::info!("tokio-console subscriber active — connect with `tokio-console`");
    }

    // @group Configuration > Tracing : Standard file-based tracing (production default)
    #[cfg(not(feature = "tokio-console"))]
    let _guard = {
        let writer = crate::logging::daemon_writer::DaemonLogWriter::new(
            config.max_log_size_mb,
            config.max_log_files,
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(writer);
        tracing_subscriber::fmt()
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();
        guard
    };

    tracing::info!("RunDock daemon starting on {}:{}", config.host, config.port);

    let state = Arc::new(DaemonState::new(config.clone())?);

    // A corrupt primary + backup state must stop startup. Silently continuing
    // with an empty registry would make every managed process disappear.
    let saved = state.load_from_disk().await.map_err(|error| {
        anyhow::anyhow!("refusing to start with unreadable daemon state: {error}")
    })?;
    state.restore(saved).await;
    state
        .save_to_disk()
        .await
        .map_err(|error| anyhow::anyhow!("failed to persist restored daemon state: {error}"))?;
    state.start_background_persistence();

    // Register OS signal handlers
    signals::register_shutdown_handler(Arc::clone(&state))?;

    loop {
        // The polling task is scoped to one server lifetime. It must be fully
        // stopped before the final persistence snapshot and before a replacement
        // daemon can take ownership of the same checkpoint files.
        let tg_state = Arc::clone(&state);
        let mut telegram_task = tokio::spawn(async move {
            crate::telegram::bot::run(tg_state).await;
        });

        // Start HTTP server (blocks until shutdown).
        let server_result = server::run(Arc::clone(&state), config.clone()).await;

        if !state.is_shutdown_requested() {
            telegram_task.abort();
        }
        if tokio::time::timeout(std::time::Duration::from_secs(10), &mut telegram_task)
            .await
            .is_err()
        {
            tracing::warn!("telegram polling did not stop within 10 seconds; aborting it");
            telegram_task.abort();
            let _ = telegram_task.await;
        }

        // Commit a final snapshot after the listener stops accepting mutations.
        // Managed child processes intentionally survive; their verified identities
        // remain in this snapshot for safe re-adoption.
        let final_save = async {
            let _mutation_guard = state.state_mutation_lock.lock().await;
            state.save_to_disk().await
        };
        let final_save_result = tokio::time::timeout(std::time::Duration::from_secs(5), final_save)
            .await
            .map_err(|_| anyhow::anyhow!("timed out while saving final daemon state"))
            .and_then(|result| result);

        if let Err(save_error) = final_save_result {
            if let Some(attempt) = state.take_restart_attempt() {
                tracing::error!(%save_error, "restart final save failed; cancelling replacement and resuming current daemon");
                let handoff_path = attempt.handoff_path;
                stop_replacement(attempt.child, "final-save failure").await?;
                remove_restart_handoff(&handoff_path).await;
                pid_file_guard
                    .ensure_owned()
                    .context("failed to restore PID ownership after final-save failure")?;
                if !state.resume_after_failed_restart() {
                    tracing::info!("external shutdown remained pending after restart cancellation");
                    break;
                }
                state.start_background_persistence();
                continue;
            }
            return Err(save_error);
        }

        if let Err(server_error) = server_result {
            if let Some(attempt) = state.take_restart_attempt() {
                tracing::error!(%server_error, "daemon server failed; cancelling pending replacement after final save");
                let handoff_path = attempt.handoff_path;
                stop_replacement(attempt.child, "server failure").await?;
                remove_restart_handoff(&handoff_path).await;
            }
            return Err(server_error).context("daemon HTTP server exited unexpectedly");
        }

        let Some(mut attempt) = state.take_restart_attempt() else {
            break;
        };
        // Release both listener and PID ownership, but keep this process alive
        // until the replacement proves a healthy post-bind API response.
        if let Err(release_error) = pid_file_guard.release_with_retry().await {
            tracing::error!(%release_error, "could not release PID ownership; cancelling replacement and resuming current daemon");
            let handoff_path = attempt.handoff_path;
            stop_replacement(attempt.child, "PID release failure").await?;
            remove_restart_handoff(&handoff_path).await;
            pid_file_guard.ensure_owned().context(
                "PID release failed and the current daemon could not prove restored ownership",
            )?;
            if !state.resume_after_failed_restart() {
                tracing::info!("external shutdown remained pending after restart cancellation");
                break;
            }
            state.start_background_persistence();
            continue;
        }
        match wait_for_replacement_ready(&mut attempt, &config, &state).await {
            Ok(()) => {
                if !state.commit_restart_handoff() {
                    tracing::info!(
                        "external shutdown won the restart handoff; stopping replacement"
                    );
                    let handoff_path = attempt.handoff_path;
                    stop_replacement(attempt.child, "external shutdown during handoff").await?;
                    remove_restart_handoff(&handoff_path).await;
                    pid_file_guard.reacquire().context(
                        "failed to reacquire PID ownership while completing external shutdown",
                    )?;
                    break;
                }
                remove_restart_handoff(&attempt.handoff_path).await;
                #[cfg(unix)]
                state
                    .manager
                    .relinquish_process_trees_after_restart_handoff()
                    .await;
                tracing::info!(pid = attempt.child.id(), "replacement daemon is healthy");
                break;
            }
            Err(error) => {
                tracing::error!(%error, "replacement daemon failed; resuming the current daemon");
                stop_replacement(attempt.child, "replacement readiness failure").await?;
                remove_restart_handoff(&attempt.handoff_path).await;
                pid_file_guard
                    .reacquire()
                    .context("failed to reacquire PID ownership after replacement failure")?;
                if !state.resume_after_failed_restart() {
                    tracing::info!("external shutdown remained pending after restart cancellation");
                    break;
                }
                state.start_background_persistence();
            }
        }
    }
    pid_file_guard.release_with_retry().await?;
    tracing::info!("RunDock daemon stopped");
    Ok(())
}
