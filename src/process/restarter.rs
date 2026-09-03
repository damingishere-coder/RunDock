// @group BusinessLogic : Auto-restart logic with exponential backoff

use crate::process::runner::RunResult;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// Message sent from restarter to manager when a process needs to be restarted or marked errored
#[derive(Debug)]
pub enum RestartEvent {
    Restart {
        process_id: Uuid,
        generation: u64,
    },
    MaxRestartsReached {
        process_id: Uuid,
        generation: u64,
        restart_count: u32,
        exit_code: Option<i32>,
    },
    Exited {
        process_id: Uuid,
        generation: u64,
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct RestartPolicy {
    pub autorestart: bool,
    pub max_restarts: u32,
    pub restart_delay_ms: u64,
    pub restart_count: u32,
}

/// Calculates the backoff delay for a given restart attempt.
/// delay = base_ms * 2^min(attempt, 8), capped at 60 seconds
pub fn backoff_delay(base_ms: u64, attempt: u32) -> Duration {
    let multiplier = 2u64.pow(attempt.min(8));
    let ms = base_ms.saturating_mul(multiplier).min(60_000);
    Duration::from_millis(ms)
}

/// Watches for process exit and decides whether to restart.
/// Sends RestartEvent back to the manager via the provided channel.
pub async fn watch_and_restart(
    process_id: Uuid,
    generation: u64,
    policy: RestartPolicy,
    mut exit_rx: mpsc::Receiver<RunResult>,
    event_tx: mpsc::Sender<RestartEvent>,
) {
    let result = match exit_rx.recv().await {
        Some(r) => r,
        None => return,
    };

    let exit_code = result.exit_code;
    let clean_exit = exit_code == Some(0);

    if clean_exit || !policy.autorestart {
        let _ = event_tx
            .send(RestartEvent::Exited {
                process_id,
                generation,
                exit_code,
            })
            .await;
        return;
    }

    if policy.restart_count >= policy.max_restarts {
        let _ = event_tx
            .send(RestartEvent::MaxRestartsReached {
                process_id,
                generation,
                restart_count: policy.restart_count,
                exit_code,
            })
            .await;
        return;
    }

    let delay = backoff_delay(policy.restart_delay_ms, policy.restart_count);
    sleep(delay).await;

    let _ = event_tx
        .send(RestartEvent::Restart {
            process_id,
            generation,
        })
        .await;
}
