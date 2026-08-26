// @group BusinessLogic : OS signal handling — graceful daemon shutdown on SIGTERM/SIGINT

use crate::daemon::state::DaemonState;
use anyhow::Result;
use std::sync::Arc;

async fn persist_and_request_shutdown(state: Arc<DaemonState>) {
    tracing::info!("shutdown signal received — saving state and stopping");
    // Make external intent sticky before persistence work. A concurrent hot
    // restart must not hand service to its replacement while this save waits.
    state.request_shutdown();
    let mut persisted = false;
    for attempt in 1u32..=6 {
        let save = async {
            let _mutation_guard = state.state_mutation_lock.lock().await;
            state.save_to_disk().await
        };
        match tokio::time::timeout(std::time::Duration::from_secs(4), save).await {
            Ok(Ok(())) => {
                persisted = true;
                break;
            }
            Ok(Err(error)) => {
                let message = format!("state save failed: {error}");
                *state.background_persistence_error.write().await = Some(message.clone());
                tracing::error!(%message, attempt, "shutdown is waiting for a durable state save");
            }
            Err(_) => {
                let message = "state save timed out after 4 seconds".to_string();
                *state.background_persistence_error.write().await = Some(message.clone());
                tracing::error!(%message, attempt, "shutdown is waiting for a durable state save");
            }
        }
        let delay = std::time::Duration::from_millis(
            (100u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(6))).min(5_000),
        );
        tokio::time::sleep(delay).await;
    }
    if persisted {
        *state.background_persistence_error.write().await = None;
    } else {
        tracing::error!(
            "shutdown is proceeding after bounded retries; the last successfully persisted snapshot remains on disk"
        );
    }
}

#[cfg(target_os = "windows")]
pub fn register_shutdown_handler(state: Arc<DaemonState>) -> Result<()> {
    use anyhow::Context;
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
    let mut cc = ctrl_c().context("failed to register Ctrl-C handler")?;
    let mut cb = ctrl_break().context("failed to register Ctrl-Break handler")?;
    let mut ccl = ctrl_close().context("failed to register Ctrl-Close handler")?;
    tokio::spawn(async move {
        tokio::select! {
            _ = cc.recv() => {},
            _ = cb.recv() => {},
            _ = ccl.recv() => {},
        }
        persist_and_request_shutdown(state).await;
    });
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn register_shutdown_handler(state: Arc<DaemonState>) -> Result<()> {
    use anyhow::Context;
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).context("failed to register SIGTERM")?;
    let mut sigint = signal(SignalKind::interrupt()).context("failed to register SIGINT")?;
    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        }
        persist_and_request_shutdown(state).await;
    });
    Ok(())
}
