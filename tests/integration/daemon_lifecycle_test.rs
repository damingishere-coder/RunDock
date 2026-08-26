// @group IntegrationTests : Daemon start → spawn process → stop daemon lifecycle

#[cfg(test)]
mod tests {
    use alter::config::daemon_config::DaemonConfig;
    use alter::config::ecosystem::AppConfig;
    use alter::daemon::state::DaemonState;
    use std::collections::HashMap;
    use std::sync::{Arc, Once};

    fn isolate_data_paths() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let path = std::env::temp_dir().join(format!(
                "alter-integration-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            // SAFETY: this process-wide variable is initialised exactly once
            // before these integration tests start any child process work.
            unsafe { std::env::set_var("ALTER_DATA_DIR", path) };
        });
    }

    fn test_config() -> AppConfig {
        isolate_data_paths();
        #[cfg(windows)]
        let (script, args) = (
            "powershell.exe".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 30".to_string(),
            ],
        );
        #[cfg(not(windows))]
        let (script, args) = (
            "sh".to_string(),
            vec!["-c".to_string(), "sleep 30".to_string()],
        );
        AppConfig {
            name: format!("test-app-{}", uuid::Uuid::new_v4()),
            project_id: None,
            script,
            args,
            cwd: None,
            instances: 1,
            autorestart: false,
            max_restarts: 0,
            restart_delay_ms: 100,
            watch: false,
            watch_paths: vec![],
            watch_ignore: vec![],
            env: HashMap::new(),
            namespace: "test".to_string(),
            log_file: None,
            error_file: None,
            max_log_size_mb: 10,
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

    // @group IntegrationTests > Lifecycle : Start a process and verify it appears in the list
    #[tokio::test]
    async fn test_start_and_list() {
        isolate_data_paths();
        let state = Arc::new(DaemonState::new_isolated(DaemonConfig::default()));
        let config = test_config();
        let expected_name = config.name.clone();
        let info = state.manager.start(config).await.unwrap();
        assert_eq!(info.name, expected_name);

        let list = state.manager.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, expected_name);
    }

    // @group IntegrationTests > Lifecycle : Stop a running process
    #[tokio::test]
    async fn test_start_and_stop() {
        isolate_data_paths();
        let state = Arc::new(DaemonState::new_isolated(DaemonConfig::default()));
        let info = state.manager.start(test_config()).await.unwrap();
        let id = info.id;

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let stopped = state.manager.stop(id).await.unwrap();
        assert_eq!(
            stopped.status,
            alter::models::process_status::ProcessStatus::Stopped
        );
        assert_eq!(state.manager.get(id).await.unwrap().pid, None);
    }

    // @group IntegrationTests > Lifecycle : Delete removes from registry
    #[tokio::test]
    async fn test_delete_removes_from_registry() {
        isolate_data_paths();
        let state = Arc::new(DaemonState::new_isolated(DaemonConfig::default()));
        let info = state.manager.start(test_config()).await.unwrap();
        let id = info.id;

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        state.manager.delete(id).await.unwrap();

        let list = state.manager.list().await;
        assert_eq!(list.len(), 0);
    }
}
