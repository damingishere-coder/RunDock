// @group BusinessLogic : Process manager — spawns, tracks, stops, and restarts all child processes

use crate::config::ecosystem::AppConfig;
use crate::config::notification_store::NotificationsStore;
use crate::logging::writer::LogWriter;
use crate::models::cron_run::{CronRun, MAX_CRON_HISTORY};
use crate::models::metric_sample::MetricSample;
use crate::models::notification::NotificationConfig;
use crate::models::process_info::ProcessInfo;
use crate::models::process_status::ProcessStatus;
use crate::notifications::sender::{fire_event, ProcessEvent};
use crate::process::identity::{
    capture_process_identity_with_retry, process_identity_matches, stable_identity_matches,
};
use crate::process::instance::{read_git_branch, LogLine, ManagedProcess, ProcessIdentity};
use crate::process::restarter::{watch_and_restart, RestartEvent, RestartPolicy};
use crate::process::runner::{spawn_process, wait_for_exit, ManagedChild};
use crate::process::scheduler::{next_run, CronScheduler};
use crate::process::watcher::FileWatcher;
use anyhow::{anyhow, Result};
use chrono::Utc;
use dashmap::DashMap;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::mpsc;
use tokio::sync::{broadcast, Mutex, RwLock, Semaphore};
use uuid::Uuid;

// @group Constants : Maximum number of metric samples retained per process (1440 × 60 s = 24 h)
const MAX_METRIC_SAMPLES: usize = 1440;
// @group Constants : Collect one metric sample every N metric-loop ticks (tick = 3 s → 20 × 3 s = 60 s)
const METRIC_SAMPLE_INTERVAL_TICKS: u32 = 20;

pub type ProcessRegistry = DashMap<Uuid, Arc<RwLock<ManagedProcess>>>;

#[derive(Debug, Clone)]
pub struct ManagedProcessSnapshot {
    pub info: ProcessInfo,
    pub config: AppConfig,
    pub process_identity: Option<ProcessIdentity>,
    pub desired_running: bool,
    pub generation: u64,
}

#[derive(Debug, Serialize)]
pub struct BulkProcessFailure {
    pub id: Uuid,
    pub error: String,
}

#[derive(Debug, Default)]
pub struct BulkProcessResult {
    pub attempted: usize,
    pub processes: Vec<ProcessInfo>,
    pub failures: Vec<BulkProcessFailure>,
}

#[derive(Clone)]
pub struct ProcessManager {
    pub registry: Arc<ProcessRegistry>,
    restart_tx: mpsc::Sender<RestartEvent>,
    /// Cron trigger channel — scheduler sends process_id when the next tick fires
    cron_trigger_tx: mpsc::Sender<Uuid>,
    /// Active CronScheduler handles, keyed by process_id
    cron_schedulers: Arc<Mutex<HashMap<Uuid, CronScheduler>>>,
    /// Shared notification store for firing alerts on process events
    notifications: Arc<RwLock<NotificationsStore>>,
    /// Suppress per-process Telegram notifications during bulk namespace ops.
    /// Value = remaining events to suppress (2 for restart = stop+start, 1 otherwise).
    pub bulk_suppress: Arc<DashMap<Uuid, u32>>,
    // @group BusinessLogic > Metrics : Rolling per-process metric history (CPU + mem samples)
    pub metrics_history: Arc<DashMap<Uuid, Mutex<VecDeque<MetricSample>>>>,
    /// Background lifecycle changes ask DaemonState to persist a fresh snapshot.
    persistence_tx: broadcast::Sender<()>,
    /// Bounds concurrent restart transactions and prevents task storms.
    lifecycle_limit: Arc<Semaphore>,
}

impl ProcessManager {
    #[cfg(unix)]
    pub async fn relinquish_process_trees_after_restart_handoff(&self) {
        let processes = self
            .registry
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect::<Vec<_>>();
        for process in processes {
            if let Some(mut process_tree) = process.write().await.process_tree.take() {
                process_tree.relinquish_without_termination();
            }
        }
    }

    pub fn new(notifications: Arc<RwLock<NotificationsStore>>) -> Self {
        let registry = Arc::new(DashMap::new());
        let (restart_tx, restart_rx) = mpsc::channel::<RestartEvent>(256);
        let (cron_trigger_tx, cron_trigger_rx) = mpsc::channel::<Uuid>(256);
        let cron_schedulers: Arc<Mutex<HashMap<Uuid, CronScheduler>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let metrics_history: Arc<DashMap<Uuid, Mutex<VecDeque<MetricSample>>>> =
            Arc::new(DashMap::new());
        let (persistence_tx, _) = broadcast::channel(64);
        let manager = Self {
            registry,
            restart_tx,
            cron_trigger_tx,
            cron_schedulers,
            notifications,
            bulk_suppress: Arc::new(DashMap::new()),
            metrics_history,
            persistence_tx,
            lifecycle_limit: Arc::new(Semaphore::new(8)),
        };

        let restart_manager = manager.clone();

        // @group BusinessLogic > Restarter : Background task that handles restart events
        tokio::spawn(async move {
            Self::restart_loop(restart_manager, restart_rx).await;
        });

        let reg_cron = Arc::clone(&manager.registry);
        let cron_sched_clone = Arc::clone(&manager.cron_schedulers);
        let ctrigger_tx_clone = manager.cron_trigger_tx.clone();
        let notif_cron = Arc::clone(&manager.notifications);
        let persistence_cron = manager.persistence_tx.clone();
        let cron_manager = manager.clone();

        // @group BusinessLogic > Cron : Background task that handles cron trigger events
        tokio::spawn(async move {
            Self::cron_trigger_loop(
                cron_manager,
                reg_cron,
                cron_trigger_rx,
                cron_sched_clone,
                ctrigger_tx_clone,
                notif_cron,
                persistence_cron,
            )
            .await;
        });

        let reg_metrics = Arc::clone(&manager.registry);
        let hist_metrics = Arc::clone(&manager.metrics_history);

        // @group BusinessLogic > Metrics : Background task that polls CPU and memory per process
        tokio::spawn(async move {
            Self::metrics_loop(reg_metrics, hist_metrics).await;
        });

        let reg_alert = Arc::clone(&manager.registry);
        let notif_alert = Arc::clone(&manager.notifications);

        // @group BusinessLogic > LogAlerts : Background task that checks stderr spikes every 5 minutes
        tokio::spawn(async move {
            Self::log_alert_loop(reg_alert, notif_alert).await;
        });

        manager
    }

    pub fn subscribe_persistence(&self) -> broadcast::Receiver<()> {
        self.persistence_tx.subscribe()
    }

    // @group Utilities > BulkSuppress : Decrement suppress counter; return true if the event should be suppressed
    fn suppress_consume(suppress: &DashMap<Uuid, u32>, id: &Uuid) -> bool {
        if let Some(mut entry) = suppress.get_mut(id) {
            if *entry > 1 {
                *entry -= 1;
            } else {
                drop(entry);
                suppress.remove(id);
            }
            return true;
        }
        false
    }

    async fn terminate_retained_process_tree(
        process: &Arc<RwLock<ManagedProcess>>,
        generation: u64,
    ) -> Result<()> {
        enum CleanupOwner {
            Guard(crate::process::tree::ProcessTreeGuard, Option<u32>),
            Identity(u32, ProcessIdentity),
        }

        let mut cleanup_waits = 0u16;
        let cleanup_owner = loop {
            let mut process = process.write().await;
            if process.process_tree_cleanup_in_progress {
                cleanup_waits = cleanup_waits.saturating_add(1);
                if cleanup_waits >= 240 {
                    return Err(anyhow!(
                        "another process-tree cleanup did not finish within 6 seconds"
                    ));
                }
                drop(process);
                tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
                continue;
            }
            if process.generation != generation {
                return Err(anyhow!(
                    "process lifecycle changed while cleaning up its process tree"
                ));
            }
            match process.process_tree.take() {
                Some(process_tree) => {
                    let owned_pid = process.pid;
                    process.process_tree_cleanup_in_progress = true;
                    break CleanupOwner::Guard(process_tree, owned_pid);
                }
                None if process.pid.is_none() => return Ok(()),
                None => match (process.pid, process.process_identity.clone()) {
                    (Some(pid), Some(identity)) => {
                        process.process_tree_cleanup_in_progress = true;
                        break CleanupOwner::Identity(pid, identity);
                    }
                    _ => {
                        return Err(anyhow!(
                            "no owned process-tree handle or stable identity is available for PID {:?}",
                            process.pid
                        ));
                    }
                },
            }
        };

        match cleanup_owner {
            CleanupOwner::Guard(process_tree, owned_pid) => {
                if let Err(error) = process_tree.terminate_and_wait().await {
                    let mut process = process.write().await;
                    process.process_tree_cleanup_in_progress = false;
                    if process.process_tree.is_none() && process.pid == owned_pid {
                        process.process_tree = Some(process_tree);
                    }
                    return Err(error);
                }
                let mut process = process.write().await;
                process.process_tree_cleanup_in_progress = false;
                if process.process_tree.is_none() && process.pid == owned_pid {
                    process.pid = None;
                    process.process_identity = None;
                }
            }
            CleanupOwner::Identity(pid, identity) => {
                if let Err(error) =
                    crate::process::tree::ProcessTreeGuard::terminate_unowned_existing(
                        pid, &identity,
                    )
                    .await
                {
                    process.write().await.process_tree_cleanup_in_progress = false;
                    return Err(error);
                }
                let mut process = process.write().await;
                process.process_tree_cleanup_in_progress = false;
                if process.process_tree.is_none() && process.pid == Some(pid) {
                    process.pid = None;
                    process.process_identity = None;
                }
            }
        }
        Ok(())
    }

    // @group BusinessLogic > Lifecycle : Start a new process from config
    pub async fn start(&self, config: AppConfig) -> Result<ProcessInfo> {
        config.validate()?;
        let mut process = ManagedProcess::new(config);
        process.refresh_git_branch().await;
        let id = process.id;

        let arc = Arc::new(RwLock::new(process));
        self.registry.insert(id, Arc::clone(&arc));

        if let Err(error) = self.do_spawn(id).await {
            // cleanup_failed_spawn retains an Errored entry when termination
            // cannot be proved. Only remove a failed start after no PID remains.
            if arc.read().await.pid.is_none() {
                self.registry.remove(&id);
            }
            return Err(error);
        }

        let guard = arc.read().await;
        Ok(guard.to_info())
    }

    // @group BusinessLogic > Lifecycle : Register a process as Stopped without spawning (used on restore)
    // Takes the persisted UUID so IDs remain stable across daemon restarts.
    pub async fn register_stopped(&self, id: Uuid, config: AppConfig) -> ProcessInfo {
        self.register_stopped_restored(id, config, 0, Vec::new())
            .await
    }

    pub async fn register_stopped_restored(
        &self,
        id: Uuid,
        config: AppConfig,
        restart_count: u32,
        cron_run_history: Vec<CronRun>,
    ) -> ProcessInfo {
        let mut process = ManagedProcess::new_with_id(id, config);
        process.restart_count = restart_count;
        process.cron_run_history = cron_run_history;
        let info = process.to_info();
        let arc = Arc::new(RwLock::new(process));
        self.registry.insert(id, Arc::clone(&arc));
        Self::refresh_git_branch_in_background(arc);
        info
    }

    fn refresh_git_branch_in_background(process: Arc<RwLock<ManagedProcess>>) {
        tokio::spawn(async move {
            let cwd = { process.read().await.config.cwd.clone() };
            let branch = match cwd.as_deref() {
                Some(cwd) => read_git_branch(cwd).await,
                None => None,
            };
            let mut current = process.write().await;
            if current.config.cwd == cwd {
                current.git_branch = branch;
            }
        });
    }

    // @group BusinessLogic > Lifecycle : Re-adopt an already-running OS process after a daemon crash.
    // We cannot re-attach stdout/stderr, but health checks and file-watch restart
    // behavior are restored immediately. A polling watcher detects process exit.
    pub async fn register_running_adopted(
        &self,
        saved_id: Uuid,
        config: AppConfig,
        pid: u32,
        identity: ProcessIdentity,
        restart_count: u32,
        cron_run_history: Vec<CronRun>,
    ) -> ProcessInfo {
        let mut process = ManagedProcess::new_with_id(saved_id, config.clone());
        let mut process_tree = match crate::process::tree::ProcessTreeGuard::new(
            pid,
            &saved_id.to_string(),
        ) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                tracing::error!(%error, %saved_id, pid, "adopted process tree ownership could not be restored; stopping the unowned process");
                if let Err(kill_error) =
                    crate::process::tree::ProcessTreeGuard::terminate_unowned_existing(
                        pid, &identity,
                    )
                    .await
                {
                    tracing::error!(%kill_error, %saved_id, pid, "failed to stop unowned adopted process");
                    process.status = ProcessStatus::Errored;
                    process.pid = Some(pid);
                    process.process_identity = Some(identity);
                    process.desired_running = false;
                    process.generation = 1;
                    process.restart_count = restart_count;
                    process.cron_run_history = cron_run_history;
                    let info = process.to_info();
                    self.registry
                        .insert(saved_id, Arc::new(RwLock::new(process)));
                    return info;
                }
                return self
                    .register_stopped_restored(saved_id, config, restart_count, cron_run_history)
                    .await;
            }
        };
        if let Err(error) = process_tree.preserve_on_drop() {
            tracing::error!(%error, %saved_id, pid, "adopted process tree could not be preserved after daemon exit");
            if let Err(kill_error) = process_tree.terminate_and_wait().await {
                tracing::error!(%kill_error, %saved_id, pid, "failed to stop adopted process after process-tree preservation failed");
                process.status = ProcessStatus::Errored;
                process.pid = Some(pid);
                process.process_identity = Some(identity);
                process.process_tree = Some(process_tree);
                process.desired_running = false;
                process.generation = 1;
                process.restart_count = restart_count;
                process.cron_run_history = cron_run_history;
                let info = process.to_info();
                self.registry
                    .insert(saved_id, Arc::new(RwLock::new(process)));
                return info;
            }
            return self
                .register_stopped_restored(saved_id, config, restart_count, cron_run_history)
                .await;
        }
        process.status = if config.watch {
            ProcessStatus::Watching
        } else {
            ProcessStatus::Running
        };
        process.pid = Some(pid);
        process.process_identity = Some(identity.clone());
        process.process_tree = Some(process_tree);
        process.started_at = Some(Utc::now()); // approximate — original start time is unknown
        process.desired_running = true;
        process.generation = 1;
        process.restart_count = restart_count;
        process.cron_run_history = cron_run_history;

        let id = process.id;
        let generation = process.generation;
        let info = process.to_info();
        let arc = Arc::new(RwLock::new(process));
        self.registry.insert(id, Arc::clone(&arc));
        Self::refresh_git_branch_in_background(Arc::clone(&arc));

        // A running cron process may be adopted mid-run. Arm its scheduler now;
        // cron_trigger_loop ignores ticks until the process transitions to Sleeping.
        if config.enabled {
            if let Some(expr) = &config.cron {
                match CronScheduler::start(id, expr, self.cron_trigger_tx.clone()) {
                    Ok(scheduler) => {
                        self.cron_schedulers.lock().await.insert(id, scheduler);
                    }
                    Err(error) => {
                        tracing::error!(%error, %id, "failed to restore scheduler for adopted cron process");
                    }
                }
            }
        }

        // Restore an owned writer handle so new in-daemon log messages retain
        // the same lifecycle. OS stdout/stderr cannot be re-attached.
        let log_dir = crate::config::paths::process_log_dir(&config.name);
        match std::fs::create_dir_all(&log_dir) {
            Ok(()) => {
                // Drop the read guard before acquiring the write guard below.
                // Keeping `arc.read().await` inside the match scrutinee extends
                // the temporary guard through the match and deadlocks restore.
                let log_tx = { arc.read().await.log_tx.clone() };
                match LogWriter::new(&log_dir, log_tx, config.max_log_size_mb) {
                    Ok(writer) => arc.write().await.log_writer = Some(writer),
                    Err(error) => tracing::warn!(
                        "failed to restore log writer for adopted process {id}: {error}"
                    ),
                }
            }
            Err(error) => tracing::warn!(
                "failed to create the log directory for adopted process {id}; logging is degraded: {error}"
            ),
        }

        if let Some(url) = &config.health_check_url {
            let handle = crate::process::health::start_health_check(
                Arc::clone(&arc),
                generation,
                url.clone(),
                config.health_check_interval_secs,
                config.health_check_timeout_secs,
                config.health_check_retries,
                Arc::clone(&self.notifications),
            );
            let mut proc = arc.write().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                handle.abort();
            } else {
                proc.health_check_handle = Some(handle);
            }
        }

        if config.watch && !config.watch_paths.is_empty() {
            let (watch_tx, mut watch_rx) = mpsc::channel::<Uuid>(8);
            let restart_tx = self.restart_tx.clone();
            let registry = Arc::clone(&self.registry);
            tokio::spawn(async move {
                while let Some(process_id) = watch_rx.recv().await {
                    let Some(entry) = registry.get(&process_id) else {
                        continue;
                    };
                    let process = entry.read().await;
                    if !process.desired_running {
                        continue;
                    }
                    let generation = process.generation;
                    drop(process);
                    let _ = restart_tx
                        .send(RestartEvent::Restart {
                            process_id,
                            generation,
                        })
                        .await;
                }
            });
            match FileWatcher::start(id, &config.watch_paths, &config.watch_ignore, watch_tx) {
                Ok(watcher) => arc.write().await.file_watcher = Some(watcher),
                Err(error) => tracing::error!(
                    "failed to restore file watcher for adopted process {id}: {error}"
                ),
            }
        }

        // @group BusinessLogic > AdoptedWatcher : Poll every 2s until the adopted PID exits
        let registry = Arc::clone(&self.registry);
        let restart_tx = self.restart_tx.clone();
        let persistence_tx = self.persistence_tx.clone();

        tokio::spawn(async move {
            let mut missing_identity_samples = 0u8;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                if process_identity_matches(pid, &identity) {
                    missing_identity_samples = 0;
                    continue;
                }
                missing_identity_samples = missing_identity_samples.saturating_add(1);
                if missing_identity_samples >= 3 {
                    break;
                }
            }

            // PID exited — update state and decide whether to restart
            if let Err(error) = Self::terminate_retained_process_tree(&arc, generation).await {
                tracing::error!(%error, %id, "could not confirm adopted process descendants were terminated");
                let mut proc = arc.write().await;
                if proc.generation == generation && proc.desired_running {
                    proc.status = ProcessStatus::Errored;
                    proc.desired_running = false;
                    drop(proc);
                    let _ = persistence_tx.send(());
                }
                return;
            }
            let (is_cron, autorestart, restart_count, max_restarts, restart_delay_ms) = {
                match registry.get(&id) {
                    Some(entry) => {
                        let mut proc = entry.write().await;
                        if !proc.desired_running || proc.generation != generation {
                            return;
                        }
                        let is_cron = proc.config.cron.is_some();
                        proc.status = if is_cron && proc.config.enabled {
                            ProcessStatus::Sleeping
                        } else {
                            ProcessStatus::Stopped
                        };
                        proc.pid = None;
                        proc.process_identity = None;
                        proc.process_tree.take();
                        proc.stopped_at = Some(Utc::now());
                        proc.cron_next_run = proc.config.cron.as_deref().and_then(next_run);
                        (
                            is_cron,
                            !is_cron && proc.config.autorestart && proc.config.enabled,
                            proc.restart_count,
                            proc.config.max_restarts,
                            proc.config.restart_delay_ms,
                        )
                    }
                    None => return,
                }
            };

            let _ = persistence_tx.send(());
            if is_cron {
                return;
            }

            if autorestart && restart_count < max_restarts {
                tokio::time::sleep(tokio::time::Duration::from_millis(restart_delay_ms)).await;
                let _ = restart_tx
                    .send(RestartEvent::Restart {
                        process_id: id,
                        generation,
                    })
                    .await;
            } else {
                let _ = restart_tx
                    .send(RestartEvent::Exited {
                        process_id: id,
                        generation,
                        exit_code: None,
                    })
                    .await;
            }
        });

        info
    }

    // @group BusinessLogic > Lifecycle : Register a cron process as Sleeping without spawning (used on restore)
    pub async fn register_sleeping(
        &self,
        id: Uuid,
        config: AppConfig,
        restart_count: u32,
        cron_run_history: Vec<CronRun>,
    ) -> Result<ProcessInfo> {
        let mut process = ManagedProcess::new_with_id(id, config.clone());
        process.status = ProcessStatus::Sleeping;
        process.desired_running = true;
        process.generation = 1;
        process.restart_count = restart_count;
        process.cron_run_history = cron_run_history;
        if let Some(expr) = &config.cron {
            process.cron_next_run = next_run(expr);
        }
        let id = process.id;
        let info = process.to_info();
        let arc = Arc::new(RwLock::new(process));
        self.registry.insert(id, Arc::clone(&arc));
        Self::refresh_git_branch_in_background(Arc::clone(&arc));

        // Start the scheduler so it fires at the right time
        if let Some(expr) = &config.cron {
            let scheduler = match CronScheduler::start(id, expr, self.cron_trigger_tx.clone()) {
                Ok(scheduler) => scheduler,
                Err(error) => {
                    self.registry.remove(&id);
                    return Err(error);
                }
            };
            self.cron_schedulers.lock().await.insert(id, scheduler);
        }

        Ok(info)
    }

    // @group BusinessLogic > Lifecycle : Stop a running process
    pub async fn stop(&self, id: Uuid) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let (is_cron, pre_stop, cwd, env, previous_status, previous_generation, stop_generation) = {
            let mut proc = arc.write().await;
            let stoppable = matches!(
                proc.status,
                ProcessStatus::Starting
                    | ProcessStatus::Stopping
                    | ProcessStatus::Running
                    | ProcessStatus::Watching
                    | ProcessStatus::Sleeping
            ) || (matches!(proc.status, ProcessStatus::Errored)
                && proc.pid.is_some())
                || (matches!(proc.status, ProcessStatus::Stopped) && proc.desired_running);
            if !stoppable {
                return Err(anyhow!(
                    "process '{}' is not running, starting, or sleeping",
                    proc.config.name
                ));
            }

            let previous_status = proc.status.clone();
            let previous_generation = proc.generation;

            // Invalidate every exit/restart event created by the current spawn
            // before doing any asynchronous work.
            proc.desired_running = false;
            proc.generation = proc.generation.wrapping_add(1);
            let stop_generation = proc.generation;
            proc.status = ProcessStatus::Stopping;

            (
                proc.config.cron.is_some(),
                proc.config.pre_stop.clone(),
                proc.config.cwd.clone(),
                proc.config.env.clone(),
                previous_status,
                previous_generation,
                stop_generation,
            )
        };

        if let Some(cmd) = pre_stop {
            if let Err(e) = crate::process::hooks::run_hook(&cmd, cwd.as_deref(), &env).await {
                tracing::warn!("pre_stop hook failed: {e}");
            }
        }

        if let Err(error) = Self::terminate_retained_process_tree(&arc, stop_generation).await {
            let mut proc = arc.write().await;
            if proc.generation != stop_generation {
                return Err(error);
            }
            proc.status = previous_status;
            proc.desired_running = true;
            proc.generation = previous_generation;
            return Err(anyhow!(
                "failed to terminate the complete owned process tree for {id}: {error}"
            ));
        }

        if is_cron {
            if let Some(sched) = self.cron_schedulers.lock().await.remove(&id) {
                sched.abort();
            }
        }

        let info_for_notif = {
            let mut proc = arc.write().await;
            if proc.generation != stop_generation {
                return Ok(proc.to_info());
            }
            proc.status = ProcessStatus::Stopped;
            proc.pid = None;
            proc.process_identity = None;
            proc.process_tree.take();
            proc.stopped_at = Some(Utc::now());
            proc.cron_next_run = None;
            if let Some(handle) = proc.health_check_handle.take() {
                handle.abort();
            }
            proc.file_watcher = None;
            proc.log_writer = None;
            proc.to_info()
        };
        let notif = Arc::clone(&self.notifications);
        let suppress = Arc::clone(&self.bulk_suppress);
        let info_clone = info_for_notif.clone();
        tokio::spawn(async move {
            if !Self::suppress_consume(&suppress, &info_clone.id) {
                let store = notif.read().await;
                fire_event(&store, &info_clone, ProcessEvent::Stopped).await;
                crate::telegram::commands::fire_telegram_notification(
                    &info_clone,
                    ProcessEvent::Stopped,
                )
                .await;
            }
        });

        Ok(info_for_notif)
    }

    // @group BusinessLogic > Lifecycle : Start a registered stopped process.
    pub async fn start_existing(&self, id: Uuid) -> Result<ProcessInfo> {
        {
            let arc = self.get_arc(id)?;
            let proc = arc.read().await;
            if !proc.config.enabled {
                return Err(anyhow!("process '{}' is disabled", proc.config.name));
            }
            if !(1..=1024).contains(&proc.config.max_log_size_mb) {
                return Err(anyhow!("max_log_size_mb must be between 1 and 1024"));
            }
            if !matches!(
                proc.status,
                ProcessStatus::Stopped | ProcessStatus::Crashed | ProcessStatus::Errored
            ) {
                return Err(anyhow!("process '{}' is already active", proc.config.name));
            }
        }
        self.do_spawn(id).await?;
        self.get(id).await
    }

    // @group BusinessLogic > Lifecycle : Restart a process (stop then start)
    pub async fn restart(&self, id: Uuid) -> Result<ProcessInfo> {
        self.restart_internal(id, true).await
    }

    async fn restart_internal(&self, id: Uuid, notify: bool) -> Result<ProcessInfo> {
        let is_active = {
            let arc = self.get_arc(id)?;
            let proc = arc.read().await;
            if !proc.config.enabled {
                return Err(anyhow!("process '{}' is disabled", proc.config.name));
            }
            matches!(
                proc.status,
                ProcessStatus::Starting
                    | ProcessStatus::Running
                    | ProcessStatus::Watching
                    | ProcessStatus::Sleeping
            ) || proc.pid.is_some()
        };
        // Suppress the individual Stop + Start events so the caller can emit one
        // aggregate event after every required step has succeeded.
        self.bulk_suppress.insert(id, if is_active { 2 } else { 1 });
        if is_active {
            if let Err(error) = self.stop(id).await {
                self.bulk_suppress.remove(&id);
                return Err(error);
            }
        }
        if let Err(error) = self.do_spawn(id).await {
            self.bulk_suppress.remove(&id);
            return Err(error);
        }

        let arc = self.get_arc(id)?;
        let (info, restart_generation) = {
            let mut proc = arc.write().await;
            if !proc.desired_running
                || !matches!(
                    proc.status,
                    ProcessStatus::Running | ProcessStatus::Watching | ProcessStatus::Sleeping
                )
            {
                self.bulk_suppress.remove(&id);
                return Err(anyhow!(
                    "process restart was superseded by a newer lifecycle request"
                ));
            }
            proc.restart_count += 1;
            (proc.to_info(), proc.generation)
        };

        if notify {
            let notif = Arc::clone(&self.notifications);
            let registry = Arc::clone(&self.registry);
            let info_clone = info.clone();
            tokio::spawn(async move {
                let current = registry
                    .get(&info_clone.id)
                    .map(|entry| Arc::clone(entry.value()));
                let still_current = if let Some(current) = current {
                    let process = current.read().await;
                    process.generation == restart_generation
                        && process.desired_running
                        && matches!(
                            process.status,
                            ProcessStatus::Running
                                | ProcessStatus::Watching
                                | ProcessStatus::Sleeping
                        )
                } else {
                    false
                };
                if !still_current {
                    return;
                }
                let store = notif.read().await;
                fire_event(&store, &info_clone, ProcessEvent::Restarted).await;
                crate::telegram::commands::fire_telegram_notification(
                    &info_clone,
                    ProcessEvent::Restarted,
                )
                .await;
            });
        }

        Ok(info)
    }

    // @group BusinessLogic > Lifecycle : Delete a process (stop + remove from registry)
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        {
            let arc = self.get_arc(id)?;
            let proc = arc.read().await;
            let is_active = matches!(
                proc.status,
                ProcessStatus::Starting
                    | ProcessStatus::Running
                    | ProcessStatus::Watching
                    | ProcessStatus::Sleeping
            ) || (matches!(proc.status, ProcessStatus::Errored)
                && proc.pid.is_some())
                || (matches!(proc.status, ProcessStatus::Stopped) && proc.desired_running);
            if is_active {
                drop(proc);
                self.stop(id).await?;
            }
        }
        // Clean up scheduler if not already removed by stop()
        if let Some(sched) = self.cron_schedulers.lock().await.remove(&id) {
            sched.abort();
        }
        self.registry.remove(&id);
        self.metrics_history.remove(&id);
        Ok(())
    }

    // @group BusinessLogic > Lifecycle : Update config for a process (stop → patch config → restart if was running)
    pub async fn update(&self, id: Uuid, patch: AppConfig) -> Result<ProcessInfo> {
        patch.validate()?;
        let (was_active, original_config) = {
            let arc = self.get_arc(id)?;
            let proc = arc.read().await;
            if matches!(
                proc.status,
                ProcessStatus::Starting | ProcessStatus::Stopping
            ) {
                return Err(anyhow!(
                    "process '{}' is busy starting or stopping; retry the update",
                    proc.config.name
                ));
            }
            (
                matches!(
                    proc.status,
                    ProcessStatus::Running | ProcessStatus::Watching | ProcessStatus::Sleeping
                ) || (matches!(proc.status, ProcessStatus::Errored) && proc.pid.is_some())
                    || (matches!(proc.status, ProcessStatus::Stopped) && proc.desired_running),
                proc.config.clone(),
            )
        };

        let should_restart = was_active && patch.enabled;
        if was_active {
            self.stop(id).await?;
        }

        {
            let arc = self.get_arc(id)?;
            let mut proc = arc.write().await;
            proc.config = patch;
        }

        if should_restart {
            if let Err(update_error) = self.do_spawn(id).await {
                {
                    let arc = self.get_arc(id)?;
                    arc.write().await.config = original_config;
                }
                if let Err(rollback_error) = self.do_spawn(id).await {
                    return Err(anyhow!(
                        "updated configuration failed to start ({update_error}); rollback also failed ({rollback_error})"
                    ));
                }
                return Err(anyhow!(
                    "updated configuration failed to start and was rolled back: {update_error}"
                ));
            }
        }

        let arc = self.get_arc(id)?;
        let guard = arc.read().await;
        Ok(guard.to_info())
    }

    // @group BusinessLogic > Enabled : Toggle enabled flag for a process (persisted via caller's save_to_disk)
    pub async fn set_enabled(&self, id: Uuid, enabled: bool) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let (info, previous_enabled, cancel_cron, stop_running_cron, start_cron) = {
            let mut guard = arc.write().await;
            let previous_enabled = guard.config.enabled;
            guard.config.enabled = enabled;
            let cancel_cron = !enabled && guard.config.cron.is_some();
            let stop_running_cron = cancel_cron && guard.pid.is_some();
            if !enabled && guard.pid.is_none() {
                guard.desired_running = false;
                guard.generation = guard.generation.wrapping_add(1);
                guard.status = ProcessStatus::Stopped;
                guard.cron_next_run = None;
            }
            let start_cron = if enabled
                && !previous_enabled
                && guard.config.cron.is_some()
                && guard.pid.is_none()
                && matches!(guard.status, ProcessStatus::Stopped)
            {
                guard.config.cron.clone()
            } else {
                None
            };
            (
                guard.to_info(),
                previous_enabled,
                cancel_cron,
                stop_running_cron,
                start_cron,
            )
        };
        if cancel_cron {
            if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&id) {
                scheduler.abort();
            }
        }
        if stop_running_cron {
            return match self.stop(id).await {
                Ok(info) => Ok(info),
                Err(error) => {
                    let cron = {
                        let mut process = arc.write().await;
                        process.config.enabled = previous_enabled;
                        process.config.cron.clone()
                    };
                    if previous_enabled {
                        if let Some(expr) = cron {
                            let scheduler = CronScheduler::start(
                                id,
                                &expr,
                                self.cron_trigger_tx.clone(),
                            )
                            .map_err(|scheduler_error| {
                                anyhow!(
                                    "cron stop failed ({error}); scheduler rollback also failed ({scheduler_error})"
                                )
                            })?;
                            if let Some(previous) =
                                self.cron_schedulers.lock().await.insert(id, scheduler)
                            {
                                previous.abort();
                            }
                        }
                    }
                    Err(error)
                }
            };
        }
        if let Some(expr) = start_cron {
            let scheduler = match CronScheduler::start(id, &expr, self.cron_trigger_tx.clone()) {
                Ok(scheduler) => scheduler,
                Err(error) => {
                    arc.write().await.config.enabled = previous_enabled;
                    return Err(error);
                }
            };
            let info = {
                let mut process = arc.write().await;
                process.desired_running = true;
                process.generation = process.generation.wrapping_add(1);
                process.status = ProcessStatus::Sleeping;
                process.cron_next_run = next_run(&expr);
                process.to_info()
            };
            if let Some(previous) = self.cron_schedulers.lock().await.insert(id, scheduler) {
                previous.abort();
            }
            return Ok(info);
        }
        Ok(info)
    }

    // @group BusinessLogic > Notifications : Update notification metadata without restarting the process
    pub async fn set_notification_config(
        &self,
        id: Uuid,
        notify: Option<NotificationConfig>,
    ) -> Result<ProcessInfo> {
        if let Some(config) = notify.as_ref() {
            config.validate()?;
        }
        let arc = self.get_arc(id)?;
        let mut process = arc.write().await;
        process.config.notify = notify;
        Ok(process.to_info())
    }

    // @group BusinessLogic > Project : Change logical project membership
    // without touching the running child or any lifecycle configuration.
    pub async fn assign_project(&self, id: Uuid, project_id: Uuid) -> Result<ProcessInfo> {
        self.set_project_assignment(id, Some(project_id)).await
    }

    pub async fn set_project_assignment(
        &self,
        id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let mut guard = arc.write().await;
        guard.config.project_id = project_id;
        Ok(guard.to_info())
    }

    // @group BusinessLogic > Namespace : Start all stopped/crashed processes in a namespace
    pub async fn start_namespace(&self, namespace: &str) -> BulkProcessResult {
        let ids: Vec<Uuid> = {
            let mut result = vec![];
            for entry in self.registry.iter() {
                let proc = entry.value().read().await;
                if proc.config.namespace == namespace
                    && proc.config.enabled
                    && matches!(
                        proc.status,
                        ProcessStatus::Stopped | ProcessStatus::Crashed | ProcessStatus::Errored
                    )
                {
                    result.push(proc.id);
                }
            }
            result
        };
        let mut result = BulkProcessResult {
            attempted: ids.len(),
            ..BulkProcessResult::default()
        };
        for id in ids {
            self.bulk_suppress.insert(id, 1);
            match self.do_spawn(id).await {
                Ok(()) => match self.get(id).await {
                    Ok(info) => result.processes.push(info),
                    Err(error) => result.failures.push(BulkProcessFailure {
                        id,
                        error: error.to_string(),
                    }),
                },
                Err(error) => {
                    self.bulk_suppress.remove(&id);
                    result.failures.push(BulkProcessFailure {
                        id,
                        error: error.to_string(),
                    });
                }
            }
        }
        result
    }

    // @group BusinessLogic > Namespace : Stop all running processes in a namespace
    pub async fn stop_namespace(&self, namespace: &str) -> BulkProcessResult {
        let ids: Vec<Uuid> = {
            let mut result = vec![];
            for entry in self.registry.iter() {
                let proc = entry.value().read().await;
                if proc.config.namespace == namespace
                    && matches!(
                        proc.status,
                        ProcessStatus::Running | ProcessStatus::Watching | ProcessStatus::Sleeping
                    )
                {
                    result.push(proc.id);
                }
            }
            result
        };
        let mut result = BulkProcessResult {
            attempted: ids.len(),
            ..BulkProcessResult::default()
        };
        for id in ids {
            self.bulk_suppress.insert(id, 1);
            match self.stop(id).await {
                Ok(info) => result.processes.push(info),
                Err(error) => {
                    self.bulk_suppress.remove(&id);
                    result.failures.push(BulkProcessFailure {
                        id,
                        error: error.to_string(),
                    });
                }
            }
        }
        result
    }

    // @group BusinessLogic > Namespace : Restart all processes and report partial failures
    pub async fn restart_namespace(&self, namespace: &str) -> BulkProcessResult {
        let ids: Vec<Uuid> = {
            let mut result = vec![];
            for entry in self.registry.iter() {
                let proc = entry.value().read().await;
                if proc.config.namespace == namespace {
                    result.push(proc.id);
                }
            }
            result
        };
        let mut result = BulkProcessResult {
            attempted: ids.len(),
            ..BulkProcessResult::default()
        };
        for id in ids {
            match self.restart_internal(id, false).await {
                Ok(info) => result.processes.push(info),
                Err(error) => {
                    self.bulk_suppress.remove(&id);
                    result.failures.push(BulkProcessFailure {
                        id,
                        error: error.to_string(),
                    });
                }
            }
        }
        result
    }

    // @group BusinessLogic > Lifecycle : Reset restart counter
    pub async fn reset(&self, id: Uuid) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let mut proc = arc.write().await;
        proc.restart_count = 0;
        Ok(proc.to_info())
    }

    pub async fn set_restart_count(&self, id: Uuid, restart_count: u32) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let mut proc = arc.write().await;
        proc.restart_count = restart_count;
        Ok(proc.to_info())
    }

    // @group BusinessLogic > Query : List all process infos
    pub async fn list(&self) -> Vec<ProcessInfo> {
        let mut result = Vec::new();
        for entry in self.registry.iter() {
            let proc = entry.value().read().await;
            result.push(proc.to_info());
        }
        result.sort_by_key(|process| process.created_at);
        result
    }

    pub async fn get_config(&self, id: Uuid) -> Result<AppConfig> {
        let arc = self.get_arc(id)?;
        let config = arc.read().await.config.clone();
        Ok(config)
    }

    pub async fn snapshot_one(&self, id: Uuid) -> Result<ManagedProcessSnapshot> {
        let arc = self.get_arc(id)?;
        let process = arc.read().await;
        Ok(ManagedProcessSnapshot {
            info: process.to_info(),
            config: process.config.clone(),
            process_identity: process.process_identity.clone(),
            desired_running: process.desired_running,
            generation: process.generation,
        })
    }

    // @group DatabaseOperations : Snapshot runtime state together with the complete config
    pub async fn snapshot(&self) -> Vec<ManagedProcessSnapshot> {
        let mut result = Vec::new();
        for entry in self.registry.iter() {
            let proc = entry.value().read().await;
            result.push(ManagedProcessSnapshot {
                info: proc.to_info(),
                config: proc.config.clone(),
                process_identity: proc.process_identity.clone(),
                desired_running: proc.desired_running,
                generation: proc.generation,
            });
        }
        result.sort_by_key(|process| process.info.created_at);
        result
    }

    /// Restore a previously captured runtime snapshot after a persistence failure.
    /// Sleeping cron jobs are rescheduled without being executed immediately.
    pub async fn restore_snapshot(&self, snapshot: ManagedProcessSnapshot) -> Result<ProcessInfo> {
        let id = snapshot.info.id;
        if let Ok(current) = self.get(id).await {
            if matches!(
                current.status,
                ProcessStatus::Starting
                    | ProcessStatus::Stopping
                    | ProcessStatus::Running
                    | ProcessStatus::Watching
                    | ProcessStatus::Sleeping
            ) || current.pid.is_some()
            {
                self.stop(id).await?;
            }
            if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&id) {
                scheduler.abort();
            }
            self.registry.remove(&id);
            self.metrics_history.remove(&id);
        }

        let status = snapshot.info.status.clone();
        let restart_count = snapshot.info.restart_count;
        let history = snapshot.info.cron_run_history.clone();
        let config = snapshot.config.clone();
        let should_run = snapshot.desired_running
            && (snapshot.info.pid.is_some()
                || matches!(
                    status,
                    ProcessStatus::Starting
                        | ProcessStatus::Stopped
                        | ProcessStatus::Running
                        | ProcessStatus::Watching
                ));
        match status.clone() {
            ProcessStatus::Sleeping if snapshot.desired_running => {
                self.register_sleeping(id, config, restart_count, history)
                    .await?;
            }
            _ if should_run => {
                self.register_stopped_restored(id, config, restart_count, history)
                    .await;
                self.get_arc(id)?.write().await.generation = snapshot.generation.wrapping_add(1);
                self.start_existing(id).await?;
            }
            _ => {
                self.register_stopped_restored(id, config, restart_count, history)
                    .await;
                let arc = self.get_arc(id)?;
                let mut process = arc.write().await;
                process.status = if matches!(
                    status,
                    ProcessStatus::Starting
                        | ProcessStatus::Stopping
                        | ProcessStatus::Running
                        | ProcessStatus::Watching
                        | ProcessStatus::Sleeping
                ) {
                    ProcessStatus::Stopped
                } else {
                    status
                };
                process.desired_running = false;
                process.last_exit_code = snapshot.info.last_exit_code;
                process.started_at = snapshot.info.started_at;
                process.stopped_at = snapshot.info.stopped_at;
                process.cron_next_run = snapshot.info.cron_next_run;
            }
        }
        let arc = self.get_arc(id)?;
        let mut process = arc.write().await;
        process.created_at = snapshot.info.created_at;
        process.generation = process.generation.max(snapshot.generation.wrapping_add(1));
        drop(process);
        self.get(id).await
    }

    /// Undo an enabled-flag mutation without restarting ordinary processes.
    /// Cron enablement owns scheduler/runtime state, so it requires the full snapshot path.
    pub async fn restore_enabled_snapshot(
        &self,
        snapshot: ManagedProcessSnapshot,
    ) -> Result<ProcessInfo> {
        if snapshot.config.cron.is_some() || snapshot.info.pid.is_none() {
            self.restore_snapshot(snapshot).await
        } else {
            self.set_enabled(snapshot.info.id, snapshot.info.enabled)
                .await
        }
    }

    // @group BusinessLogic > LogStats : Return bucketed stdout/stderr log counts for a process
    pub async fn get_log_stats(&self, id: Uuid) -> Vec<crate::models::log_stats::LogStatsBucket> {
        match self.registry.get(&id) {
            Some(arc) => {
                // Clone the Arc out before dropping the read guard to avoid lifetime issues
                let stats_arc = {
                    let proc = arc.read().await;
                    Arc::clone(&proc.log_stats)
                };
                let snapshot = stats_arc.lock().await.snapshot();
                snapshot
            }
            None => Vec::new(),
        }
    }

    // @group BusinessLogic > Metrics : Return a snapshot of all recorded samples for a process
    pub async fn get_metrics_history(&self, id: Uuid) -> Vec<MetricSample> {
        match self.metrics_history.get(&id) {
            Some(entry) => entry.lock().await.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    // @group BusinessLogic > Query : Get a single process info by id
    pub async fn get(&self, id: Uuid) -> Result<ProcessInfo> {
        let arc = self.get_arc(id)?;
        let proc = arc.read().await;
        Ok(proc.to_info())
    }

    pub async fn clear_logs(&self, id: Uuid) -> Result<()> {
        let arc = self.get_arc(id)?;
        let process = arc.read().await;
        let log_dir = crate::config::paths::process_log_dir(&process.config.name);
        if let Some(writer) = process.log_writer.as_ref() {
            let clear = writer.clear();
            drop(process);
            clear.await
        } else {
            drop(process);
            LogWriter::clear_inactive(log_dir).await
        }
    }

    // @group BusinessLogic > Query : Subscribe to a process's log broadcast channel
    pub async fn subscribe_logs(&self, id: Uuid) -> Result<broadcast::Receiver<LogLine>> {
        let arc = self.get_arc(id)?;
        let proc = arc.read().await;
        Ok(proc.log_tx.subscribe())
    }

    // @group BusinessLogic > Utilities : Resolve process ID from name or UUID string
    pub async fn resolve_id(&self, name_or_id: &str) -> Result<Uuid> {
        if let Ok(id) = name_or_id.parse::<Uuid>() {
            if self.registry.contains_key(&id) {
                return Ok(id);
            }
        }
        let mut matched = None;
        for entry in self.registry.iter() {
            let proc = entry.value().read().await;
            if proc.config.name == name_or_id {
                if matched.is_some() {
                    return Err(anyhow!(
                        "multiple processes are named '{name_or_id}'; use the process UUID"
                    ));
                }
                matched = Some(proc.id);
            }
        }
        matched.ok_or_else(|| anyhow!("no process found with name or id: {name_or_id}"))
    }

    // @group BusinessLogic > Internal : Core spawn logic shared by start/restart
    async fn do_spawn(&self, id: Uuid) -> Result<()> {
        self.do_spawn_with_event(id, ProcessEvent::Started, None)
            .await
    }

    async fn do_spawn_with_event(
        &self,
        id: Uuid,
        event: ProcessEvent,
        expected_generation: Option<u64>,
    ) -> Result<()> {
        // Hook execution and every external probe have their own owned-child
        // timeout. Do not cancel the aggregate spawn future: cancellation after
        // OS spawn would lose the Child handle before cleanup is confirmed.
        self.do_spawn_with_event_inner(id, event, expected_generation)
            .await
    }

    async fn do_spawn_with_event_inner(
        &self,
        id: Uuid,
        event: ProcessEvent,
        expected_generation: Option<u64>,
    ) -> Result<()> {
        let arc = self.get_arc(id)?;

        {
            let proc = arc.read().await;
            if !proc.config.enabled {
                return Err(anyhow!("process '{}' is disabled", proc.config.name));
            }
            if expected_generation.is_none()
                && (proc.pid.is_some()
                    || matches!(
                        proc.status,
                        ProcessStatus::Starting
                            | ProcessStatus::Stopping
                            | ProcessStatus::Running
                            | ProcessStatus::Watching
                            | ProcessStatus::Sleeping
                    ))
            {
                return Err(anyhow!(
                    "process '{}' is already active or still cleaning up",
                    proc.config.name
                ));
            }
            if let Some(expr) = &proc.config.cron {
                if next_run(expr).is_none() {
                    return Err(anyhow!("invalid cron expression '{}'", expr));
                }
            }
            if proc.config.watch && proc.config.watch_paths.is_empty() {
                return Err(anyhow!("watch mode requires at least one watch path"));
            }
        }

        let (config, log_tx, log_stats, generation) = {
            let mut proc = arc.write().await;
            if !proc.config.enabled {
                return Err(anyhow!("process '{}' is disabled", proc.config.name));
            }
            if let Some(expected_generation) = expected_generation {
                if !proc.desired_running || proc.generation != expected_generation {
                    return Err(anyhow!(
                        "process restart was cancelled by a newer lifecycle request"
                    ));
                }
            }

            // Open / rotate log files
            let log_dir = crate::config::paths::process_log_dir(&proc.config.name);
            std::fs::create_dir_all(&log_dir)?;
            let writer =
                LogWriter::new(&log_dir, proc.log_tx.clone(), proc.config.max_log_size_mb)?;

            if expected_generation.is_none() {
                proc.desired_running = true;
            }
            proc.generation = proc.generation.wrapping_add(1);
            let generation = proc.generation;
            proc.status = ProcessStatus::Starting;
            proc.started_at = Some(Utc::now());
            proc.stopped_at = None;
            proc.cron_next_run = None;
            // Reset health status on each (re)spawn
            proc.health_status = None;
            proc.log_writer = Some(writer);

            (
                proc.config.clone(),
                proc.log_tx.clone(),
                Arc::clone(&proc.log_stats),
                generation,
            )
        };

        // @group BusinessLogic > Hooks : Run pre_start hook before spawning
        if let Some(cmd) = &config.pre_start {
            if let Err(e) =
                crate::process::hooks::run_hook(cmd, config.cwd.as_deref(), &config.env).await
            {
                if Self::mark_errored_if_current(&arc, generation).await {
                    let _ = self.persistence_tx.send(());
                }
                return Err(anyhow::anyhow!("pre_start hook failed: {e}"));
            }
        }

        // @group BusinessLogic > EnvFile : Merge .env file vars with explicit env (explicit wins)
        let merged_env = match crate::config::env_file::merge_env(
            config.env_file.as_deref(),
            config.cwd.as_deref(),
            &config.env,
        ) {
            Ok(env) => env,
            Err(error) => {
                if Self::mark_errored_if_current(&arc, generation).await {
                    let _ = self.persistence_tx.send(());
                }
                return Err(anyhow!("failed to load process environment: {error}"));
            }
        };

        let (exit_tx, exit_rx) = mpsc::channel::<crate::process::runner::RunResult>(1);

        let mut child = match spawn_process(
            id,
            &config.script,
            &config.args,
            config.cwd.as_deref(),
            &merged_env,
            log_tx,
            log_stats,
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                if Self::mark_errored_if_current(&arc, generation).await {
                    let _ = self.persistence_tx.send(());
                }
                return Err(e);
            }
        };

        let Some(pid) = child.id() else {
            let _ = child.wait().await;
            if Self::mark_errored_if_current(&arc, generation).await {
                let _ = self.persistence_tx.send(());
            }
            return Err(anyhow!("spawned process did not expose a PID"));
        };
        let identity = match capture_process_identity_with_retry(pid).await {
            Some(identity) => identity,
            None => {
                // This PID was obtained from the child we just spawned, so it is
                // safe to clean it up without a persisted identity. Never expose
                // an unidentifiable process as successfully managed.
                let cleanup_error = self
                    .cleanup_failed_spawn(Arc::clone(&arc), child, pid, None, generation)
                    .await;
                return Err(match cleanup_error {
                    None => anyhow!(
                        "spawned PID {pid}, but could not capture a verifiable process identity"
                    ),
                    Some(error) => anyhow!(
                        "spawned PID {pid}, but could not capture its identity or clean it up: {error}"
                    ),
                });
            }
        };

        if let Err(error) = child.preserve_process_tree() {
            let cleanup_error = self
                .cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                .await;
            return Err(match cleanup_error {
                Some(cleanup_error) => anyhow!(
                    "failed to preserve managed process tree ({error}); cleanup also failed: {cleanup_error}"
                ),
                None => anyhow!("failed to preserve managed process tree: {error}"),
            });
        }

        {
            let mut proc = arc.write().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                drop(proc);
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!("process start was cancelled by a stop request"));
            }
            proc.pid = Some(pid);
            proc.process_identity = Some(identity.clone());
        }

        // For cron jobs we force autorestart=false so watch_and_restart just fires Exited.
        // The cron_trigger_loop handles re-spawning on the next tick.
        let effective_autorestart = if config.cron.is_some() {
            false
        } else {
            config.autorestart
        };

        let restart_count = {
            let proc = arc.read().await;
            proc.restart_count
        };

        // @group BusinessLogic > HealthCheck : Start health probe loop if configured
        if let Some(url) = &config.health_check_url {
            let handle = crate::process::health::start_health_check(
                Arc::clone(&arc),
                generation,
                url.clone(),
                config.health_check_interval_secs,
                config.health_check_timeout_secs,
                config.health_check_retries,
                Arc::clone(&self.notifications),
            );
            let mut proc = arc.write().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                handle.abort();
                drop(proc);
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!(
                    "process start was cancelled while attaching health checks"
                ));
            }
            proc.health_check_handle = Some(handle);
        }

        // Start file watcher if watch mode is enabled
        if config.watch && !config.watch_paths.is_empty() {
            let watch_restart_tx = {
                let (tx, mut rx) = mpsc::channel::<Uuid>(8);
                let manager_rtx = self.restart_tx.clone();
                let manager_registry = Arc::clone(&self.registry);
                tokio::spawn(async move {
                    while let Some(pid_id) = rx.recv().await {
                        let Some(entry) = manager_registry.get(&pid_id) else {
                            continue;
                        };
                        let proc = entry.read().await;
                        if !proc.desired_running
                            || !matches!(
                                proc.status,
                                ProcessStatus::Running | ProcessStatus::Watching
                            )
                        {
                            continue;
                        }
                        let current_generation = proc.generation;
                        drop(proc);
                        let _ = manager_rtx
                            .send(RestartEvent::Restart {
                                process_id: pid_id,
                                generation: current_generation,
                            })
                            .await;
                    }
                });
                tx
            };

            let watcher = match FileWatcher::start(
                id,
                &config.watch_paths,
                &config.watch_ignore,
                watch_restart_tx,
            ) {
                Ok(watcher) => watcher,
                Err(error) => {
                    let cleanup_error = self
                        .cleanup_failed_spawn(
                            Arc::clone(&arc),
                            child,
                            pid,
                            Some(identity),
                            generation,
                        )
                        .await;
                    return Err(match cleanup_error {
                        Some(cleanup_error) => anyhow!(
                            "failed to start file watcher ({error}); process cleanup also failed: {cleanup_error}"
                        ),
                        None => anyhow!("failed to start file watcher: {error}"),
                    });
                }
            };
            let mut proc = arc.write().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                drop(watcher);
                drop(proc);
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!(
                    "process start was cancelled while attaching file watcher"
                ));
            }
            proc.file_watcher = Some(watcher);
        }

        // @group BusinessLogic > Cron : Start (or replace) the cron scheduler for this process
        if let Some(expr) = &config.cron {
            // Remove old scheduler if we're restarting
            if let Some(old) = self.cron_schedulers.lock().await.remove(&id) {
                old.abort();
            }
            let scheduler = match CronScheduler::start(id, expr, self.cron_trigger_tx.clone()) {
                Ok(scheduler) => scheduler,
                Err(error) => {
                    let cleanup_error = self
                        .cleanup_failed_spawn(
                            Arc::clone(&arc),
                            child,
                            pid,
                            Some(identity),
                            generation,
                        )
                        .await;
                    return Err(match cleanup_error {
                        Some(cleanup_error) => anyhow!(
                            "failed to start cron scheduler ({error}); process cleanup also failed: {cleanup_error}"
                        ),
                        None => error,
                    });
                }
            };
            {
                let proc = arc.read().await;
                if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                    drop(proc);
                    scheduler.abort();
                    self.cleanup_failed_spawn(
                        Arc::clone(&arc),
                        child,
                        pid,
                        Some(identity),
                        generation,
                    )
                    .await;
                    return Err(anyhow!(
                        "process start was cancelled while attaching cron scheduler"
                    ));
                }
            }
            self.cron_schedulers.lock().await.insert(id, scheduler);
        }

        // Only expose lifecycle success after every required resource exists.
        {
            let proc = arc.read().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                drop(proc);
                if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&id) {
                    scheduler.abort();
                }
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!(
                    "process start was cancelled before lifecycle commit completed"
                ));
            }
        }

        let info_for_notif = {
            let mut proc = arc.write().await;
            if !proc.config.enabled || proc.generation != generation || !proc.desired_running {
                drop(proc);
                if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&id) {
                    scheduler.abort();
                }
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!(
                    "process start was cancelled during lifecycle commit"
                ));
            }
            // Cron jobs are scheduler-driven; watch and normal processes retain
            // their externally visible running mode only after setup commits.
            proc.status = if config.watch {
                ProcessStatus::Watching
            } else {
                ProcessStatus::Running
            };
            proc.process_tree = child.take_process_tree();
            if proc.process_tree.is_none() {
                drop(proc);
                if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&id) {
                    scheduler.abort();
                }
                self.cleanup_failed_spawn(Arc::clone(&arc), child, pid, Some(identity), generation)
                    .await;
                return Err(anyhow!(
                    "process-tree ownership was lost before lifecycle commit"
                ));
            }
            proc.to_info()
        };

        let notif = Arc::clone(&self.notifications);
        let suppress = Arc::clone(&self.bulk_suppress);
        let info_for_tg = info_for_notif.clone();
        tokio::spawn(async move {
            if !Self::suppress_consume(&suppress, &info_for_tg.id) {
                let store = notif.read().await;
                fire_event(&store, &info_for_notif, event).await;
                crate::telegram::commands::fire_telegram_notification(&info_for_tg, event).await;
            }
        });

        if let Some(cmd) = config.post_start.clone() {
            let cwd = config.cwd.clone();
            let env = config.env.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::process::hooks::run_hook(&cmd, cwd.as_deref(), &env).await {
                    tracing::warn!("post_start hook failed: {e}");
                }
            });
        }

        tokio::spawn(async move {
            wait_for_exit(child, exit_tx).await;
        });
        let rtx = self.restart_tx.clone();
        tokio::spawn(watch_and_restart(
            id,
            generation,
            RestartPolicy {
                autorestart: effective_autorestart,
                max_restarts: config.max_restarts,
                restart_delay_ms: config.restart_delay_ms,
                restart_count,
            },
            exit_rx,
            rtx,
        ));

        Ok(())
    }

    /// Fail a partially completed spawn without losing ownership of the child.
    /// If the immediate kill fails, a background task keeps the Child handle and
    /// retries until the process exits, while the registry retains its PID.
    async fn cleanup_failed_spawn(
        &self,
        arc: Arc<RwLock<ManagedProcess>>,
        mut child: ManagedChild,
        pid: u32,
        identity: Option<ProcessIdentity>,
        generation: u64,
    ) -> Option<String> {
        let cleanup_error = child
            .terminate_process_tree()
            .await
            .err()
            .map(|error| error.to_string());

        let retained_generation = {
            let mut proc = arc.write().await;
            if proc.generation == generation {
                proc.status = ProcessStatus::Errored;
                proc.desired_running = false;
                proc.generation = proc.generation.wrapping_add(1);
                proc.pid = cleanup_error.as_ref().map(|_| pid);
                proc.process_identity = if cleanup_error.is_some() {
                    identity.clone()
                } else {
                    None
                };
                if let Some(handle) = proc.health_check_handle.take() {
                    handle.abort();
                }
                proc.file_watcher = None;
                proc.log_writer = None;
                Some(proc.generation)
            } else {
                None
            }
        };
        let _ = self.persistence_tx.send(());

        if cleanup_error.is_some() {
            let cleanup_arc = Arc::clone(&arc);
            let persistence_tx = self.persistence_tx.clone();
            tokio::spawn(async move {
                let mut attempt = 0u32;
                let mut retained_identity = identity;
                loop {
                    attempt = attempt.saturating_add(1);
                    match child.terminate_process_tree().await {
                        Ok(()) => {
                            break;
                        }
                        Err(error) => {
                            if attempt <= 30 || attempt.is_multiple_of(10) {
                                tracing::error!(
                                    attempt,
                                    "failed to clean up partially spawned PID {pid}; retaining the child handle and process-tree guard: {error}"
                                );
                            }
                            if retained_identity.is_none() {
                                if let Some(captured) =
                                    capture_process_identity_with_retry(pid).await
                                {
                                    let mut proc = cleanup_arc.write().await;
                                    let generation_matches = retained_generation
                                        .is_some_and(|retained| proc.generation == retained);
                                    if generation_matches && proc.pid == Some(pid) {
                                        proc.process_identity = Some(captured.clone());
                                        retained_identity = Some(captured);
                                        let _ = persistence_tx.send(());
                                    }
                                }
                            }
                            let delay_secs = u64::from(attempt.min(30));
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                        }
                    }
                }
                let _ = child.wait().await;
                let mut proc = cleanup_arc.write().await;
                let generation_matches =
                    retained_generation.is_some_and(|retained| proc.generation == retained);
                if generation_matches && proc.pid == Some(pid) {
                    proc.pid = None;
                    proc.process_identity = None;
                    proc.status = ProcessStatus::Stopped;
                    proc.stopped_at = Some(Utc::now());
                    drop(proc);
                    let _ = persistence_tx.send(());
                }
            });
        }

        cleanup_error
    }

    // @group BusinessLogic > Internal : Background restart event loop
    async fn restart_loop(manager: Self, mut rx: mpsc::Receiver<RestartEvent>) {
        while let Some(event) = rx.recv().await {
            match event {
                RestartEvent::Restart {
                    process_id,
                    generation,
                } => {
                    let Some(entry) = manager.registry.get(&process_id) else {
                        continue;
                    };
                    let arc = Arc::clone(entry.value());
                    drop(entry);
                    let restart_manager = manager.clone();
                    let Ok(lifecycle_permit) =
                        Arc::clone(&manager.lifecycle_limit).acquire_owned().await
                    else {
                        return;
                    };

                    tokio::spawn(async move {
                        let _lifecycle_permit = lifecycle_permit;
                        let previous_status = {
                            let mut proc = arc.write().await;
                            if !proc.config.enabled
                                || !proc.desired_running
                                || proc.generation != generation
                                || matches!(proc.status, ProcessStatus::Stopping)
                            {
                                return;
                            }
                            let previous_status = proc.status.clone();
                            proc.status = ProcessStatus::Stopping;
                            previous_status
                        };

                        if let Err(error) =
                            Self::terminate_retained_process_tree(&arc, generation).await
                        {
                            tracing::warn!(
                                "failed to stop the complete owned process tree for {process_id} before restart: {error}"
                            );
                            let mut proc = arc.write().await;
                            if proc.generation == generation {
                                // The old process is still alive and owned. Restore its
                                // operable state and resources so Stop/Delete can retry.
                                proc.status = previous_status;
                                let _ = restart_manager.persistence_tx.send(());
                            }
                            return;
                        }

                        {
                            let mut proc = arc.write().await;
                            if !proc.config.enabled
                                || !proc.desired_running
                                || proc.generation != generation
                            {
                                if !proc.config.enabled && proc.generation == generation {
                                    proc.status = ProcessStatus::Stopped;
                                    proc.desired_running = false;
                                    proc.pid = None;
                                    proc.process_identity = None;
                                    proc.process_tree.take();
                                    proc.stopped_at = Some(Utc::now());
                                    let _ = restart_manager.persistence_tx.send(());
                                }
                                return;
                            }
                            if let Some(handle) = proc.health_check_handle.take() {
                                handle.abort();
                            }
                            proc.file_watcher = None;
                            proc.log_writer = None;
                            proc.pid = None;
                            proc.process_identity = None;
                            proc.process_tree.take();
                            proc.restart_count += 1;
                        }

                        if let Err(error) = restart_manager
                            .do_spawn_with_event(
                                process_id,
                                ProcessEvent::Restarted,
                                Some(generation),
                            )
                            .await
                        {
                            if let Some(entry) = restart_manager.registry.get(&process_id) {
                                let info_for_notif = {
                                    let mut proc = entry.write().await;
                                    if !Self::mark_respawn_failed(&mut proc, generation) {
                                        tracing::debug!(
                                            "ignored stale or cancelled respawn for process {process_id}: {error}"
                                        );
                                        return;
                                    }
                                    proc.to_info()
                                };
                                tracing::error!("failed to respawn process {process_id}: {error}");
                                let notifications = Arc::clone(&restart_manager.notifications);
                                tokio::spawn(async move {
                                    let store = notifications.read().await;
                                    fire_event(&store, &info_for_notif, ProcessEvent::Crashed)
                                        .await;
                                    crate::telegram::commands::fire_telegram_notification(
                                        &info_for_notif,
                                        ProcessEvent::Crashed,
                                    )
                                    .await;
                                });
                            }
                        }
                        let _ = restart_manager.persistence_tx.send(());
                    });
                }

                RestartEvent::Exited {
                    process_id,
                    generation,
                    exit_code,
                } => {
                    let mut changed = false;
                    let arc = manager
                        .registry
                        .get(&process_id)
                        .map(|entry| Arc::clone(entry.value()));
                    if let Some(arc) = arc {
                        let is_current = {
                            let proc = arc.read().await;
                            proc.desired_running && proc.generation == generation
                        };
                        if !is_current {
                            continue;
                        }
                        if let Err(error) =
                            Self::terminate_retained_process_tree(&arc, generation).await
                        {
                            tracing::error!(%error, %process_id, "could not confirm exited process descendants were terminated");
                            let mut proc = arc.write().await;
                            if proc.generation == generation {
                                proc.status = ProcessStatus::Errored;
                                proc.desired_running = false;
                                changed = true;
                            }
                            drop(proc);
                            if changed {
                                let _ = manager.persistence_tx.send(());
                            }
                            continue;
                        }
                        let mut proc = arc.write().await;
                        if !proc.desired_running || proc.generation != generation {
                            continue;
                        }
                        proc.status = if proc.config.cron.is_some() {
                            ProcessStatus::Sleeping
                        } else {
                            ProcessStatus::Stopped
                        };
                        proc.pid = None;
                        proc.process_identity = None;
                        proc.process_tree.take();
                        proc.last_exit_code = exit_code;
                        proc.stopped_at = Some(Utc::now());
                        if let Some(handle) = proc.health_check_handle.take() {
                            handle.abort();
                        }
                        proc.file_watcher = None;
                        proc.log_writer = None;
                        if let Some(expr) = &proc.config.cron.clone() {
                            proc.cron_next_run = next_run(expr);
                        }
                        changed = true;
                    }
                    if changed {
                        let _ = manager.persistence_tx.send(());
                    }
                }

                RestartEvent::MaxRestartsReached {
                    process_id,
                    generation,
                    restart_count,
                    exit_code,
                } => {
                    let mut changed = false;
                    let arc = manager
                        .registry
                        .get(&process_id)
                        .map(|entry| Arc::clone(entry.value()));
                    if let Some(arc) = arc {
                        let is_current = {
                            let proc = arc.read().await;
                            proc.desired_running
                                && proc.generation == generation
                                && proc.restart_count == restart_count
                        };
                        if !is_current {
                            continue;
                        }
                        if let Err(error) =
                            Self::terminate_retained_process_tree(&arc, generation).await
                        {
                            tracing::error!(%error, %process_id, "could not confirm max-restart process descendants were terminated");
                            let mut proc = arc.write().await;
                            if proc.generation == generation {
                                proc.status = ProcessStatus::Errored;
                                proc.desired_running = false;
                                changed = true;
                            }
                            drop(proc);
                            if changed {
                                let _ = manager.persistence_tx.send(());
                            }
                            continue;
                        }
                        let info_for_notif = {
                            let mut proc = arc.write().await;
                            if !proc.desired_running
                                || proc.generation != generation
                                || proc.restart_count != restart_count
                            {
                                continue;
                            }
                            proc.status = ProcessStatus::Errored;
                            proc.desired_running = false;
                            proc.pid = None;
                            proc.process_identity = None;
                            proc.process_tree.take();
                            proc.last_exit_code = exit_code;
                            proc.stopped_at = Some(Utc::now());
                            if let Some(handle) = proc.health_check_handle.take() {
                                handle.abort();
                            }
                            proc.file_watcher = None;
                            proc.log_writer = None;
                            tracing::warn!(
                                "process '{}' reached max restarts ({})",
                                proc.config.name,
                                proc.config.max_restarts
                            );
                            proc.to_info()
                        };
                        let notifications = Arc::clone(&manager.notifications);
                        tokio::spawn(async move {
                            let store = notifications.read().await;
                            fire_event(&store, &info_for_notif, ProcessEvent::Crashed).await;
                            crate::telegram::commands::fire_telegram_notification(
                                &info_for_notif,
                                ProcessEvent::Crashed,
                            )
                            .await;
                        });
                        changed = true;
                    }
                    if changed {
                        let _ = manager.persistence_tx.send(());
                    }
                }
            }
        }
    }

    async fn mark_errored_if_current(
        process: &Arc<RwLock<ManagedProcess>>,
        generation: u64,
    ) -> bool {
        let mut process = process.write().await;
        if process.generation != generation || !process.desired_running {
            return false;
        }
        process.status = ProcessStatus::Errored;
        process.desired_running = false;
        process.generation = process.generation.wrapping_add(1);
        process.pid = None;
        process.process_identity = None;
        process.process_tree.take();
        if let Some(handle) = process.health_check_handle.take() {
            handle.abort();
        }
        process.file_watcher = None;
        process.log_writer = None;
        true
    }

    fn mark_respawn_failed(process: &mut ManagedProcess, previous_generation: u64) -> bool {
        let failed_generation = previous_generation.wrapping_add(1);
        if !process.desired_running
            || (process.generation != previous_generation
                && process.generation != failed_generation)
        {
            return false;
        }
        process.generation = failed_generation;
        process.status = ProcessStatus::Errored;
        process.desired_running = false;
        process.pid = None;
        process.process_identity = None;
        process.process_tree.take();
        true
    }

    /// Complete one Cron run only after the complete retained process tree is
    /// confirmed gone. Cleanup failure is terminal for the scheduler and keeps
    /// ownership metadata available for a later explicit Stop/Delete retry.
    async fn finish_cron_run(
        &self,
        process_id: Uuid,
        process: &Arc<RwLock<ManagedProcess>>,
        generation: u64,
        run: CronRun,
    ) -> Result<Option<ProcessInfo>> {
        if let Err(error) = Self::terminate_retained_process_tree(process, generation).await {
            let changed = {
                let mut process = process.write().await;
                if process.generation != generation || !process.desired_running {
                    false
                } else {
                    process.status = ProcessStatus::Errored;
                    process.desired_running = false;
                    process.last_exit_code = run.exit_code;
                    process.stopped_at = Some(Utc::now());
                    if let Some(handle) = process.health_check_handle.take() {
                        handle.abort();
                    }
                    process.file_watcher = None;
                    process.log_writer = None;
                    process.cron_next_run = None;
                    process.cron_run_history.push(run);
                    if process.cron_run_history.len() > MAX_CRON_HISTORY {
                        process.cron_run_history.remove(0);
                    }
                    true
                }
            };
            if changed {
                if let Some(scheduler) = self.cron_schedulers.lock().await.remove(&process_id) {
                    scheduler.abort();
                }
                let _ = self.persistence_tx.send(());
            }
            return Err(anyhow!(
                "cron run exited but its retained process tree could not be confirmed terminated: {error}"
            ));
        }

        let info_for_failure = {
            let mut process = process.write().await;
            if process.generation != generation || !process.desired_running {
                return Ok(None);
            }
            process.status = ProcessStatus::Sleeping;
            process.cron_next_run = process.config.cron.as_deref().and_then(next_run);
            process.pid = None;
            process.process_identity = None;
            process.process_tree.take();
            if let Some(handle) = process.health_check_handle.take() {
                handle.abort();
            }
            process.file_watcher = None;
            process.log_writer = None;
            process.last_exit_code = run.exit_code;
            process.stopped_at = Some(Utc::now());
            process.cron_run_history.push(run);
            if process.cron_run_history.len() > MAX_CRON_HISTORY {
                process.cron_run_history.remove(0);
            }
            process
                .last_exit_code
                .filter(|code| *code != 0)
                .map(|_| process.to_info())
        };
        let _ = self.persistence_tx.send(());
        Ok(info_for_failure)
    }

    // @group BusinessLogic > Cron : Background loop that re-spawns cron processes on each tick
    async fn cron_trigger_loop(
        manager: Self,
        registry: Arc<ProcessRegistry>,
        mut rx: mpsc::Receiver<Uuid>,
        cron_schedulers: Arc<Mutex<HashMap<Uuid, CronScheduler>>>,
        cron_trigger_tx: mpsc::Sender<Uuid>,
        notifications: Arc<RwLock<NotificationsStore>>,
        persistence_tx: broadcast::Sender<()>,
    ) {
        while let Some(process_id) = rx.recv().await {
            if let Some(arc) = registry.get(&process_id) {
                let arc = Arc::clone(arc.value());
                let cron_schedulers = Arc::clone(&cron_schedulers);
                let trigger_tx = cron_trigger_tx.clone();
                let persistence_tx = persistence_tx.clone();
                let manager = manager.clone();

                let notif_cron = Arc::clone(&notifications);
                tokio::spawn(async move {
                    let config = {
                        let proc = arc.read().await;
                        // Only fire if still in Sleeping state (not manually stopped)
                        if !proc.config.enabled
                            || proc.status != ProcessStatus::Sleeping
                            || !proc.desired_running
                        {
                            return;
                        }
                        proc.config.clone()
                    };

                    // Capture start time before spawning for duration calculation
                    let run_started_at = Utc::now();

                    // Transition to Starting
                    let generation = {
                        let mut proc = arc.write().await;
                        if !proc.config.enabled
                            || !proc.desired_running
                            || proc.status != ProcessStatus::Sleeping
                        {
                            return;
                        }
                        proc.generation = proc.generation.wrapping_add(1);
                        let generation = proc.generation;
                        proc.status = ProcessStatus::Starting;
                        proc.started_at = Some(run_started_at);
                        proc.stopped_at = None;
                        proc.cron_next_run = None;
                        generation
                    };

                    let log_dir = crate::config::paths::process_log_dir(&config.name);
                    if let Err(error) = std::fs::create_dir_all(&log_dir) {
                        tracing::error!("cron: failed to prepare logs for {process_id}: {error}");
                        if Self::mark_errored_if_current(&arc, generation).await {
                            if let Some(scheduler) =
                                cron_schedulers.lock().await.remove(&process_id)
                            {
                                scheduler.abort();
                            }
                            let _ = persistence_tx.send(());
                        }
                        return;
                    }

                    let (log_tx, log_stats) = {
                        let mut proc = arc.write().await;
                        if !proc.config.enabled
                            || proc.generation != generation
                            || !proc.desired_running
                        {
                            return;
                        }
                        let writer = match LogWriter::new(
                            &log_dir,
                            proc.log_tx.clone(),
                            proc.config.max_log_size_mb,
                        ) {
                            Ok(writer) => writer,
                            Err(error) => {
                                proc.status = ProcessStatus::Errored;
                                proc.desired_running = false;
                                proc.generation = proc.generation.wrapping_add(1);
                                tracing::error!(
                                    "cron: failed to start log writer for {process_id}: {error}"
                                );
                                drop(proc);
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                let _ = persistence_tx.send(());
                                return;
                            }
                        };
                        proc.log_writer = Some(writer);
                        (proc.log_tx.clone(), Arc::clone(&proc.log_stats))
                    };

                    let (exit_tx, exit_rx) = mpsc::channel::<crate::process::runner::RunResult>(1);

                    // We need a local restart_tx to wire up watch_and_restart.
                    // Since cron jobs use autorestart=false, watch_and_restart will just send Exited
                    // which the restart_loop will catch and transition back to Sleeping.
                    // We create a one-shot dummy channel — the Exited event goes to restart_loop.
                    // But we don't have access to restart_tx here, so we use a side channel approach:
                    // Send a RestartEvent::Exited through a local mpsc that immediately updates state.
                    let (local_restart_tx, mut local_restart_rx) =
                        mpsc::channel::<crate::process::restarter::RestartEvent>(4);
                    let arc2 = Arc::clone(&arc);
                    let notif_exit = Arc::clone(&notif_cron);
                    let manager_exit = manager.clone();

                    // Handle the exit event inline — record run history, transition to Sleeping, fire CronFailed if needed
                    tokio::spawn(async move {
                        if let Some(crate::process::restarter::RestartEvent::Exited {
                            generation: event_generation,
                            exit_code,
                            ..
                        }) = local_restart_rx.recv().await
                        {
                            if event_generation != generation {
                                return;
                            }
                            let finished_at = Utc::now();
                            let duration_secs =
                                (finished_at - run_started_at).num_seconds().max(0) as u64;
                            let run = CronRun {
                                run_at: run_started_at,
                                exit_code,
                                duration_secs,
                            };
                            let info_for_fail = match manager_exit
                                .finish_cron_run(process_id, &arc2, generation, run)
                                .await
                            {
                                Ok(info) => info,
                                Err(error) => {
                                    tracing::error!(
                                        %error,
                                        %process_id,
                                        "cron run cleanup failed; scheduler stopped to prevent overlapping descendants"
                                    );
                                    let proc = arc2.read().await;
                                    (proc.generation == generation).then(|| proc.to_info())
                                }
                            };
                            if let Some(info) = info_for_fail {
                                let store = notif_exit.read().await;
                                fire_event(&store, &info, ProcessEvent::CronFailed).await;
                                crate::telegram::commands::fire_telegram_notification(
                                    &info,
                                    ProcessEvent::CronFailed,
                                )
                                .await;
                            }
                        }
                    });

                    if let Some(cmd) = &config.pre_start {
                        if let Err(error) =
                            crate::process::hooks::run_hook(cmd, config.cwd.as_deref(), &config.env)
                                .await
                        {
                            tracing::error!(
                                "cron: pre_start hook failed for {process_id}: {error}"
                            );
                            if Self::mark_errored_if_current(&arc, generation).await {
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                let _ = persistence_tx.send(());
                            }
                            return;
                        }
                    }

                    // @group BusinessLogic > EnvFile : Merge .env for cron spawn
                    let merged_env = match crate::config::env_file::merge_env(
                        config.env_file.as_deref(),
                        config.cwd.as_deref(),
                        &config.env,
                    ) {
                        Ok(env) => env,
                        Err(error) => {
                            tracing::error!(
                                "cron: failed to load environment for {process_id}: {error}"
                            );
                            if Self::mark_errored_if_current(&arc, generation).await {
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                let _ = persistence_tx.send(());
                            }
                            return;
                        }
                    };

                    match spawn_process(
                        process_id,
                        &config.script,
                        &config.args,
                        config.cwd.as_deref(),
                        &merged_env,
                        log_tx,
                        log_stats,
                    )
                    .await
                    {
                        Ok(mut child) => {
                            let Some(pid) = child.id() else {
                                let _ = child.wait().await;
                                tracing::error!(
                                    "cron: spawned process {process_id} did not expose a PID"
                                );
                                if Self::mark_errored_if_current(&arc, generation).await {
                                    if let Some(scheduler) =
                                        cron_schedulers.lock().await.remove(&process_id)
                                    {
                                        scheduler.abort();
                                    }
                                    let _ = persistence_tx.send(());
                                }
                                return;
                            };
                            let Some(identity) = capture_process_identity_with_retry(pid).await
                            else {
                                let cleanup_error = manager
                                    .cleanup_failed_spawn(
                                        Arc::clone(&arc),
                                        child,
                                        pid,
                                        None,
                                        generation,
                                    )
                                    .await;
                                tracing::error!(
                                    "cron: could not capture identity for PID {pid}; cleanup error: {cleanup_error:?}"
                                );
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                return;
                            };
                            if let Err(error) = child.preserve_process_tree() {
                                let cleanup_error = manager
                                    .cleanup_failed_spawn(
                                        Arc::clone(&arc),
                                        child,
                                        pid,
                                        Some(identity),
                                        generation,
                                    )
                                    .await;
                                tracing::error!(
                                    "cron: failed to preserve managed process tree for PID {pid}: {error}; cleanup error: {cleanup_error:?}"
                                );
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                return;
                            }
                            let mut health_check_handle =
                                config.health_check_url.as_ref().map(|url| {
                                    crate::process::health::start_health_check(
                                        Arc::clone(&arc),
                                        generation,
                                        url.clone(),
                                        config.health_check_interval_secs,
                                        config.health_check_timeout_secs,
                                        config.health_check_retries,
                                        Arc::clone(&notif_cron),
                                    )
                                });

                            // Commit PID, tree ownership, health monitoring and
                            // Running state in one registry critical section. Until
                            // this point the ManagedChild retains the guard, so every
                            // cancellation path can perform complete-tree cleanup.
                            let commit_error = {
                                let mut proc = arc.write().await;
                                if proc.generation != generation || !proc.desired_running {
                                    Some("process start was cancelled before cron ownership commit")
                                } else if let Some(process_tree) = child.take_process_tree() {
                                    proc.pid = Some(pid);
                                    proc.process_identity = Some(identity.clone());
                                    proc.process_tree = Some(process_tree);
                                    proc.health_check_handle = health_check_handle.take();
                                    proc.status = ProcessStatus::Running;
                                    None
                                } else {
                                    Some("process-tree ownership was lost before cron ownership commit")
                                }
                            };
                            if let Some(commit_error) = commit_error {
                                if let Some(handle) = health_check_handle.take() {
                                    handle.abort();
                                }
                                let cleanup_error = manager
                                    .cleanup_failed_spawn(
                                        Arc::clone(&arc),
                                        child,
                                        pid,
                                        Some(identity),
                                        generation,
                                    )
                                    .await;
                                tracing::error!(
                                    "cron: {commit_error} for PID {pid}; cleanup error: {cleanup_error:?}"
                                );
                                if let Some(scheduler) =
                                    cron_schedulers.lock().await.remove(&process_id)
                                {
                                    scheduler.abort();
                                }
                                return;
                            }

                            let info_for_notif = {
                                let proc = arc.read().await;
                                proc.to_info()
                            };
                            let _ = persistence_tx.send(());

                            if let Some(cmd) = config.post_start.clone() {
                                let cwd = config.cwd.clone();
                                let env = config.env.clone();
                                tokio::spawn(async move {
                                    if let Err(error) =
                                        crate::process::hooks::run_hook(&cmd, cwd.as_deref(), &env)
                                            .await
                                    {
                                        tracing::warn!(
                                            "cron: post_start hook failed for {process_id}: {error}"
                                        );
                                    }
                                });
                            }
                            // Fire CronRun notification when cron job starts (non-blocking)
                            let notif3 = Arc::clone(&notif_cron);
                            let info_for_tg = info_for_notif.clone();
                            tokio::spawn(async move {
                                let store = notif3.read().await;
                                fire_event(&store, &info_for_notif, ProcessEvent::CronRun).await;
                                crate::telegram::commands::fire_telegram_notification(
                                    &info_for_tg,
                                    ProcessEvent::CronRun,
                                )
                                .await;
                            });

                            let restart_count = { arc.read().await.restart_count };
                            tokio::spawn(async move {
                                wait_for_exit(child, exit_tx).await;
                            });
                            tokio::spawn(watch_and_restart(
                                process_id,
                                generation,
                                RestartPolicy {
                                    autorestart: false,
                                    max_restarts: config.max_restarts,
                                    restart_delay_ms: config.restart_delay_ms,
                                    restart_count,
                                },
                                exit_rx,
                                local_restart_tx,
                            ));

                            // Ensure the scheduler is still alive (it may have been dropped on stop)
                            let should_schedule = {
                                let proc = arc.read().await;
                                proc.desired_running && proc.generation == generation
                            };
                            let has_scheduler =
                                cron_schedulers.lock().await.contains_key(&process_id);
                            if should_schedule && !has_scheduler {
                                if let Some(expr) = &config.cron {
                                    if let Ok(sched) =
                                        CronScheduler::start(process_id, expr, trigger_tx)
                                    {
                                        let mut schedulers = cron_schedulers.lock().await;
                                        let still_current = {
                                            let proc = arc.read().await;
                                            proc.desired_running
                                                && proc.generation == generation
                                                && proc.status != ProcessStatus::Stopped
                                        };
                                        if still_current && !schedulers.contains_key(&process_id) {
                                            schedulers.insert(process_id, sched);
                                        } else {
                                            sched.abort();
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("cron: failed to spawn process {process_id}: {e}");
                            let info_for_notif = {
                                let mut proc = arc.write().await;
                                if proc.generation != generation || !proc.desired_running {
                                    return;
                                }
                                proc.status = ProcessStatus::Errored;
                                proc.desired_running = false;
                                proc.generation = proc.generation.wrapping_add(1);
                                proc.pid = None;
                                proc.process_identity = None;
                                if let Some(handle) = proc.health_check_handle.take() {
                                    handle.abort();
                                }
                                proc.file_watcher = None;
                                proc.log_writer = None;
                                proc.to_info()
                            };
                            if let Some(scheduler) =
                                cron_schedulers.lock().await.remove(&process_id)
                            {
                                scheduler.abort();
                            }
                            let _ = persistence_tx.send(());
                            let notif3 = Arc::clone(&notif_cron);
                            let info_for_tg = info_for_notif.clone();
                            tokio::spawn(async move {
                                let store = notif3.read().await;
                                fire_event(&store, &info_for_notif, ProcessEvent::CronFailed).await;
                                crate::telegram::commands::fire_telegram_notification(
                                    &info_for_tg,
                                    ProcessEvent::CronFailed,
                                )
                                .await;
                            });
                        }
                    }
                });
            }
        }
    }

    // @group BusinessLogic > Metrics : Periodically collects CPU and memory for each running process
    async fn metrics_loop(
        registry: Arc<ProcessRegistry>,
        hist: Arc<DashMap<Uuid, Mutex<VecDeque<MetricSample>>>>,
    ) {
        let mut sys = System::new();
        let mut tick: u32 = 0;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            tick = tick.wrapping_add(1);

            // Collect process IDs that currently have a PID (i.e. are running)
            let pid_map: Vec<(Uuid, Pid, u64, Option<ProcessIdentity>)> = {
                let mut result = Vec::new();
                for entry in registry.iter() {
                    let proc = entry.value().read().await;
                    if let Some(pid) = proc.pid {
                        result.push((
                            *entry.key(),
                            Pid::from_u32(pid),
                            proc.generation,
                            proc.process_identity.clone(),
                        ));
                    }
                }
                result
            };

            if pid_map.is_empty() {
                continue;
            }

            // Refresh ALL processes so we can walk the full process tree.
            // On Windows, non-.exe scripts are wrapped in cmd.exe /C, so proc.pid points to
            // cmd.exe rather than the real child (node.exe, python.exe, etc.). Summing the
            // entire subtree rooted at proc.pid gives accurate CPU + memory figures.
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                false,
                ProcessRefreshKind::new().with_cpu().with_memory(),
            );

            // Build parent -> [children] map for tree traversal
            let mut children_map: HashMap<Pid, Vec<Pid>> = HashMap::new();
            for (pid, process) in sys.processes() {
                if let Some(parent) = process.parent() {
                    children_map.entry(parent).or_default().push(*pid);
                }
            }

            // @group BusinessLogic > Metrics : Decide whether this tick records a history sample
            let record_sample = tick.is_multiple_of(METRIC_SAMPLE_INTERVAL_TICKS);
            let now = Utc::now();

            // Write new metrics back into each process entry
            for (id, sysinfo_pid, expected_generation, expected_identity) in &pid_map {
                if let Some(arc) = registry.get(id) {
                    let mut proc = arc.write().await;
                    let snapshot_is_current = proc.generation == *expected_generation
                        && proc.pid == Some(sysinfo_pid.as_u32())
                        && proc.process_identity.as_ref() == expected_identity.as_ref();
                    if !snapshot_is_current {
                        continue;
                    }
                    let identity_matches = sys.process(*sysinfo_pid).is_some_and(|process| {
                        expected_identity.as_ref().is_some_and(|expected| {
                            stable_identity_matches(
                                &ProcessIdentity {
                                    executable: process
                                        .exe()
                                        .map(|path| path.to_string_lossy().into_owned()),
                                    command_line: Vec::new(),
                                    cwd: None,
                                    start_time_secs: process.start_time(),
                                },
                                expected,
                            )
                        })
                    });
                    if identity_matches {
                        // Sum CPU and memory across the entire process subtree so that
                        // shell wrappers (cmd.exe) and real child processes are both counted.
                        let (cpu, mem) = sum_process_tree(&sys, &children_map, *sysinfo_pid);
                        proc.cpu_percent = Some(cpu);
                        proc.memory_bytes = Some(mem);

                        // @group BusinessLogic > Metrics : Push sample into the per-process ring buffer
                        if record_sample {
                            let entry = hist.entry(*id).or_insert_with(|| {
                                Mutex::new(VecDeque::with_capacity(MAX_METRIC_SAMPLES + 1))
                            });
                            let mut buf = entry.lock().await;
                            buf.push_back(MetricSample {
                                timestamp: now,
                                cpu_percent: cpu,
                                memory_bytes: mem,
                            });
                            if buf.len() > MAX_METRIC_SAMPLES {
                                buf.pop_front();
                            }
                        }
                    } else {
                        proc.cpu_percent = None;
                        proc.memory_bytes = None;
                    }
                }
            }
        }
    }

    // @group BusinessLogic > LogAlerts : Configurable-interval loop — resolves per-process effective settings
    async fn log_alert_loop(
        registry: Arc<ProcessRegistry>,
        notifications: Arc<RwLock<NotificationsStore>>,
    ) {
        // Per-process cooldown tracker — stores the last time an alert was fired
        let mut last_alerted: HashMap<Uuid, chrono::DateTime<Utc>> = HashMap::new();

        loop {
            // Read interval before sleeping so a setting change takes effect next cycle
            let interval_secs = {
                let s = crate::config::log_alert_config::load_fail_closed();
                (s.global.check_interval_mins as u64).max(1) * 60
            };
            tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;

            // Reload store fresh after sleep so threshold/cooldown/enabled changes apply immediately
            let alert_store = crate::config::log_alert_config::load_fail_closed();
            let now = Utc::now();
            last_alerted.retain(|id, _| registry.contains_key(id));

            for entry in registry.iter() {
                let id = *entry.key();

                // Clone stats + info without holding the RwLock guard across awaits
                let (stats_arc, proc_info) = {
                    let proc = entry.value().read().await;
                    (Arc::clone(&proc.log_stats), proc.to_info())
                };

                // Skip alert if process is not actively running
                use crate::models::process_status::ProcessStatus;
                match proc_info.status {
                    ProcessStatus::Running | ProcessStatus::Watching => {}
                    _ => continue,
                }

                // Resolve effective settings: process override → namespace override → global
                let (enabled, threshold, cooldown_mins) =
                    alert_store.resolve(&proc_info.namespace, proc_info.log_alert.as_ref());

                if !enabled {
                    continue;
                }

                let cooldown_secs = cooldown_mins as i64 * 60;

                // Check per-process cooldown
                if let Some(&last) = last_alerted.get(&id) {
                    if (now - last).num_seconds() < cooldown_secs {
                        continue;
                    }
                }

                let stderr_count = {
                    let stats = stats_arc.lock().await;
                    // Use the most recently completed bucket
                    stats.history.back().map(|b| b.stderr_count).unwrap_or(0)
                };

                if stderr_count < threshold {
                    continue;
                }

                // Threshold exceeded — record cooldown and fire
                last_alerted.insert(id, now);

                let notif = Arc::clone(&notifications);
                let name = proc_info.name.clone();

                tokio::spawn(async move {
                    let store = notif.read().await;
                    crate::notifications::sender::fire_log_alert(
                        &store,
                        &proc_info,
                        stderr_count,
                        threshold,
                    )
                    .await;
                    crate::telegram::commands::fire_log_alert_telegram(
                        &name,
                        stderr_count,
                        threshold,
                    )
                    .await;
                });
            }
        }
    }

    fn get_arc(&self, id: Uuid) -> Result<Arc<RwLock<ManagedProcess>>> {
        self.registry
            .get(&id)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| anyhow!("process not found: {id}"))
    }
}

// @group Utilities > Metrics : Sum CPU% and memory bytes for a process and all its descendants
fn sum_process_tree(sys: &System, children_map: &HashMap<Pid, Vec<Pid>>, root: Pid) -> (f32, u64) {
    let mut total_cpu = 0.0f32;
    let mut total_mem = 0u64;
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if let Some(p) = sys.process(pid) {
            total_cpu += p.cpu_usage();
            total_mem += p.memory();
        }
        if let Some(children) = children_map.get(&pid) {
            stack.extend_from_slice(children);
        }
    }
    (total_cpu, total_mem)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config(name: String) -> AppConfig {
        AppConfig {
            name,
            project_id: None,
            script: "test".to_string(),
            args: vec![],
            cwd: None,
            instances: 1,
            autorestart: true,
            max_restarts: 3,
            restart_delay_ms: 10,
            watch: false,
            watch_paths: vec![],
            watch_ignore: vec![],
            env: HashMap::new(),
            namespace: "tests".to_string(),
            log_file: None,
            error_file: None,
            max_log_size_mb: 1,
            cron: None,
            cron_last_run: None,
            cron_next_run: None,
            notify: None,
            log_alert: None,
            env_file: None,
            health_check_url: None,
            health_check_interval_secs: 30,
            health_check_timeout_secs: 5,
            health_check_retries: 3,
            pre_start: None,
            post_start: None,
            pre_stop: None,
            enabled: true,
        }
    }

    #[tokio::test]
    async fn notification_metadata_update_does_not_restart_process() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("notification-update-{id}")))
            .await;
        let mut notify = NotificationConfig::default();
        notify.events.on_crash = true;

        let updated = manager
            .set_notification_config(id, Some(notify))
            .await
            .unwrap();

        assert_eq!(updated.status, ProcessStatus::Stopped);
        assert_eq!(updated.pid, None);
        assert!(
            manager
                .get_config(id)
                .await
                .unwrap()
                .notify
                .unwrap()
                .events
                .on_crash
        );
    }

    #[tokio::test]
    async fn manual_stop_rejects_stale_autorestart_event() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let name = format!("stop-race-{}", Uuid::new_v4());
        let id = Uuid::new_v4();
        manager.register_stopped(id, test_config(name)).await;
        let generation = {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 7;
            process.generation
        };

        let stopped = manager.stop(id).await.unwrap();
        assert_eq!(stopped.status, ProcessStatus::Stopped);
        manager
            .restart_tx
            .send(RestartEvent::Restart {
                process_id: id,
                generation,
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let info = manager.get(id).await.unwrap();
        assert_eq!(info.status, ProcessStatus::Stopped);
        assert_eq!(info.pid, None);
        let desired_running = manager
            .registry
            .get(&id)
            .unwrap()
            .read()
            .await
            .desired_running;
        assert!(!desired_running);
    }

    #[tokio::test]
    async fn stale_restart_commit_cannot_reenable_a_stopped_process() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("restart-commit-race-{id}")))
            .await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Stopped;
            process.desired_running = false;
            process.generation = 8;
        }

        let error = manager
            .do_spawn_with_event(id, ProcessEvent::Restarted, Some(7))
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("cancelled by a newer lifecycle request"));

        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert_eq!(process.status, ProcessStatus::Stopped);
        assert!(!process.desired_running);
        assert_eq!(process.generation, 8);
        assert_eq!(process.pid, None);
    }

    #[tokio::test]
    async fn background_exit_requests_persistence() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let mut persistence = manager.subscribe_persistence();
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("exit-persist-{id}")))
            .await;
        let generation = {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 3;
            process.generation
        };

        manager
            .restart_tx
            .send(RestartEvent::Exited {
                process_id: id,
                generation,
                exit_code: Some(0),
            })
            .await
            .unwrap();
        tokio::time::timeout(tokio::time::Duration::from_secs(1), persistence.recv())
            .await
            .expect("background exit should emit promptly")
            .expect("persistence channel should remain open");

        let info = manager.get(id).await.unwrap();
        assert_eq!(info.status, ProcessStatus::Stopped);
        assert_eq!(info.last_exit_code, Some(0));
    }

    #[tokio::test]
    async fn manual_stop_cancels_a_restart_already_marked_stopping() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("stop-stopping-race-{id}")))
            .await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Stopping;
            process.desired_running = true;
            process.generation = 9;
        }

        let stopped = manager.stop(id).await.unwrap();
        assert_eq!(stopped.status, ProcessStatus::Stopped);
        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert!(!process.desired_running);
        assert_eq!(process.generation, 10);
    }

    #[tokio::test]
    async fn manual_stop_waits_for_an_in_progress_tree_cleanup() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("stop-cleanup-race-{id}")))
            .await;
        let arc = Arc::clone(manager.registry.get(&id).unwrap().value());
        {
            let mut process = arc.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 5;
            process.pid = Some(std::process::id());
            process.process_tree_cleanup_in_progress = true;
        }
        let cleanup_arc = Arc::clone(&arc);
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let mut process = cleanup_arc.write().await;
            process.process_tree_cleanup_in_progress = false;
            process.pid = None;
            process.process_identity = None;
        });

        let stopped = manager.stop(id).await.unwrap();

        assert_eq!(stopped.status, ProcessStatus::Stopped);
        assert_eq!(stopped.pid, None);
        let process = arc.read().await;
        assert!(!process.desired_running);
        assert!(!process.process_tree_cleanup_in_progress);
    }

    #[tokio::test]
    async fn disabled_config_cancels_a_queued_adopted_restart() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let config = test_config(format!("disable-queued-restart-{id}"));
        manager.register_stopped(id, config.clone()).await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Stopped;
            process.desired_running = true;
            process.generation = 4;
        }

        let mut disabled = config;
        disabled.enabled = false;
        let updated = manager.update(id, disabled).await.unwrap();
        assert_eq!(updated.status, ProcessStatus::Stopped);
        assert!(!updated.enabled);
        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert!(!process.desired_running);
        assert_eq!(process.generation, 5);
    }

    #[tokio::test]
    async fn disabling_a_sleeping_cron_invalidates_queued_triggers() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let mut config = test_config(format!("disable-cron-{id}"));
        config.cron = Some("0 * * * * *".to_string());
        manager.register_stopped(id, config).await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Sleeping;
            process.desired_running = true;
            process.generation = 7;
        }

        let disabled = manager.set_enabled(id, false).await.unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.status, ProcessStatus::Stopped);
        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert!(!process.desired_running);
        assert_eq!(process.generation, 8);
        assert!(process.cron_next_run.is_none());
    }

    #[tokio::test]
    async fn reenabling_a_disabled_cron_restores_sleeping_scheduler() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let mut config = test_config(format!("reenable-cron-{id}"));
        config.cron = Some("0 * * * * *".to_string());
        config.enabled = false;
        manager.register_stopped(id, config).await;

        let enabled = manager.set_enabled(id, true).await.unwrap();

        assert!(enabled.enabled);
        assert_eq!(enabled.status, ProcessStatus::Sleeping);
        assert!(enabled.cron_next_run.is_some());
        assert!(manager.cron_schedulers.lock().await.contains_key(&id));
        manager.stop(id).await.unwrap();
    }

    #[tokio::test]
    async fn failed_running_cron_disable_restores_its_scheduler() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let mut config = test_config(format!("rollback-cron-{id}"));
        let expression = "0 * * * * *".to_string();
        config.cron = Some(expression.clone());
        manager.register_stopped(id, config).await;
        let scheduler =
            CronScheduler::start(id, &expression, manager.cron_trigger_tx.clone()).unwrap();
        manager.cron_schedulers.lock().await.insert(id, scheduler);
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 6;
            process.pid = Some(std::process::id());
            process.process_identity = None;
        }

        let error = manager.set_enabled(id, false).await.unwrap_err();

        assert!(error
            .to_string()
            .contains("no owned process-tree handle or stable identity is available"));
        assert!(manager.cron_schedulers.lock().await.contains_key(&id));
        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert!(process.config.enabled);
        assert_eq!(process.status, ProcessStatus::Running);
        assert!(process.desired_running);
        assert_eq!(process.generation, 6);
        drop(process);
        if let Some(scheduler) = manager.cron_schedulers.lock().await.remove(&id) {
            scheduler.abort();
        };
    }

    #[tokio::test]
    async fn cron_cleanup_failure_stops_scheduler_and_retains_diagnostics() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let mut config = test_config(format!("cron-cleanup-failure-{id}"));
        let expression = "0 * * * * *".to_string();
        config.cron = Some(expression.clone());
        manager.register_stopped(id, config).await;
        let scheduler =
            CronScheduler::start(id, &expression, manager.cron_trigger_tx.clone()).unwrap();
        manager.cron_schedulers.lock().await.insert(id, scheduler);
        let generation = {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 12;
            process.pid = Some(std::process::id());
            process.generation
        };
        let run = CronRun {
            run_at: Utc::now(),
            exit_code: Some(0),
            duration_secs: 1,
        };

        let error = manager
            .finish_cron_run(
                id,
                manager.registry.get(&id).unwrap().value(),
                generation,
                run,
            )
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("retained process tree could not be confirmed terminated"));
        assert!(!manager.cron_schedulers.lock().await.contains_key(&id));
        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert_eq!(process.status, ProcessStatus::Errored);
        assert!(!process.desired_running);
        assert_eq!(process.pid, Some(std::process::id()));
        assert_eq!(process.cron_run_history.len(), 1);
        assert!(process.cron_next_run.is_none());
    }

    #[tokio::test]
    async fn restart_counter_reset_rejects_an_older_max_restart_event() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("reset-race-{id}")))
            .await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Running;
            process.desired_running = true;
            process.generation = 9;
            process.restart_count = 3;
        }

        manager.reset(id).await.unwrap();
        manager
            .restart_tx
            .send(RestartEvent::MaxRestartsReached {
                process_id: id,
                generation: 9,
                restart_count: 3,
                exit_code: Some(1),
            })
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let entry = manager.registry.get(&id).unwrap();
        let process = entry.read().await;
        assert_eq!(process.restart_count, 0);
        assert_eq!(process.status, ProcessStatus::Running);
        assert!(process.desired_running);
    }

    #[tokio::test]
    async fn duplicate_process_names_require_uuid_resolution() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        manager
            .register_stopped(first, test_config("duplicate-name".to_string()))
            .await;
        manager
            .register_stopped(second, test_config("duplicate-name".to_string()))
            .await;

        let error = manager.resolve_id("duplicate-name").await.unwrap_err();

        assert!(error.to_string().contains("use the process UUID"));
        assert_eq!(manager.resolve_id(&first.to_string()).await.unwrap(), first);
    }

    #[tokio::test]
    async fn snapshot_restore_keeps_sleeping_cron_asleep() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let mut config = test_config(format!("restore-cron-{id}"));
        config.cron = Some("0 * * * * *".to_string());
        manager
            .register_sleeping(id, config, 4, Vec::new())
            .await
            .unwrap();
        let snapshot = manager
            .snapshot()
            .await
            .into_iter()
            .find(|snapshot| snapshot.info.id == id)
            .unwrap();
        manager.stop(id).await.unwrap();

        let restored = manager.restore_snapshot(snapshot).await.unwrap();

        assert_eq!(restored.status, ProcessStatus::Sleeping);
        assert_eq!(restored.restart_count, 4);
        assert!(restored.cron_next_run.is_some());
        manager.stop(id).await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_restore_normalizes_transient_state_and_invalidates_stale_events() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("restore-transient-{id}")))
            .await;
        let mut snapshot = manager.snapshot_one(id).await.unwrap();
        snapshot.info.status = ProcessStatus::Stopping;
        snapshot.desired_running = false;
        snapshot.generation = 41;

        let restored = manager.restore_snapshot(snapshot).await.unwrap();
        let after = manager.snapshot_one(id).await.unwrap();

        assert_eq!(restored.status, ProcessStatus::Stopped);
        assert!(!after.desired_running);
        assert!(after.generation > 41);
    }

    #[tokio::test]
    async fn enabled_rollback_restores_non_running_error_state() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("restore-enabled-{id}")))
            .await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Errored;
            process.desired_running = false;
            process.generation = 17;
        }
        let snapshot = manager.snapshot_one(id).await.unwrap();
        manager.set_enabled(id, false).await.unwrap();

        let restored = manager.restore_enabled_snapshot(snapshot).await.unwrap();
        let after = manager.snapshot_one(id).await.unwrap();

        assert!(restored.enabled);
        assert_eq!(restored.status, ProcessStatus::Errored);
        assert!(!after.desired_running);
        assert!(after.generation > 17);
    }

    #[tokio::test]
    async fn start_rejects_a_process_while_failed_child_cleanup_is_pending() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        manager
            .register_stopped(id, test_config(format!("cleanup-pending-{id}")))
            .await;
        {
            let entry = manager.registry.get(&id).unwrap();
            let mut process = entry.write().await;
            process.status = ProcessStatus::Errored;
            process.desired_running = false;
            process.pid = Some(std::process::id());
        }

        let error = manager.start_existing(id).await.unwrap_err();
        assert!(error.to_string().contains("still cleaning up"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn explicit_stop_terminates_preserved_windows_descendants() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let directory = std::env::temp_dir().join(format!("rundock-stop-tree-{id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let pid_file = directory.join("descendant.pid");
        let pid_path = pid_file.to_string_lossy().replace("'", "''");
        let command = format!(
            "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-Command', 'Start-Sleep -Seconds 30') -PassThru; $child.Id | Set-Content -Encoding ascii '{pid_path}'; Start-Sleep -Seconds 30"
        );
        let mut config = test_config(format!("stop-tree-{id}"));
        config.script = "powershell.exe".to_string();
        config.args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            command,
        ];
        config.autorestart = false;

        let started = manager.start(config).await.unwrap();
        let root_pid = started.pid.unwrap();
        let root_identity = crate::process::identity::capture_process_identity(root_pid)
            .expect("managed root process has no identity");
        for _ in 0..50 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        let descendant_pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("descendant did not publish its PID")
            .trim()
            .parse()
            .unwrap();
        let descendant_identity =
            crate::process::identity::capture_process_identity(descendant_pid)
                .expect("managed descendant process has no identity");
        {
            let entry = manager.registry.get(&started.id).unwrap();
            let mut process = entry.write().await;
            let mut guard = process.process_tree.take().unwrap();
            guard.preserve_on_drop().unwrap();
        }

        let stopped = manager.stop(started.id).await.unwrap();

        assert_eq!(stopped.status, ProcessStatus::Stopped);
        for _ in 0..50 {
            if !crate::process::identity::process_identity_matches(root_pid, &root_identity)
                && !crate::process::identity::process_identity_matches(
                    descendant_pid,
                    &descendant_identity,
                )
            {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
        assert!(!crate::process::identity::process_identity_matches(
            root_pid,
            &root_identity
        ));
        assert!(!crate::process::identity::process_identity_matches(
            descendant_pid,
            &descendant_identity
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cron_root_exit_terminates_descendants_before_sleeping() {
        let manager = ProcessManager::new(Arc::new(RwLock::new(NotificationsStore::default())));
        let id = Uuid::new_v4();
        let directory = std::env::temp_dir().join(format!("rundock-cron-tree-{id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let pid_file = directory.join("descendant.pid");
        let pid_path = pid_file.to_string_lossy().replace('\'', "''");
        let command = format!(
            "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-Command', 'Start-Sleep -Seconds 30') -PassThru; $child.Id | Set-Content -Encoding ascii '{pid_path}'; Start-Sleep -Seconds 1"
        );
        let mut config = test_config(format!("cron-tree-{id}"));
        config.script = "powershell.exe".to_string();
        config.args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            command,
        ];
        config.autorestart = false;
        config.cron = Some("0 0 0 1 1 *".to_string());

        let started = manager.start(config).await.unwrap();
        tokio::time::timeout(tokio::time::Duration::from_secs(10), async {
            loop {
                let info = manager.get(started.id).await.unwrap();
                if info.status == ProcessStatus::Sleeping && info.pid.is_none() {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("initial cron run did not settle");
        if pid_file.exists() {
            std::fs::remove_file(&pid_file).unwrap();
        }
        manager.cron_trigger_tx.send(started.id).await.unwrap();
        for _ in 0..50 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        let descendant_pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("descendant did not publish its PID")
            .trim()
            .parse()
            .unwrap();
        let descendant_identity =
            crate::process::identity::capture_process_identity(descendant_pid)
                .expect("managed descendant process has no identity");

        let info = tokio::time::timeout(tokio::time::Duration::from_secs(10), async {
            loop {
                let info = manager.get(started.id).await.unwrap();
                if info.status == ProcessStatus::Errored || !info.cron_run_history.is_empty() {
                    break info;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("cron exit did not settle");

        assert_eq!(info.status, ProcessStatus::Sleeping);
        assert_eq!(info.pid, None);
        assert_eq!(info.cron_run_history.len(), 1);
        assert!(!crate::process::identity::process_identity_matches(
            descendant_pid,
            &descendant_identity
        ));
        manager.stop(started.id).await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn pre_spawn_restart_failure_cannot_leave_process_stopping() {
        let mut process = ManagedProcess::new(test_config("restart-log-failure".to_string()));
        process.status = ProcessStatus::Stopping;
        process.desired_running = true;
        process.generation = 11;

        assert!(ProcessManager::mark_respawn_failed(&mut process, 11));
        assert_eq!(process.status, ProcessStatus::Errored);
        assert!(!process.desired_running);
        assert_eq!(process.generation, 12);
    }
}
