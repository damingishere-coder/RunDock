// @group BusinessLogic : Managed process instance — holds full lifecycle state

use crate::config::ecosystem::AppConfig;
use crate::logging::writer::LogWriter;
use crate::models::cron_run::CronRun;
use crate::models::log_stats::LogStatsState;
use crate::models::process_info::{HealthCheckStatus, ProcessInfo};
use crate::models::process_status::ProcessStatus;
use crate::process::tree::ProcessTreeGuard;
use crate::process::watcher::FileWatcher;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

/// A single log line emitted by a child process
#[derive(Debug, Clone)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub process_id: Uuid,
    pub stream: LogStream,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Stable-enough OS identity used to reject a PID that has been recycled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub executable: Option<String>,
    pub command_line: Vec<String>,
    pub cwd: Option<String>,
    pub start_time_secs: u64,
}

/// Live in-memory state for a managed process
pub struct ManagedProcess {
    pub id: Uuid,
    pub config: AppConfig,
    pub status: ProcessStatus,
    pub pid: Option<u32>,
    pub process_identity: Option<ProcessIdentity>,
    /// Re-opened process-tree ownership for a process adopted after daemon handoff.
    pub process_tree: Option<ProcessTreeGuard>,
    pub restart_count: u32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub last_exit_code: Option<i32>,
    /// Broadcast channel: all subscribers receive new log lines in real-time
    pub log_tx: broadcast::Sender<LogLine>,
    /// Rolling file writer for this process
    pub log_writer: Option<LogWriter>,
    /// Next scheduled run time for cron processes
    pub cron_next_run: Option<DateTime<Utc>>,
    /// History of the last MAX_CRON_HISTORY cron runs (timestamp, exit code, duration)
    pub cron_run_history: Vec<CronRun>,
    /// Last measured CPU usage percentage — updated by the metrics loop
    pub cpu_percent: Option<f32>,
    /// Last measured resident memory in bytes — updated by the metrics loop
    pub memory_bytes: Option<u64>,
    /// Current health probe result — None if no health check is configured
    pub health_status: Option<HealthCheckStatus>,
    /// Handle to the running health check task — aborted on process stop
    pub health_check_handle: Option<tokio::task::JoinHandle<()>>,
    /// Keep the native watcher alive for as long as this process is managed.
    pub file_watcher: Option<FileWatcher>,
    /// Desired lifecycle state set by explicit user actions. Restart/watch
    /// events must not respawn a process after a manual stop.
    pub desired_running: bool,
    /// Monotonic spawn generation used to discard stale exit/restart events.
    pub generation: u64,
    /// Cached git branch from the process cwd — populated at creation time
    pub git_branch: Option<String>,
    // @group BusinessLogic > LogStats : Rolling 5-minute log volume buckets (stdout + stderr counts)
    pub log_stats: Arc<Mutex<LogStatsState>>,
}

impl ManagedProcess {
    pub fn new(config: AppConfig) -> Self {
        Self::new_with_id(Uuid::new_v4(), config)
    }

    /// Restore a process with its persisted UUID so IDs remain stable across daemon restarts.
    pub fn new_with_id(id: Uuid, config: AppConfig) -> Self {
        let (log_tx, _) = broadcast::channel(1024);
        Self {
            id,
            config,
            status: ProcessStatus::Stopped,
            pid: None,
            process_identity: None,
            process_tree: None,
            restart_count: 0,
            created_at: Utc::now(),
            started_at: None,
            stopped_at: None,
            last_exit_code: None,
            log_tx,
            log_writer: None,
            cron_next_run: None,
            cron_run_history: vec![],
            cpu_percent: None,
            memory_bytes: None,
            health_status: None,
            health_check_handle: None,
            file_watcher: None,
            desired_running: false,
            generation: 0,
            log_stats: Arc::new(Mutex::new(LogStatsState::new())),
            git_branch: None,
        }
    }

    pub async fn refresh_git_branch(&mut self) {
        self.git_branch = match self.config.cwd.as_deref() {
            Some(cwd) => read_git_branch(cwd).await,
            None => None,
        };
    }

    pub fn uptime_secs(&self) -> Option<u64> {
        self.started_at.map(|t| {
            let stopped = self.stopped_at.unwrap_or_else(Utc::now);
            (stopped - t).num_seconds().max(0) as u64
        })
    }

    pub fn to_info(&self) -> ProcessInfo {
        ProcessInfo {
            id: self.id,
            name: self.config.name.clone(),
            project_id: self.config.project_id,
            script: self.config.script.clone(),
            args: self.config.args.clone(),
            cwd: self.config.cwd.clone(),
            status: self.status.clone(),
            pid: self.pid,
            restart_count: self.restart_count,
            uptime_secs: self.uptime_secs(),
            last_exit_code: self.last_exit_code,
            autorestart: self.config.autorestart,
            max_restarts: self.config.max_restarts,
            watch: self.config.watch,
            namespace: self.config.namespace.clone(),
            created_at: self.created_at,
            started_at: self.started_at,
            stopped_at: self.stopped_at,
            cron: self.config.cron.clone(),
            cron_next_run: self.cron_next_run,
            cron_run_history: self.cron_run_history.clone(),
            cpu_percent: self.cpu_percent,
            memory_bytes: self.memory_bytes,
            env: self.config.env.clone(),
            notify: self.config.notify.clone(),
            log_alert: self.config.log_alert.clone(),
            health_status: self.health_status.clone(),
            git_branch: self.git_branch.clone(),
            enabled: self.config.enabled,
        }
    }
}

// @group Utilities > Git : Read the active git branch from a directory path
pub(crate) async fn read_git_branch(cwd: &str) -> Option<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), cmd.output())
        .await
        .ok()
        .and_then(Result::ok)
        .filter(|o| o.status.success())
        .filter(|o| o.stdout.len() <= 4_096)
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
}
