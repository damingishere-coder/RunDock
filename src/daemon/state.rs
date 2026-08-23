// @group DatabaseOperations : Daemon shared state — process registry with disk persistence

use crate::config::auth_config::AuthConfig;
use crate::config::daemon_config::DaemonConfig;
use crate::config::ecosystem::AppConfig;
use crate::config::notification_store::NotificationsStore;
use crate::config::project_store::ProjectStore;
use crate::config::telegram_config::TelegramConfig;
use crate::models::cron_run::CronRun;
use crate::models::tunnel::TunnelSettings;
use crate::process::manager::{ManagedProcessSnapshot, ProcessManager};
use crate::terminal::TerminalManager;
use crate::tunnel::TunnelManager;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;


/// Persistent snapshot of process configs (saved to disk)
#[derive(Serialize, Deserialize, Default)]
pub struct SavedState {
    pub saved_at: Option<DateTime<Utc>>,
    pub apps: Vec<SavedApp>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedApp {
    pub id: Uuid,
    pub config: AppConfig,
    pub restart_count: u32,
    pub autorestart_on_restore: bool,
    #[serde(default)]
    pub cron_run_history: Vec<CronRun>,
    /// PID of the process at the time state was last saved.
    /// Used on restore to detect and clean up orphaned OS processes.
    #[serde(default)]
    pub last_pid: Option<u32>,
    /// For cron jobs: true if the scheduler was active (Sleeping) at save time.
    /// false means the user had manually stopped it — do NOT re-arm the scheduler on restore.
    /// Defaults to true for backward compatibility with old state files.
    #[serde(default = "default_true")]
    pub cron_was_active: bool,
}

fn default_true() -> bool { true }

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
    pub ai_device_auth: Arc<tokio::sync::Mutex<Option<crate::models::ai::DeviceAuthState>>>,

    // @group Authentication : Session and auth state
    /// Active browser sessions: token → expiry timestamp
    pub sessions: Arc<DashMap<String, DateTime<Utc>>>,
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
}

impl DaemonState {
    pub fn new(config: DaemonConfig) -> Self {
        let notifications = Arc::new(RwLock::new(crate::config::notification_store::load()));
        let projects = Arc::new(RwLock::new(crate::config::project_store::load()));

        let auth_cfg = crate::config::auth_config::load();

        let telegram_cfg = crate::config::telegram_config::load();
        let tunnel_cfg = crate::config::tunnel_config::load();

        Self {
            manager: ProcessManager::new(Arc::clone(&notifications)),
            config,
            started_at: Utc::now(),
            notifications,
            projects,
            ai_device_auth: Arc::new(tokio::sync::Mutex::new(None)),
            sessions: Arc::new(DashMap::new()),
            auth: Arc::new(RwLock::new(auth_cfg)),
            telegram: Arc::new(RwLock::new(telegram_cfg)),
            tunnel_manager: TunnelManager::new(),
            tunnel_settings: Arc::new(RwLock::new(tunnel_cfg)),
            terminal_manager: TerminalManager::new(),
        }
    }

    pub async fn save_projects(&self) -> Result<()> {
        let store = self.projects.read().await.clone();
        tokio::task::spawn_blocking(move || crate::config::project_store::save(&store)).await??;
        Ok(())
    }

    // @group DatabaseOperations : Serialize current process list to JSON file
    pub async fn save_to_disk(&self) -> Result<()> {
        let apps = self
            .manager
            .snapshot()
            .await
            .into_iter()
            .map(saved_app_from_snapshot)
            .collect();

        let saved = SavedState {
            saved_at: Some(Utc::now()),
            apps,
        };

        let path = crate::config::paths::state_file();
        let content = serde_json::to_string_pretty(&saved)?;

        // Run blocking I/O on a dedicated thread to avoid stalling the async runtime.
        // Uses atomic tmp-then-rename pattern; falls back to a direct write on Windows
        // if MoveFileExW fails (e.g. due to antivirus locks on the destination).
        tokio::task::spawn_blocking(move || -> Result<()> {
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &content)?;
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
                std::fs::write(&path, &content)?;
            }
            Ok(())
        })
        .await??;
        Ok(())
    }

    // @group DatabaseOperations : Load persisted state from disk
    pub async fn load_from_disk() -> Result<SavedState> {
        let path = crate::config::paths::state_file();
        let content = std::fs::read_to_string(path)?;
        let state: SavedState = serde_json::from_str(&content)?;
        Ok(state)
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
        use crate::process::manager::{is_pid_alive, kill_orphan_pid};

        for app in saved.apps {
            // Disabled entries must never come back merely because their old
            // PID survived the daemon restart. Keep the real running state if
            // the verified kill fails so the UI can report and retry it.
            if !app.config.enabled {
                if let Some(pid) = app.last_pid {
                    if is_pid_alive(pid) {
                        match kill_orphan_pid(pid).await {
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
                                self.manager
                                    .register_running_adopted(app.id, app.config, pid)
                                    .await;
                                continue;
                            }
                        }
                    }
                }
                self.manager.register_stopped(app.id, app.config).await;
                continue;
            }

            if app.config.cron.is_some() {
                // Kill any stale PID first (cron jobs are idempotent)
                if let Some(pid) = app.last_pid {
                    if is_pid_alive(pid) {
                        tracing::info!(
                            "killing stale cron process '{}' (PID {}) before re-registering",
                            app.config.name, pid
                        );
                        if let Err(error) = kill_orphan_pid(pid).await {
                            tracing::warn!(
                                "failed to stop stale cron process '{}' (PID {}): {error}",
                                app.config.name,
                                pid
                            );
                        }
                    }
                }
                if app.cron_was_active {
                    // Cron scheduler was running at shutdown — restore as Sleeping (re-arm scheduler)
                    if let Err(e) = self.manager.register_sleeping(app.id, app.config, app.cron_run_history).await {
                        tracing::warn!("failed to restore cron process '{}': {e}", app.id);
                    }
                } else {
                    // User had manually stopped this cron job — restore as Stopped, don't re-arm
                    tracing::info!(
                        "cron process '{}' was stopped at shutdown — restoring as stopped",
                        app.config.name
                    );
                    self.manager.register_stopped(app.id, app.config).await;
                }
                continue;
            }

            match app.last_pid {
                Some(pid) if is_pid_alive(pid) => {
                    // Process survived the daemon restart — re-adopt it with its saved ID
                    tracing::info!(
                        "re-adopting running process '{}' (PID {})",
                        app.config.name, pid
                    );
                    self.manager.register_running_adopted(app.id, app.config, pid).await;
                }
                Some(pid) => {
                    // Process died while daemon was down — mark stopped, let user restart
                    tracing::info!(
                        "process '{}' (PID {}) exited while daemon was down — marking stopped",
                        app.config.name, pid
                    );
                    self.manager.register_stopped(app.id, app.config).await;
                }
                None => {
                    // No PID was ever saved — mark stopped
                    self.manager.register_stopped(app.id, app.config).await;
                }
            }
        }
    }
}

fn saved_app_from_snapshot(snapshot: ManagedProcessSnapshot) -> SavedApp {
    let info = snapshot.info;
    SavedApp {
        id: info.id,
        config: snapshot.config,
        restart_count: info.restart_count,
        autorestart_on_restore: info.autorestart,
        cron_run_history: info.cron_run_history,
        last_pid: info.pid,
        // Cron scheduler was active if the job was Sleeping at save time.
        // Stopped = user manually stopped it; don't re-arm on next daemon start.
        cron_was_active: info.cron.is_some()
            && !matches!(
                info.status,
                crate::models::process_status::ProcessStatus::Stopped
            ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::process_status::ProcessStatus;
    use std::collections::HashMap;

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
        assert_eq!(decoded.apps[0].autorestart_on_restore, false);
        assert_eq!(decoded.apps[0].cron_was_active, false);
        assert_eq!(ProcessStatus::Stopped, manager.get(id).await.unwrap().status);
    }
}
