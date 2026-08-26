// @group Configuration : Library crate root — exposes all modules and shared CLI entry logic

pub mod api;
pub mod cli;
pub mod client;
pub mod config;
pub mod daemon;
pub mod logging;
pub mod models;
pub mod notifications;
pub mod process;
pub mod telegram;
pub mod terminal;
pub mod tunnel;
pub mod utils;
pub mod web;

use crate::cli::args::{Cli, Commands};
use crate::client::daemon_client::DaemonClient;

struct RestartHandoffGuard {
    path: std::path::PathBuf,
}

impl Drop for RestartHandoffGuard {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.path.display(), %error, "failed to remove restart handoff during cleanup");
            }
        }
    }
}

fn write_restart_handoff(token: &str) -> anyhow::Result<RestartHandoffGuard> {
    let parsed_token = uuid::Uuid::parse_str(token)?;
    anyhow::ensure!(
        parsed_token.to_string() == token,
        "invalid restart handoff token"
    );
    let raw_path = std::env::var_os("ALTER_RESTART_HANDOFF_PATH")
        .ok_or_else(|| anyhow::anyhow!("restart handoff path is missing"))?;
    let path = std::path::PathBuf::from(raw_path);
    let data_dir = crate::config::paths::data_dir();
    let expected = data_dir.join(format!(".restart-handoff-{token}.json"));
    anyhow::ensure!(
        path == expected,
        "restart handoff path is outside the data directory"
    );
    std::fs::create_dir_all(&data_dir)?;
    anyhow::ensure!(
        !path.exists(),
        "restart handoff path already exists: {}",
        path.display()
    );
    let bytes = serde_json::to_vec(&serde_json::json!({
        "token": token,
        "pid": std::process::id(),
        "phase": "prepared"
    }))?;
    crate::config::atomic_file::write_with_backup(&path, &bytes, None)?;
    Ok(RestartHandoffGuard { path })
}

pub(crate) fn mark_restart_handoff_ready_from_env() -> anyhow::Result<()> {
    if std::env::var_os("ALTER_RESTART_WAIT_FOR_PORT").is_none() {
        return Ok(());
    }
    let token = std::env::var("ALTER_RESTART_HANDOFF_TOKEN")
        .map_err(|_| anyhow::anyhow!("restart handoff token is missing"))?;
    let parsed_token = uuid::Uuid::parse_str(&token)?;
    anyhow::ensure!(
        parsed_token.to_string() == token,
        "invalid restart handoff token"
    );
    let raw_path = std::env::var_os("ALTER_RESTART_HANDOFF_PATH")
        .ok_or_else(|| anyhow::anyhow!("restart handoff path is missing"))?;
    let path = std::path::PathBuf::from(raw_path);
    let expected = crate::config::paths::data_dir().join(format!(".restart-handoff-{token}.json"));
    anyhow::ensure!(
        path == expected,
        "restart handoff path is outside the data directory"
    );
    let bytes = serde_json::to_vec(&serde_json::json!({
        "token": token,
        "pid": std::process::id(),
        "phase": "ready"
    }))?;
    crate::config::atomic_file::write_with_backup(&path, &bytes, None)
}

// @group BusinessLogic : Shared CLI dispatch logic — used by both alter and alter-dev binaries
pub async fn run_cli(cli: Cli) -> anyhow::Result<()> {
    // @group BusinessLogic > Daemon : Hidden internal entry point for daemon process
    if cli.internal_daemon {
        let mut _restart_process_trees = Vec::new();
        let mut _restart_handoff = None;
        let config = crate::config::daemon_config::DaemonConfig {
            host: cli.host.clone(),
            port: cli.port,
            ..Default::default()
        };
        if std::env::var_os("ALTER_RESTART_WAIT_FOR_PORT").is_some() {
            // Prove that the replacement can load every durable config and the
            // process snapshot before the old daemon gives up the control port.
            let preflight = crate::daemon::state::DaemonState::new(config.clone())?;
            let saved = preflight.load_from_disk().await?;
            for app in &saved.apps {
                if let (Some(pid), Some(identity)) = (app.last_pid, &app.process_identity) {
                    anyhow::ensure!(
                        crate::process::identity::process_identity_matches(pid, identity),
                        "saved process {} changed identity before restart handoff",
                        app.id
                    );
                    let mut process_tree =
                        crate::process::tree::ProcessTreeGuard::new(pid, &app.id.to_string())
                            .map_err(|error| {
                                anyhow::anyhow!(
                                    "failed to preserve process tree {} during restart: {error}",
                                    app.id
                                )
                            })?;
                    process_tree.preserve_on_drop();
                    _restart_process_trees.push(process_tree);
                }
            }
            drop(saved);
            drop(preflight);
            let token = std::env::var("ALTER_RESTART_HANDOFF_TOKEN")
                .map_err(|_| anyhow::anyhow!("restart handoff token is missing"))?;
            _restart_handoff = Some(write_restart_handoff(&token)?);
            let address = crate::daemon::server::loopback_socket_addr(&cli.host, cli.port)?;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    anyhow::bail!("previous daemon did not release {address} within 30 seconds");
                }
                match tokio::time::timeout(
                    remaining.min(std::time::Duration::from_millis(500)),
                    tokio::net::TcpStream::connect(address),
                )
                .await
                {
                    Ok(Ok(_)) | Err(_) => {}
                    Ok(Err(_)) => break,
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            while crate::utils::pid::is_daemon_running() {
                if tokio::time::Instant::now() >= deadline {
                    anyhow::bail!("previous daemon did not release its PID file within 30 seconds");
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        return daemon::run(config).await;
    }

    let client = DaemonClient::new(&cli.host, cli.port)?;
    let json = cli.json;

    match cli.command.unwrap_or(Commands::List) {
        Commands::Start(args) => cli::commands::start::run(&client, args, json).await?,

        Commands::Stop(r) => cli::commands::stop::run(&client, &r.target, json).await?,

        Commands::Restart(r) => cli::commands::restart::run(&client, &r.target, json).await?,

        Commands::Delete(r) => cli::commands::delete::run(&client, &r.target, json).await?,

        Commands::List => cli::commands::list::run(&client, json).await?,

        Commands::Describe(r) => cli::commands::describe::run(&client, &r.target, json).await?,

        Commands::Logs(args) => cli::commands::logs::run(&client, args, json).await?,

        Commands::Flush(r) => cli::commands::flush::run(&client, r.target.as_deref(), json).await?,

        Commands::Reset(r) => cli::commands::reset::run(&client, &r.target, json).await?,

        Commands::Save => cli::commands::save::run(&client, json).await?,

        Commands::Resurrect => cli::commands::resurrect::run(&client, json).await?,

        Commands::Daemon(d) => {
            cli::commands::daemon::run(&client, d.action, &cli.host, cli.port).await?
        }

        Commands::Auth(a) => cli::commands::auth::run(&client, a.action, json).await?,

        Commands::Startup => cli::commands::startup::run_startup().await?,

        Commands::Unstartup => cli::commands::startup::run_unstartup().await?,

        Commands::Web => {
            let url = format!("http://{}:{}/", cli.host, cli.port);
            println!("[alter] dashboard: {url}");
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("explorer.exe").arg(&url).spawn();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
    }

    Ok(())
}
