// @group BusinessLogic : Shared daemon probing and detached startup for CLI and desktop shell

use crate::client::daemon_client::{DaemonClient, DaemonProbe};
use crate::models::process_info::ProcessInfo;
use crate::models::process_status::ProcessStatus;
use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(windows)]
const DAEMON_EXIT_GRACE: Duration = Duration::from_secs(3);
const DAEMON_PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const DAEMON_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureDaemonOutcome {
    AlreadyRunning,
    Started,
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
struct UpgradeProcess {
    id: uuid::Uuid,
    name: String,
    status: ProcessStatus,
    pid: Option<u32>,
}

#[cfg(windows)]
struct PreservedUpgradeProcess {
    id: uuid::Uuid,
    name: String,
    pid: u32,
    identity: crate::process::instance::ProcessIdentity,
    _process_tree: crate::process::tree::ProcessTreeGuard,
}

fn process_requires_uninstall_hold(process: &ProcessInfo) -> bool {
    status_requires_uninstall_hold(&process.status, process.pid)
}

fn status_requires_uninstall_hold(status: &ProcessStatus, pid: Option<u32>) -> bool {
    pid.is_some()
        || matches!(
            status,
            ProcessStatus::Starting
                | ProcessStatus::Running
                | ProcessStatus::Stopping
                | ProcessStatus::Watching
                | ProcessStatus::Sleeping
        )
}

/// Refuse uninstall whenever a verified managed child is still alive. When the
/// daemon is healthy its live registry is authoritative. When it is offline,
/// use only identity-pinned PIDs from the last durable snapshot; stale or
/// malformed state fails closed instead of stopping an unrelated process.
pub async fn ensure_uninstall_safe(client: &DaemonClient) -> Result<()> {
    let active_names = match client.probe_readiness(DAEMON_PROBE_TIMEOUT).await {
        DaemonProbe::Ready(_) => {
            let response = client.get("/api/v1/processes").await?;
            let raw = response
                .get("processes")
                .ok_or_else(|| anyhow::anyhow!("daemon process list is missing 'processes'"))?;
            let processes: Vec<ProcessInfo> =
                serde_json::from_value(raw.clone()).context("daemon process list is malformed")?;
            processes
                .into_iter()
                .filter(process_requires_uninstall_hold)
                .map(|process| process.name)
                .collect::<Vec<_>>()
        }
        DaemonProbe::Offline => {
            let path = crate::config::paths::state_file();
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect saved process state {}", path.display())
                    });
                }
            };
            if bytes.is_empty() {
                Vec::new()
            } else {
                let saved: crate::daemon::state::SavedState = serde_json::from_slice(&bytes)
                    .with_context(|| {
                        format!("saved process state is invalid: {}", path.display())
                    })?;
                saved.validate()?;
                saved
                    .apps
                    .into_iter()
                    .filter_map(|app| {
                        let pid = app.last_pid?;
                        let alive = app.process_identity.as_ref().map_or_else(
                            || crate::process::identity::is_pid_alive(pid),
                            |identity| {
                                crate::process::identity::process_identity_matches(pid, identity)
                            },
                        );
                        alive.then_some(app.config.name)
                    })
                    .collect::<Vec<_>>()
            }
        }
        DaemonProbe::Occupied { detail } => {
            anyhow::bail!(
                "cannot verify managed projects because 127.0.0.1:2999 is occupied or incompatible: {detail}"
            );
        }
    };

    anyhow::ensure!(
        active_names.is_empty(),
        "uninstall cancelled because managed projects are still active: {}. Stop them explicitly in RunDock, then retry uninstall",
        active_names.join(", ")
    );
    Ok(())
}

/// Prepare an in-place Windows installer upgrade without terminating managed
/// projects created by an older daemon. The new binary opens each verified
/// named Job object, removes kill-on-final-handle-close, then asks the old
/// daemon to save and exit. No unknown listener or unverified PID is touched.
#[cfg(windows)]
pub async fn prepare_upgrade_handoff(client: &DaemonClient) -> Result<()> {
    let health = match client.probe_readiness(DAEMON_PROBE_TIMEOUT).await {
        DaemonProbe::Offline => return Ok(()),
        DaemonProbe::Ready(health) => health,
        DaemonProbe::Occupied { detail } => {
            anyhow::bail!(
                "cannot prepare the RunDock upgrade because 127.0.0.1:2999 is occupied or incompatible: {detail}"
            );
        }
    };
    let daemon_identity = crate::process::identity::capture_process_identity(health.pid)
        .ok_or_else(|| anyhow::anyhow!("verified daemon PID {} disappeared", health.pid))?;
    anyhow::ensure!(
        daemon_identity.start_time_secs != 0,
        "verified daemon PID {} has no stable start time",
        health.pid
    );

    let response = client.get("/api/v1/processes").await?;
    let raw = response
        .get("processes")
        .ok_or_else(|| anyhow::anyhow!("daemon process list is missing 'processes'"))?;
    let processes: Vec<UpgradeProcess> =
        serde_json::from_value(raw.clone()).context("daemon process list is malformed")?;
    let mut preserved = Vec::new();

    for process in processes {
        let Some(pid) = process.pid else {
            anyhow::ensure!(
                !matches!(
                    process.status,
                    ProcessStatus::Starting
                        | ProcessStatus::Running
                        | ProcessStatus::Stopping
                        | ProcessStatus::Watching
                ),
                "cannot safely upgrade while managed process '{}' is {:?} without an owned PID",
                process.name,
                process.status
            );
            continue;
        };
        let identity = crate::process::identity::capture_process_identity(pid).ok_or_else(|| {
            anyhow::anyhow!(
                "cannot safely upgrade because managed process '{}' PID {pid} has no verifiable identity",
                process.name
            )
        })?;
        anyhow::ensure!(
            identity.start_time_secs != 0,
            "cannot safely upgrade because managed process '{}' PID {pid} has no stable start time",
            process.name
        );
        let mut process_tree =
            crate::process::tree::ProcessTreeGuard::new(pid, &process.id.to_string())
                .with_context(|| {
                    format!(
                        "cannot open the owned process tree for '{}' PID {pid}",
                        process.name
                    )
                })?;
        process_tree.preserve_on_drop().with_context(|| {
            format!(
                "cannot preserve the owned process tree for '{}' PID {pid}",
                process.name
            )
        })?;
        preserved.push(PreservedUpgradeProcess {
            id: process.id,
            name: process.name,
            pid,
            identity,
            _process_tree: process_tree,
        });
    }

    client
        .post("/api/v1/system/shutdown", serde_json::json!({}))
        .await
        .context("old daemon rejected the upgrade shutdown request")?;
    let deadline = Instant::now() + DAEMON_STOP_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !remaining.is_zero(),
            "old daemon acknowledged the upgrade handoff but did not release its listener and PID ownership within 15s"
        );
        let pid_owner = crate::utils::pid::read_pid_result()
            .context("failed to verify daemon PID ownership during upgrade handoff")?;
        anyhow::ensure!(
            pid_owner.is_none_or(|pid| pid == health.pid),
            "a different daemon claimed PID-file ownership during upgrade handoff"
        );
        match client
            .probe_readiness(remaining.min(DAEMON_PROBE_TIMEOUT))
            .await
        {
            DaemonProbe::Offline if pid_owner.is_none() => break,
            DaemonProbe::Ready(current) => {
                anyhow::ensure!(
                    current.pid == health.pid,
                    "a different RunDock daemon appeared during upgrade handoff"
                );
            }
            DaemonProbe::Occupied { detail }
                if !crate::process::identity::process_identity_matches(
                    health.pid,
                    &daemon_identity,
                ) =>
            {
                anyhow::bail!(
                    "an unknown listener appeared on 127.0.0.1:2999 during upgrade handoff: {detail}"
                );
            }
            DaemonProbe::Offline | DaemonProbe::Occupied { .. } => {}
        }
        tokio::time::sleep(remaining.min(DAEMON_POLL_INTERVAL)).await;
    }

    let exit_deadline = Instant::now() + DAEMON_EXIT_GRACE;
    while crate::process::identity::process_identity_matches(health.pid, &daemon_identity)
        && Instant::now() < exit_deadline
    {
        tokio::time::sleep(DAEMON_POLL_INTERVAL).await;
    }
    if let Some(current_identity) = crate::process::identity::capture_process_identity(health.pid) {
        anyhow::ensure!(
            crate::process::identity::stable_identity_matches(&current_identity, &daemon_identity,),
            "refusing to stop recycled PID {} after upgrade handoff",
            health.pid
        );
        crate::process::identity::kill_process_verified(health.pid, Some(&daemon_identity))
            .await
            .context(
                "failed to stop the verified old daemon after it released service ownership",
            )?;
    }
    anyhow::ensure!(
        crate::process::identity::capture_process_identity(health.pid).is_none(),
        "verified old daemon PID {} is still alive after upgrade handoff cleanup",
        health.pid
    );

    for process in &preserved {
        anyhow::ensure!(
            crate::process::identity::process_identity_matches(process.pid, &process.identity),
            "managed process '{}' ({}, PID {}) did not survive the upgrade handoff",
            process.name,
            process.id,
            process.pid
        );
    }
    Ok(())
}

pub async fn ensure_daemon(
    daemon_exe: &Path,
    host: &str,
    port: u16,
) -> Result<EnsureDaemonOutcome> {
    crate::daemon::server::loopback_socket_addr(host, port)?;
    let probe_client = DaemonClient::new(host, port)?;
    match probe_client.probe_readiness(DAEMON_PROBE_TIMEOUT).await {
        DaemonProbe::Ready(_) => return Ok(EnsureDaemonOutcome::AlreadyRunning),
        DaemonProbe::Offline => {}
        DaemonProbe::Occupied { detail } => {
            anyhow::bail!(
                "refusing to start RunDock because {host}:{port} is occupied or incompatible: {detail}"
            );
        }
    }
    anyhow::ensure!(
        daemon_exe.is_file(),
        "RunDock daemon executable was not found at {}",
        daemon_exe.display()
    );

    #[cfg(target_os = "windows")]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;

        std::process::Command::new(daemon_exe)
            .arg("--internal-daemon")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .with_context(|| format!("failed to start {}", daemon_exe.display()))?
    };

    #[cfg(not(target_os = "windows"))]
    let mut child = std::process::Command::new(daemon_exe)
        .arg("--internal-daemon")
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {}", daemon_exe.display()))?;

    let pid = child.id();
    let deadline = Instant::now() + DAEMON_START_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match probe_client
            .probe_readiness(remaining.min(DAEMON_PROBE_TIMEOUT))
            .await
        {
            DaemonProbe::Ready(_) => return Ok(EnsureDaemonOutcome::Started),
            DaemonProbe::Offline => {}
            DaemonProbe::Occupied { detail } => {
                let spawned_process_owns_daemon = crate::utils::pid::read_pid() == Some(pid)
                    && crate::utils::pid::is_daemon_running();
                if spawned_process_owns_daemon {
                    tokio::time::sleep(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(DAEMON_POLL_INTERVAL),
                    )
                    .await;
                    continue;
                }
                crate::daemon::terminate_failed_replacement(child)
                    .await
                    .context(
                        "failed to clean up the daemon after an incompatible listener appeared",
                    )?;
                anyhow::bail!(
                    "RunDock startup was blocked because {host}:{port} became occupied or incompatible: {detail}"
                );
            }
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!(
                "RunDock daemon exited before readiness (pid={pid}, status={status}). Check: {}",
                crate::config::paths::daemon_log_file().display()
            );
        }
        tokio::time::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(DAEMON_POLL_INTERVAL),
        )
        .await;
    }

    let status = child.try_wait()?;
    crate::daemon::terminate_failed_replacement(child)
        .await
        .context("daemon startup timed out and the spawned process could not be terminated")?;
    anyhow::ensure!(
        crate::process::identity::capture_process_identity(pid).is_none(),
        "daemon startup timed out and PID {pid} is still alive after cleanup"
    );
    anyhow::bail!(
        "RunDock daemon did not become healthy within 10s (pid={pid}, status={status:?}). Check: {}",
        crate::config::paths::daemon_log_file().display()
    );
}

#[cfg(test)]
mod tests {
    use super::status_requires_uninstall_hold;
    use crate::models::process_status::ProcessStatus;

    #[test]
    fn uninstall_holds_only_active_or_pid_owned_processes() {
        for status in [
            ProcessStatus::Starting,
            ProcessStatus::Running,
            ProcessStatus::Stopping,
            ProcessStatus::Watching,
            ProcessStatus::Sleeping,
        ] {
            assert!(status_requires_uninstall_hold(&status, None));
        }
        assert!(status_requires_uninstall_hold(
            &ProcessStatus::Errored,
            Some(42)
        ));
        assert!(!status_requires_uninstall_hold(
            &ProcessStatus::Stopped,
            None
        ));
        assert!(!status_requires_uninstall_hold(
            &ProcessStatus::Crashed,
            None
        ));
    }
}
