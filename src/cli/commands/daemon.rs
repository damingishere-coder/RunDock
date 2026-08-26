// @group BusinessLogic : `alter daemon` command handler — start/stop/status daemon

use crate::cli::args::DaemonAction;
use crate::client::daemon_client::DaemonClient;
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(15);
const DAEMON_RESTART_TIMEOUT: Duration = Duration::from_secs(45);
const DAEMON_PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const DAEMON_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run(client: &DaemonClient, action: DaemonAction, host: &str, port: u16) -> Result<()> {
    match action {
        DaemonAction::Start { port: p } => start_daemon(host, p).await,
        DaemonAction::Stop => stop_daemon(client).await,
        DaemonAction::Restart => restart_daemon(client, host, port).await,
        DaemonAction::Status => status(client).await,
        DaemonAction::Logs => show_logs(),
    }
}

// @group BusinessLogic > Daemon : Spawn daemon as detached background process and wait for it to bind
async fn start_daemon(host: &str, port: u16) -> Result<()> {
    crate::daemon::server::loopback_socket_addr(host, port)?;
    let probe_client = DaemonClient::new(host, port)?;
    // Check if a real daemon is running — TCP connect + HTTP health check.
    // A zombie socket will accept TCP but won't respond to HTTP, so we treat that as dead.
    if probe_client.is_alive().await {
        println!("[alter] daemon is already running on {host}:{port}");
        return Ok(());
    }

    let exe = std::env::current_exe()?;

    #[cfg(target_os = "windows")]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;

        std::process::Command::new(&exe)
            .arg("--internal-daemon")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()?
    };

    #[cfg(not(target_os = "windows"))]
    let mut child = {
        std::process::Command::new(&exe)
            .arg("--internal-daemon")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?
    };

    if wait_until_alive(&probe_client, DAEMON_START_TIMEOUT).await {
        println!("[alter] daemon started  →  http://{host}:{port}");
        return Ok(());
    }

    let pid = child.id();
    let status = child.try_wait()?;
    eprintln!(
        "[alter] daemon did not become healthy within 10s (pid={pid}, status={status:?}). Check: {}",
        crate::config::paths::daemon_log_file().display()
    );
    crate::daemon::terminate_failed_replacement(child)
        .await
        .context("daemon startup timed out and the spawned process could not be terminated")?;
    anyhow::ensure!(
        crate::process::identity::capture_process_identity(pid).is_none(),
        "daemon startup timed out and PID {pid} is still alive after cleanup"
    );
    anyhow::bail!("daemon startup health check timed out; the spawned process was terminated");
}

async fn stop_daemon(client: &DaemonClient) -> Result<()> {
    if !client.is_alive().await {
        println!("[alter] daemon is not running");
        return Ok(());
    }
    let old_pid = crate::utils::pid::read_pid_result()?;
    client
        .post("/api/v1/system/shutdown", serde_json::json!({}))
        .await?;
    if wait_until_stopped(client, old_pid, DAEMON_STOP_TIMEOUT).await {
        println!("[alter] daemon stopped");
        return Ok(());
    }
    anyhow::bail!("daemon acknowledged shutdown but remained healthy after 15s");
}

// @group BusinessLogic > Daemon : Stop daemon then start it again; managed processes survive
// because runner.rs uses CREATE_BREAKAWAY_FROM_JOB on Windows.
async fn restart_daemon(client: &DaemonClient, host: &str, port: u16) -> Result<()> {
    if client.is_alive().await {
        let old_pid = crate::utils::pid::read_pid_result()?;
        client
            .post("/api/v1/system/restart", serde_json::json!({}))
            .await?;
        if !wait_until_restarted(client, old_pid, DAEMON_RESTART_TIMEOUT).await? {
            anyhow::bail!(
                "daemon restart did not produce a healthy replacement with new PID ownership after 45s"
            );
        }
        println!("[alter] daemon restarted");
        return Ok(());
    }
    start_daemon(host, port).await?;
    println!("[alter] daemon restarted");
    Ok(())
}

async fn wait_until_restarted(
    client: &DaemonClient,
    old_pid: Option<u32>,
    timeout: Duration,
) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let new_pid = crate::utils::pid::read_pid_result()
            .context("failed to read daemon PID ownership while waiting for restart")?;
        let ownership_changed = new_pid.is_some() && new_pid != old_pid;
        if ownership_changed
            && client
                .is_alive_with_timeout(remaining.min(DAEMON_PROBE_TIMEOUT))
                .await
        {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        tokio::time::sleep(remaining.min(DAEMON_POLL_INTERVAL)).await;
    }
}

async fn wait_until_alive(client: &DaemonClient, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        if client
            .is_alive_with_timeout(remaining.min(DAEMON_PROBE_TIMEOUT))
            .await
        {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(remaining.min(DAEMON_POLL_INTERVAL)).await;
    }
}

async fn wait_until_stopped(
    client: &DaemonClient,
    old_pid: Option<u32>,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let alive = client
            .is_alive_with_timeout(remaining.min(DAEMON_PROBE_TIMEOUT))
            .await;
        let pid_file_released = matches!(crate::utils::pid::read_pid_result(), Ok(None));
        let old_process_exited = old_pid
            .is_none_or(|pid| crate::process::identity::capture_process_identity(pid).is_none());
        let pid_released = pid_file_released && old_process_exited;
        if !alive && pid_released {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(remaining.min(DAEMON_POLL_INTERVAL)).await;
    }
}

async fn status(client: &DaemonClient) -> Result<()> {
    if client.is_alive().await {
        let health = client.get("/api/v1/system/health").await?;
        println!("[alter] daemon is running");
        println!(
            "  version:    {}",
            health["version"].as_str().unwrap_or("?")
        );
        println!(
            "  uptime:     {}s",
            health["uptime_secs"].as_u64().unwrap_or(0)
        );
        println!(
            "  processes:  {}",
            health["process_count"].as_u64().unwrap_or(0)
        );
    } else {
        println!("[alter] daemon is NOT running");
    }
    Ok(())
}

fn show_logs() -> Result<()> {
    let path = crate::config::paths::daemon_log_file();
    if !path.exists() {
        println!("[alter] no daemon log file found at {}", path.display());
        return Ok(());
    }
    let lines = crate::logging::reader::read_last_lines(&path, 100)?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
