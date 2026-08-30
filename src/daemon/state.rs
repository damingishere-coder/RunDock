// @group DatabaseOperations : Daemon shared state — process registry with disk persistence

use crate::config::auth_config::AuthConfig;
use crate::config::daemon_config::DaemonConfig;
use crate::config::ecosystem::AppConfig;
use crate::config::notification_store::NotificationsStore;
use crate::config::project_store::ProjectStore;
use crate::config::telegram_config::TelegramConfig;
use crate::models::cron_run::CronRun;
use crate::models::tunnel::TunnelSettings;
use crate::process::instance::ProcessIdentity;
use crate::process::manager::{ManagedProcessSnapshot, ProcessManager};
use crate::terminal::TerminalManager;
use crate::tunnel::TunnelManager;
use anyhow::{Context, Result};
use axum::http::Method;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{broadcast, Mutex, RwLock, Semaphore};
use uuid::Uuid;

/// Persistent snapshot of process configs (saved to disk)
const SAVED_STATE_SCHEMA_VERSION: u32 = 1;
const MAX_SAVED_APPS: usize = 1_000;
const MAX_SAVED_STATE_PAYLOAD_BYTES: usize = 14 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default = "default_saved_state_schema_version")]
    pub schema_version: u32,
    pub saved_at: Option<DateTime<Utc>>,
    pub apps: Vec<SavedApp>,
}

fn default_saved_state_schema_version() -> u32 {
    SAVED_STATE_SCHEMA_VERSION
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            schema_version: SAVED_STATE_SCHEMA_VERSION,
            saved_at: None,
            apps: Vec::new(),
        }
    }
}

impl SavedState {
    pub(crate) fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == SAVED_STATE_SCHEMA_VERSION,
            "unsupported state schema version: {}",
            self.schema_version
        );
        anyhow::ensure!(
            self.apps.len() <= MAX_SAVED_APPS,
            "saved state contains too many apps"
        );
        let mut ids = HashSet::with_capacity(self.apps.len());
        let mut payload_bytes = 0usize;
        for app in &self.apps {
            anyhow::ensure!(ids.insert(app.id), "saved state contains duplicate app IDs");
            app.config.validate()?;
            anyhow::ensure!(
                app.cron_run_history.len() <= crate::models::cron_run::MAX_CRON_HISTORY,
                "saved cron history exceeds its retention limit"
            );
            anyhow::ensure!(
                app.last_pid.is_none_or(|pid| pid > 0),
                "saved process PID must be positive"
            );
            if let Some(identity) = &app.process_identity {
                anyhow::ensure!(
                    identity.command_line.len() <= 256
                        && identity.command_line.iter().all(|part| part.len() <= 4_096)
                        && identity
                            .executable
                            .as_deref()
                            .is_none_or(|path| path.len() <= 4_096)
                        && identity
                            .cwd
                            .as_deref()
                            .is_none_or(|path| path.len() <= 4_096),
                    "saved process identity exceeds its supported bounds"
                );
            }
            // Validate and serialize one bounded app at a time before the full
            // document is built, preventing a valid-per-app configuration set
            // from forcing an unbounded whole-state allocation.
            payload_bytes = payload_bytes.saturating_add(serde_json::to_vec(app)?.len());
            anyhow::ensure!(
                payload_bytes <= MAX_SAVED_STATE_PAYLOAD_BYTES,
                "saved state exceeds its aggregate payload limit"
            );
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedApp {
    pub id: Uuid,
    pub config: AppConfig,
    pub restart_count: u32,
    #[serde(default)]
    pub cron_run_history: Vec<CronRun>,
    /// PID of the process at the time state was last saved.
    /// Used on restore to detect and clean up orphaned OS processes.
    #[serde(default)]
    pub last_pid: Option<u32>,
    /// OS identity captured together with last_pid. Legacy snapshots omit it
    /// and are never allowed to terminate or adopt that PID automatically.
    #[serde(default)]
    pub process_identity: Option<ProcessIdentity>,
    /// For cron jobs: true if the scheduler was active (Sleeping) at save time.
    /// false means the user had manually stopped it — do NOT re-arm the scheduler on restore.
    /// Defaults to true for backward compatibility with old state files.
    #[serde(default = "default_true")]
    pub cron_was_active: bool,
}

fn default_true() -> bool {
    true
}

/// Live daemon state — shared across all Axum handlers
pub struct DaemonState {
    pub manager: ProcessManager,
    pub config: DaemonConfig,
    pub started_at: DateTime<Utc>,
    pub notifications: Arc<RwLock<NotificationsStore>>,
    /// User-facing logical project metadata, persisted independently from
    /// process runtime state so editing it never restarts a process.
    pub projects: Arc<RwLock<ProjectStore>>,
    /// Ephemeral GitHub Device Flow auth state — cleared after successful login or expiry
    pub ai_device_auth:
        Arc<tokio::sync::Mutex<HashMap<String, crate::models::ai::DeviceAuthState>>>,

    // @group Authentication : Session and auth state
    /// Active browser sessions: token → expiry timestamp
    pub sessions: Arc<DashMap<String, DateTime<Utc>>>,
    /// Makes session cleanup, quota admission and insertion one atomic operation.
    pub(crate) session_lock: Arc<Mutex<()>>,
    /// One-time, short-lived credentials for SSE and WebSocket handshakes.
    pub stream_tickets: Arc<DashMap<String, StreamTicket>>,
    /// Makes stream-ticket cleanup, admission and insertion one atomic quota operation.
    pub(crate) stream_ticket_lock: Arc<Mutex<()>>,
    /// Invalidates login work that began before a password/PIN mutation.
    pub(crate) auth_generation: AtomicU64,
    /// Per-peer login-attempt windows. Direct socket peers are used instead of
    /// forwarded headers so one client cannot exhaust every user's allowance.
    pub login_attempts: Arc<Mutex<HashMap<std::net::IpAddr, VecDeque<DateTime<Utc>>>>>,
    /// Auth config (password hash, master token, stored passkeys) — guarded for write access
    pub auth: Arc<RwLock<AuthConfig>>,

    // @group Configuration : Telegram bot config — guarded for hot reload
    pub telegram: Arc<RwLock<TelegramConfig>>,

    // @group BusinessLogic : Tunnel manager — tracks active cloudflared/ngrok/custom subprocesses
    pub tunnel_manager: TunnelManager,
    // @group Configuration : Tunnel provider settings — guarded for hot reload
    pub tunnel_settings: Arc<RwLock<TunnelSettings>>,

    // @group BusinessLogic : Terminal manager — tracks active PTY/WebSocket terminal sessions
    pub terminal_manager: TerminalManager,
    /// Serialises snapshots and commits so a late detached save can never
    /// overwrite a newer snapshot with stale content.
    state_save_lock: Arc<Mutex<()>>,
    project_save_lock: Arc<Mutex<()>>,
    persistence_write_lock: Arc<Mutex<()>>,
    /// Serialises user-visible mutations with their persistence/rollback step.
    /// The save locks protect files; this lock protects the whole transaction.
    pub(crate) state_mutation_lock: Arc<Mutex<()>>,
    /// Prevents two installer executions from racing over the same executable.
    pub(crate) update_lock: Arc<Mutex<()>>,
    /// Serialises and briefly caches GitHub release checks to prevent request amplification.
    pub(crate) update_check_cache: Arc<Mutex<Option<(std::time::Instant, serde_json::Value)>>>,
    /// Prevents concurrent package-manager processes for tunnel providers.
    pub(crate) tunnel_install_lock: Arc<Mutex<()>>,
    /// Serialises last-writer-wins settings mutations across API surfaces.
    pub(crate) config_mutation_lock: Arc<Mutex<()>>,
    /// Caps billable/provider-backed AI streams and provides fail-fast load shedding.
    pub(crate) ai_stream_limit: Arc<Semaphore>,
    /// Caps GitHub Device Flow network work before an active flow exists.
    pub(crate) ai_auth_limit: Arc<Semaphore>,
    /// Caps expensive Argon2 verification before work enters Tokio's blocking pool.
    pub(crate) auth_verify_limit: Arc<Semaphore>,
    /// Caps provider executable probes and their owned process trees.
    pub(crate) tunnel_probe_limit: Arc<Semaphore>,
    /// Caps executable script streams so one local page cannot exhaust the host.
    pub(crate) script_run_limit: Arc<Semaphore>,
    /// Caps concurrent blocking filesystem scans exposed through API routes.
    pub(crate) blocking_io_limit: Arc<Semaphore>,
    /// Serialises script replacement/deletion and keeps a launched script's
    /// source stable until its process tree exits.
    pub(crate) script_mutation_lock: Arc<Mutex<()>>,
    /// Last exhausted background persistence error, surfaced by /system/health.
    pub(crate) background_persistence_error: Arc<RwLock<Option<String>>>,
    /// Prevents two pull/install/restart transactions from racing in one checkout.
    pub(crate) git_operation_lock: Arc<Mutex<()>>,
    restart_attempt: std::sync::Mutex<Option<RestartAttempt>>,
    restart_shutdown_requested: AtomicBool,
    shutdown_requested: AtomicBool,
    restart_handoff_committed: AtomicBool,
    shutdown_coordination_lock: std::sync::Mutex<()>,
    shutdown_tx: broadcast::Sender<()>,
}

impl DaemonState {
    pub fn new(config: DaemonConfig) -> Result<Self> {
        if crate::config::state_transaction::recover_pending()? {
            tracing::warn!("completed an interrupted state/projects persistence transaction");
        }
        Self::from_parts(
            config,
            crate::config::notification_store::load()?,
            crate::config::project_store::load()?,
            crate::config::auth_config::load(),
            crate::config::telegram_config::load()?,
            crate::config::tunnel_config::load()?,
        )
    }

    /// Construct a daemon state without reading or writing global application
    /// data. Integration tests should use this together with ALTER_DATA_DIR.
    #[doc(hidden)]
    pub fn new_isolated(config: DaemonConfig) -> Self {
        Self::from_parts(
            config,
            NotificationsStore::default(),
            ProjectStore::default(),
            AuthConfig::new_unconfigured(),
            TelegramConfig::default(),
            TunnelSettings::default(),
        )
        .expect("in-memory daemon state construction cannot fail")
    }

    fn from_parts(
        config: DaemonConfig,
        notification_store: NotificationsStore,
        project_store: ProjectStore,
        auth_cfg: AuthConfig,
        telegram_cfg: TelegramConfig,
        tunnel_cfg: TunnelSettings,
    ) -> Result<Self> {
        let notifications = Arc::new(RwLock::new(notification_store));
        let projects = Arc::new(RwLock::new(project_store));
        let (shutdown_tx, _) = broadcast::channel(4);
        Ok(Self {
            manager: ProcessManager::new(Arc::clone(&notifications)),
            config,
            started_at: Utc::now(),
            notifications,
            projects,
            ai_device_auth: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            sessions: Arc::new(DashMap::new()),
            session_lock: Arc::new(Mutex::new(())),
            stream_tickets: Arc::new(DashMap::new()),
            stream_ticket_lock: Arc::new(Mutex::new(())),
            auth_generation: AtomicU64::new(0),
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
            auth: Arc::new(RwLock::new(auth_cfg)),
            telegram: Arc::new(RwLock::new(telegram_cfg)),
            tunnel_manager: TunnelManager::new(),
            tunnel_settings: Arc::new(RwLock::new(tunnel_cfg)),
            terminal_manager: TerminalManager::new(),
            state_save_lock: Arc::new(Mutex::new(())),
            project_save_lock: Arc::new(Mutex::new(())),
            persistence_write_lock: Arc::new(Mutex::new(())),
            state_mutation_lock: Arc::new(Mutex::new(())),
            update_lock: Arc::new(Mutex::new(())),
            update_check_cache: Arc::new(Mutex::new(None)),
            tunnel_install_lock: Arc::new(Mutex::new(())),
            config_mutation_lock: Arc::new(Mutex::new(())),
            ai_stream_limit: Arc::new(Semaphore::new(4)),
            ai_auth_limit: Arc::new(Semaphore::new(8)),
            auth_verify_limit: Arc::new(Semaphore::new(8)),
            tunnel_probe_limit: Arc::new(Semaphore::new(4)),
            script_run_limit: Arc::new(Semaphore::new(2)),
            blocking_io_limit: Arc::new(Semaphore::new(8)),
            script_mutation_lock: Arc::new(Mutex::new(())),
            background_persistence_error: Arc::new(RwLock::new(None)),
            git_operation_lock: Arc::new(Mutex::new(())),
            restart_attempt: std::sync::Mutex::new(None),
            restart_shutdown_requested: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            restart_handoff_committed: AtomicBool::new(false),
            shutdown_coordination_lock: std::sync::Mutex::new(()),
            shutdown_tx,
        })
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub fn request_shutdown(&self) {
        let _coordination_guard = self.shutdown_coordination_lock.lock().ok();
        self.shutdown_requested.store(true, Ordering::Release);
        if self.restart_handoff_committed.load(Ordering::Acquire) {
            tracing::info!(
                "external shutdown arrived after restart ownership was committed to the replacement"
            );
        }
        let _ = self.shutdown_tx.send(());
    }

    pub(crate) fn request_restart_shutdown(&self) {
        self.restart_shutdown_requested
            .store(true, Ordering::Release);
        let _ = self.shutdown_tx.send(());
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
            || self.restart_shutdown_requested.load(Ordering::Acquire)
    }

    pub(crate) fn external_shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }

    /// Linearization point for the old daemon handing service ownership to a
    /// healthy replacement. External shutdown wins if it was recorded first.
    pub(crate) fn commit_restart_handoff(&self) -> bool {
        let Ok(_coordination_guard) = self.shutdown_coordination_lock.lock() else {
            return false;
        };
        if self.shutdown_requested.load(Ordering::Acquire) {
            return false;
        }
        self.restart_handoff_committed
            .store(true, Ordering::Release);
        true
    }

    pub(crate) fn arm_restart(&self, mut attempt: RestartAttempt) -> Result<()> {
        self.restart_handoff_committed
            .store(false, Ordering::Release);
        let Ok(mut slot) = self.restart_attempt.lock() else {
            let _ = attempt.child.kill();
            let _ = attempt.child.wait();
            anyhow::bail!("restart coordination lock is poisoned");
        };
        if slot.is_some() {
            let _ = attempt.child.kill();
            let _ = attempt.child.wait();
            anyhow::bail!("another daemon restart is already pending");
        }
        *slot = Some(attempt);
        Ok(())
    }

    pub(crate) fn take_restart_attempt(&self) -> Option<RestartAttempt> {
        self.restart_attempt.lock().ok()?.take()
    }

    pub(crate) fn resume_after_failed_restart(&self) -> bool {
        self.restart_shutdown_requested
            .store(false, Ordering::Release);
        self.restart_handoff_committed
            .store(false, Ordering::Release);
        !self.shutdown_requested.load(Ordering::Acquire)
    }

    pub async fn save_projects(&self) -> Result<()> {
        self.save_projects_inner(false).await
    }

    /// Persist a compensated project transaction and make its recovery copy
    /// match the restored primary instead of the failed intermediate commit.
    pub async fn save_projects_rollback(&self) -> Result<()> {
        self.save_projects_inner(true).await
    }

    async fn save_projects_inner(&self, refresh_recovery_copy: bool) -> Result<()> {
        self.save_state_and_projects_inner(refresh_recovery_copy)
            .await
    }

    /// Persist runtime state and logical projects as one crash-recoverable unit.
    pub async fn save_state_and_projects(&self) -> Result<()> {
        self.save_state_and_projects_inner(false).await
    }

    /// Persist an in-memory compensation and align both recovery copies with it.
    pub async fn save_state_and_projects_rollback(&self) -> Result<()> {
        self.save_state_and_projects_inner(true).await
    }

    async fn save_state_and_projects_inner(&self, refresh_recovery_copy: bool) -> Result<()> {
        let _write_guard = self.persistence_write_lock.lock().await;
        let _state_guard = self.state_save_lock.lock().await;
        let _project_guard = self.project_save_lock.lock().await;
        tokio::task::spawn_blocking(crate::config::state_transaction::recover_pending)
            .await?
            .context("failed to recover pending state transaction before persistence save")?;
        let saved = self.saved_state_snapshot().await?;
        let projects = self.projects.read().await.clone();
        projects.validate()?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            crate::config::state_transaction::commit(&saved, &projects)?;
            if refresh_recovery_copy {
                crate::config::atomic_file::refresh_backup_from_primary_validated::<SavedState, _>(
                    &crate::config::paths::state_file(),
                    SavedState::validate,
                )?;
                crate::config::atomic_file::refresh_backup_from_primary_validated::<
                    crate::config::project_store::ProjectStore,
                    _,
                >(
                    &crate::config::paths::projects_file(),
                    crate::config::project_store::ProjectStore::validate,
                )?;
            }
            Ok(())
        })
        .await??;
        Ok(())
    }

    // @group DatabaseOperations : Serialize current process list to JSON file
    pub async fn save_to_disk(&self) -> Result<()> {
        self.save_to_disk_inner(false).await
    }

    /// Persist compensated runtime state and synchronize the recovery copy
    /// while the save lock is still held.
    pub async fn save_state_rollback(&self) -> Result<()> {
        self.save_to_disk_inner(true).await
    }

    async fn save_to_disk_inner(&self, refresh_recovery_copy: bool) -> Result<()> {
        self.save_state_and_projects_inner(refresh_recovery_copy)
            .await
    }

    async fn saved_state_snapshot(&self) -> Result<SavedState> {
        let apps = self
            .manager
            .snapshot()
            .await
            .into_iter()
            .map(saved_app_from_snapshot)
            .collect();

        let saved = SavedState {
            schema_version: SAVED_STATE_SCHEMA_VERSION,
            saved_at: Some(Utc::now()),
            apps,
        };
        saved.validate()?;
        Ok(saved)
    }

    /// Persist lifecycle changes that originate in manager background tasks
    /// (automatic restarts/exits), which have no API request available to commit them.
    pub fn start_background_persistence(self: &Arc<Self>) {
        let mut receiver = self.manager.subscribe_persistence();
        let state = Arc::clone(self);
        tokio::spawn(async move {
            const MAX_ATTEMPTS: u32 = 8;
            let mut shutdown = state.subscribe_shutdown();
            loop {
                let event = tokio::select! {
                    event = receiver.recv() => event,
                    _ = shutdown.recv() => return,
                };
                match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
                while receiver.try_recv().is_ok() {}
                let mut attempt = 0u32;
                loop {
                    attempt = attempt.saturating_add(1);
                    let result = {
                        let _mutation_guard = state.state_mutation_lock.lock().await;
                        state.save_to_disk().await
                    };
                    match result {
                        Ok(()) => {
                            *state.background_persistence_error.write().await = None;
                            break;
                        }
                        Err(error) => {
                            if attempt < MAX_ATTEMPTS {
                                tracing::warn!(%error, attempt, "background state persistence failed; retrying");
                            } else {
                                let message = format!(
                                    "background persistence failed after {attempt} attempts: {error}"
                                );
                                tracing::error!(%error, attempt, "background state persistence circuit opened");
                                *state.background_persistence_error.write().await = Some(message);
                                break;
                            }
                            let exponent = attempt.saturating_sub(1).min(6);
                            let delay_ms = (100u64 * (1u64 << exponent)).min(5_000);
                            tokio::select! {
                                _ = tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)) => {}
                                _ = shutdown.recv() => return,
                            }
                        }
                    }
                }
            }
        });
    }

    // @group DatabaseOperations : Load persisted state from disk
    pub async fn load_from_disk(&self) -> Result<SavedState> {
        let _write_guard = self.persistence_write_lock.lock().await;
        let path = crate::config::paths::state_file();
        tokio::task::spawn_blocking(move || {
            crate::config::state_transaction::recover_pending()
                .context("failed to recover pending state transaction before state load")?;
            load_saved_state(&path)
        })
        .await?
    }

    // @group DatabaseOperations : Restore previously saved processes on daemon startup.
    //
    // Strategy (PID-first):
    //   • Cron jobs     → always restore as Sleeping (kill any stale PID first to avoid duplicates)
    //   • last_pid alive  → re-adopt the running process; a watcher fires autorestart when it exits
    //   • last_pid dead   → mark Stopped; user decides when to restart
    //   • no last_pid     → mark Stopped (daemon crashed before the process was ever saved with a PID)
    //
    // This prevents both duplicate spawns and silent orphan accumulation.
    pub async fn restore(&self, saved: SavedState) {
        use crate::process::identity::{
            capture_process_identity, kill_orphan_pid, process_identity_matches,
            stable_identity_matches,
        };

        for app in saved.apps {
            let live_identity = app.last_pid.and_then(capture_process_identity);
            let verified_identity = match (&app.process_identity, &live_identity) {
                (Some(expected), Some(current)) if stable_identity_matches(current, expected) => {
                    Some(current.clone())
                }
                (None, Some(_)) => {
                    tracing::error!(
                        "refusing automatic PID recovery for '{}' because the saved state has no process identity",
                        app.config.name
                    );
                    None
                }
                (Some(_), Some(_)) => {
                    tracing::error!(
                        "refusing automatic PID recovery for '{}' because PID {} now belongs to a different process",
                        app.config.name,
                        app.last_pid.unwrap_or_default()
                    );
                    None
                }
                _ => None,
            };

            // Disabled entries must never come back merely because their old
            // PID survived the daemon restart. Keep the real running state if
            // the verified kill fails so the UI can report and retry it.
            if !app.config.enabled {
                if let Some(pid) = app.last_pid {
                    if let Some(identity) = verified_identity.as_ref() {
                        match kill_orphan_pid(pid, identity).await {
                            Ok(()) => tracing::info!(
                                "stopped disabled process '{}' (PID {}) during restore",
                                app.config.name,
                                pid
                            ),
                            Err(error) => {
                                tracing::warn!(
                                    "failed to stop disabled process '{}' (PID {}) during restore: {error}",
                                    app.config.name,
                                    pid
                                );
                                if process_identity_matches(pid, identity) {
                                    self.manager
                                        .register_running_adopted(
                                            app.id,
                                            app.config,
                                            pid,
                                            identity.clone(),
                                            app.restart_count,
                                            app.cron_run_history,
                                        )
                                        .await;
                                    continue;
                                }
                            }
                        }
                    }
                }
                self.manager
                    .register_stopped_restored(
                        app.id,
                        app.config,
                        app.restart_count,
                        app.cron_run_history,
                    )
                    .await;
                continue;
            }

            if app.config.cron.is_some() {
                // Kill any stale PID first (cron jobs are idempotent)
                if let Some(pid) = app.last_pid {
                    if let Some(identity) = verified_identity.as_ref() {
                        tracing::info!(
                            "killing stale cron process '{}' (PID {}) before re-registering",
                            app.config.name,
                            pid
                        );
                        if let Err(error) = kill_orphan_pid(pid, identity).await {
                            tracing::warn!(
                                "failed to stop stale cron process '{}' (PID {}): {error}",
                                app.config.name,
                                pid
                            );
                            if process_identity_matches(pid, identity) {
                                self.manager
                                    .register_running_adopted(
                                        app.id,
                                        app.config,
                                        pid,
                                        identity.clone(),
                                        app.restart_count,
                                        app.cron_run_history,
                                    )
                                    .await;
                                continue;
                            }
                        }
                    } else if live_identity.is_some() {
                        self.manager
                            .register_stopped_restored(
                                app.id,
                                app.config,
                                app.restart_count,
                                app.cron_run_history,
                            )
                            .await;
                        continue;
                    }
                }
                if app.cron_was_active {
                    // Cron scheduler was running at shutdown — restore as Sleeping (re-arm scheduler)
                    let fallback_config = app.config.clone();
                    let fallback_history = app.cron_run_history.clone();
                    if let Err(e) = self
                        .manager
                        .register_sleeping(
                            app.id,
                            app.config,
                            app.restart_count,
                            app.cron_run_history,
                        )
                        .await
                    {
                        tracing::warn!("failed to restore cron process '{}': {e}", app.id);
                        self.manager
                            .register_stopped_restored(
                                app.id,
                                fallback_config,
                                app.restart_count,
                                fallback_history,
                            )
                            .await;
                    }
                } else {
                    // User had manually stopped this cron job — restore as Stopped, don't re-arm
                    tracing::info!(
                        "cron process '{}' was stopped at shutdown — restoring as stopped",
                        app.config.name
                    );
                    self.manager
                        .register_stopped_restored(
                            app.id,
                            app.config,
                            app.restart_count,
                            app.cron_run_history,
                        )
                        .await;
                }
                continue;
            }

            match (app.last_pid, verified_identity) {
                (Some(pid), Some(identity)) => {
                    // Process survived the daemon restart — re-adopt it with its saved ID
                    tracing::info!(
                        "re-adopting running process '{}' (PID {})",
                        app.config.name,
                        pid
                    );
                    self.manager
                        .register_running_adopted(
                            app.id,
                            app.config,
                            pid,
                            identity,
                            app.restart_count,
                            app.cron_run_history,
                        )
                        .await;
                }
                (Some(pid), None) => {
                    // Process died while daemon was down — mark stopped, let user restart
                    tracing::info!(
                        "process '{}' (PID {}) exited while daemon was down — marking stopped",
                        app.config.name,
                        pid
                    );
                    self.manager
                        .register_stopped_restored(
                            app.id,
                            app.config,
                            app.restart_count,
                            app.cron_run_history,
                        )
                        .await;
                }
                (None, _) => {
                    // No PID was ever saved — mark stopped
                    self.manager
                        .register_stopped_restored(
                            app.id,
                            app.config,
                            app.restart_count,
                            app.cron_run_history,
                        )
                        .await;
                }
            }
        }
    }
}

fn load_saved_state(path: &std::path::Path) -> Result<SavedState> {
    crate::config::atomic_file::load_json_with_backup_validated(path, SavedState::validate)
}

#[derive(Debug, Clone)]
pub struct StreamTicket {
    pub method: Method,
    pub path: String,
    pub query: Option<String>,
    pub expires_at: DateTime<Utc>,
}

pub(crate) struct RestartAttempt {
    pub child: std::process::Child,
    pub handoff_path: std::path::PathBuf,
    pub handoff_token: String,
}

fn saved_app_from_snapshot(snapshot: ManagedProcessSnapshot) -> SavedApp {
    let info = snapshot.info;
    let cron_was_active = info.cron.is_some()
        && snapshot.desired_running
        && matches!(
            info.status,
            crate::models::process_status::ProcessStatus::Sleeping
                | crate::models::process_status::ProcessStatus::Starting
                | crate::models::process_status::ProcessStatus::Running
                | crate::models::process_status::ProcessStatus::Stopping
        );
    SavedApp {
        id: info.id,
        config: snapshot.config,
        restart_count: info.restart_count,
        cron_run_history: info.cron_run_history,
        last_pid: info.pid,
        process_identity: snapshot.process_identity,
        // A transient in-flight state can still belong to an active Cron job,
        // but failed/crashed/stopped jobs must remain fail-closed after restart.
        cron_was_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::process_status::ProcessStatus;
    use crate::process::instance::ManagedProcess;
    use std::collections::HashMap;

    fn isolated_state_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("alter-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.json")
    }

    #[test]
    fn first_run_without_primary_or_backup_loads_empty_state() {
        let path = isolated_state_path();
        let loaded = load_saved_state(&path).unwrap();
        assert!(loaded.apps.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn cron_snapshot_rearms_only_normal_desired_running_states() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "name": "cron-snapshot",
            "script": "job.exe",
            "cwd": null,
            "cron": "0 * * * *"
        }))
        .unwrap();
        let id = Uuid::new_v4();

        let saved_for = |status, desired_running| {
            let mut process = ManagedProcess::new_with_id(id, config.clone());
            process.status = status;
            process.desired_running = desired_running;
            saved_app_from_snapshot(ManagedProcessSnapshot {
                info: process.to_info(),
                config: config.clone(),
                process_identity: None,
                desired_running,
                generation: 1,
            })
            .cron_was_active
        };

        for status in [
            ProcessStatus::Sleeping,
            ProcessStatus::Starting,
            ProcessStatus::Running,
            ProcessStatus::Stopping,
        ] {
            assert!(saved_for(status, true));
        }
        for status in [
            ProcessStatus::Stopped,
            ProcessStatus::Crashed,
            ProcessStatus::Errored,
            ProcessStatus::Watching,
        ] {
            assert!(!saved_for(status, true));
        }
        assert!(!saved_for(ProcessStatus::Sleeping, false));
    }

    #[test]
    fn corrupt_primary_recovers_only_from_valid_backup() {
        let path = isolated_state_path();
        std::fs::write(&path, "{broken").unwrap();
        assert!(load_saved_state(&path).is_err());

        let backup = path.with_extension("json.bak");
        std::fs::write(&backup, r#"{"saved_at":null,"apps":[]}"#).unwrap();
        let loaded = load_saved_state(&path).unwrap();
        assert!(loaded.apps.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn state_loader_rejects_duplicate_process_ids() {
        let path = isolated_state_path();
        let id = Uuid::new_v4();
        let app = serde_json::json!({
            "id": id,
            "config": { "name": "duplicate", "script": "duplicate.exe", "cwd": null },
            "restart_count": 0
        });
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SAVED_STATE_SCHEMA_VERSION,
                "saved_at": null,
                "apps": [app.clone(), app]
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(load_saved_state(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn shutdown_request_is_sticky_for_late_subscribers() {
        let state = DaemonState::new_isolated(DaemonConfig::default());
        assert!(!state.is_shutdown_requested());
        state.request_shutdown();
        assert!(state.is_shutdown_requested());
        assert!(state.external_shutdown_requested());
        assert!(!state.commit_restart_handoff());
    }

    #[tokio::test]
    async fn failed_restart_can_resume_only_without_an_external_shutdown() {
        let restart_only = DaemonState::new_isolated(DaemonConfig::default());
        restart_only.request_restart_shutdown();
        assert!(restart_only.is_shutdown_requested());
        assert!(restart_only.resume_after_failed_restart());
        assert!(!restart_only.is_shutdown_requested());

        let externally_stopped = DaemonState::new_isolated(DaemonConfig::default());
        externally_stopped.request_restart_shutdown();
        externally_stopped.request_shutdown();
        assert!(!externally_stopped.resume_after_failed_restart());
        assert!(externally_stopped.is_shutdown_requested());
    }

    #[tokio::test]
    async fn saved_state_preserves_complete_app_config() {
        let notifications = Arc::new(RwLock::new(NotificationsStore::default()));
        let manager = ProcessManager::new(notifications);
        let id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let config = AppConfig {
            name: "full-config".to_string(),
            project_id: Some(project_id),
            script: "server.exe".to_string(),
            args: vec!["--serve".to_string()],
            cwd: Some("C:\\work".to_string()),
            instances: 3,
            autorestart: false,
            max_restarts: 7,
            restart_delay_ms: 2_500,
            watch: true,
            watch_paths: vec!["src".to_string()],
            watch_ignore: vec!["target".to_string()],
            env: HashMap::from([("MODE".to_string(), "test".to_string())]),
            namespace: "tests".to_string(),
            log_file: Some("app.log".to_string()),
            error_file: Some("error.log".to_string()),
            max_log_size_mb: 42,
            cron: None,
            cron_last_run: None,
            cron_next_run: None,
            notify: None,
            log_alert: None,
            env_file: Some(".env.local".to_string()),
            health_check_url: Some("http://127.0.0.1:8080/health".to_string()),
            health_check_interval_secs: 11,
            health_check_timeout_secs: 4,
            health_check_retries: 5,
            pre_start: Some("prepare".to_string()),
            post_start: Some("announce".to_string()),
            pre_stop: Some("cleanup".to_string()),
            enabled: false,
        };

        manager.register_stopped(id, config).await;
        let saved = SavedState {
            schema_version: SAVED_STATE_SCHEMA_VERSION,
            saved_at: Some(Utc::now()),
            apps: manager
                .snapshot()
                .await
                .into_iter()
                .map(saved_app_from_snapshot)
                .collect(),
        };
        let decoded: SavedState =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let restored = &decoded.apps[0].config;

        assert_eq!(restored.project_id, Some(project_id));
        assert_eq!(restored.instances, 3);
        assert_eq!(restored.restart_delay_ms, 2_500);
        assert_eq!(restored.watch_paths, ["src"]);
        assert_eq!(restored.watch_ignore, ["target"]);
        assert_eq!(restored.log_file.as_deref(), Some("app.log"));
        assert_eq!(restored.error_file.as_deref(), Some("error.log"));
        assert_eq!(restored.env_file.as_deref(), Some(".env.local"));
        assert_eq!(
            restored.health_check_url.as_deref(),
            Some("http://127.0.0.1:8080/health")
        );
        assert_eq!(restored.health_check_interval_secs, 11);
        assert_eq!(restored.health_check_timeout_secs, 4);
        assert_eq!(restored.health_check_retries, 5);
        assert_eq!(restored.pre_start.as_deref(), Some("prepare"));
        assert_eq!(restored.post_start.as_deref(), Some("announce"));
        assert_eq!(restored.pre_stop.as_deref(), Some("cleanup"));
        assert!(!restored.enabled);
        assert_eq!(decoded.apps[0].id, id);
        assert_eq!(decoded.apps[0].restart_count, 0);
        assert_eq!(decoded.apps[0].last_pid, None);
        assert!(!decoded.apps[0].cron_was_active);
        assert_eq!(
            ProcessStatus::Stopped,
            manager.get(id).await.unwrap().status
        );
    }

    #[tokio::test]
    async fn restore_preserves_restart_budget_and_stopped_cron_history() {
        let state = DaemonState::new_isolated(DaemonConfig::default());
        let regular_id = Uuid::new_v4();
        let cron_id = Uuid::new_v4();
        let regular_config: AppConfig = serde_json::from_value(serde_json::json!({
            "name": "regular",
            "script": "regular.exe",
            "cwd": null
        }))
        .unwrap();
        let cron_config: AppConfig = serde_json::from_value(serde_json::json!({
            "name": "cron",
            "script": "cron.exe",
            "cwd": null,
            "cron": "0 * * * *"
        }))
        .unwrap();
        let run = CronRun {
            run_at: Utc::now(),
            exit_code: Some(0),
            duration_secs: 2,
        };

        state
            .restore(SavedState {
                schema_version: SAVED_STATE_SCHEMA_VERSION,
                saved_at: Some(Utc::now()),
                apps: vec![
                    SavedApp {
                        id: regular_id,
                        config: regular_config,
                        restart_count: 4,
                        cron_run_history: Vec::new(),
                        last_pid: None,
                        process_identity: None,
                        cron_was_active: false,
                    },
                    SavedApp {
                        id: cron_id,
                        config: cron_config,
                        restart_count: 3,
                        cron_run_history: vec![run],
                        last_pid: None,
                        process_identity: None,
                        cron_was_active: false,
                    },
                ],
            })
            .await;

        assert_eq!(
            state.manager.get(regular_id).await.unwrap().restart_count,
            4
        );
        let cron = state.manager.get(cron_id).await.unwrap();
        assert_eq!(cron.restart_count, 3);
        assert_eq!(cron.cron_run_history.len(), 1);
        assert_eq!(cron.status, ProcessStatus::Stopped);
    }

    #[tokio::test]
    async fn invalid_active_cron_is_retained_as_stopped_during_restore() {
        let state = DaemonState::new_isolated(DaemonConfig::default());
        let id = Uuid::new_v4();
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "name": "invalid-cron",
            "script": "invalid-cron.exe",
            "cwd": null,
            "cron": "not a cron expression"
        }))
        .unwrap();

        state
            .restore(SavedState {
                schema_version: SAVED_STATE_SCHEMA_VERSION,
                saved_at: Some(Utc::now()),
                apps: vec![SavedApp {
                    id,
                    config,
                    restart_count: 2,
                    cron_run_history: Vec::new(),
                    last_pid: None,
                    process_identity: None,
                    cron_was_active: true,
                }],
            })
            .await;

        let restored = state.manager.get(id).await.unwrap();
        assert_eq!(restored.status, ProcessStatus::Stopped);
        assert_eq!(restored.restart_count, 2);
    }
}
