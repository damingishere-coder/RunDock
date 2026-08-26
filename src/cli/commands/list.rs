// @group BusinessLogic : `alter list` command handler

use crate::client::daemon_client::DaemonClient;
use crate::models::process_info::ProcessInfo;
use crate::utils::table::render_process_table;
use anyhow::{Context, Result};

pub async fn run(client: &DaemonClient, json_mode: bool) -> Result<()> {
    if !client.is_alive().await {
        eprintln!("[alter] daemon is not running. Start it with: alter daemon start");
        std::process::exit(1);
    }

    let result = client.get("/api/v1/processes").await?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let raw_processes = result
        .get("processes")
        .ok_or_else(|| anyhow::anyhow!("daemon process list response is missing 'processes'"))?;
    let processes: Vec<ProcessInfo> = serde_json::from_value(raw_processes.clone())
        .context("daemon process list response is malformed")?;

    render_process_table(&processes);
    Ok(())
}
