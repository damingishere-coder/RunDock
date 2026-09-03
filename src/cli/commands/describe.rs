// @group BusinessLogic : `alter describe` command handler

use crate::cli::commands::stop::{require_alive, resolve_id};
use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub async fn run(client: &DaemonClient, target: &str, json_mode: bool) -> Result<()> {
    require_alive(client).await;
    let id = resolve_id(client, target).await?;
    let result = client.get(&format!("/api/v1/processes/{id}")).await?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("{:<20} {}", "name:", result["name"].as_str().unwrap_or("-"));
    println!("{:<20} {}", "id:", result["id"].as_str().unwrap_or("-"));
    println!(
        "{:<20} {}",
        "status:",
        result["status"].as_str().unwrap_or("-")
    );
    println!(
        "{:<20} {}",
        "pid:",
        result["pid"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or("-".to_string())
    );
    println!(
        "{:<20} {}",
        "script:",
        result["script"].as_str().unwrap_or("-")
    );
    println!(
        "{:<20} {}",
        "restarts:",
        result["restart_count"].as_u64().unwrap_or(0)
    );
    println!(
        "{:<20} {}",
        "autorestart:",
        result["autorestart"].as_bool().unwrap_or(false)
    );
    println!(
        "{:<20} {}",
        "max_restarts:",
        result["max_restarts"].as_u64().unwrap_or(0)
    );
    println!(
        "{:<20} {}",
        "watch:",
        result["watch"].as_bool().unwrap_or(false)
    );
    println!(
        "{:<20} {}",
        "created_at:",
        result["created_at"].as_str().unwrap_or("-")
    );
    println!(
        "{:<20} {}",
        "started_at:",
        result["started_at"].as_str().unwrap_or("-")
    );
    Ok(())
}
