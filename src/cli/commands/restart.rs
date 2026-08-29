// @group BusinessLogic : `alter restart` command handler

use crate::cli::commands::stop::{process_targets, require_alive, resolve_id};
use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub async fn run(client: &DaemonClient, target: &str, json_mode: bool) -> Result<()> {
    require_alive(client).await;

    if target == "all" {
        let list = client.get("/api/v1/processes").await?;
        let processes = process_targets(&list)?;
        let mut failures = Vec::new();
        for (id, name) in &processes {
            if let Err(e) = client
                .post(
                    &format!("/api/v1/processes/{id}/restart"),
                    serde_json::json!({}),
                )
                .await
            {
                failures.push(format!("{name}: {e}"));
            } else if !json_mode {
                println!("[RunDock] restarted '{name}'");
            }
        }
        if !failures.is_empty() {
            anyhow::bail!("failed to restart: {}", failures.join("; "));
        }
        return Ok(());
    }

    let id = resolve_id(client, target).await?;
    let result = client
        .post(
            &format!("/api/v1/processes/{id}/restart"),
            serde_json::json!({}),
        )
        .await?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let name = result["name"].as_str().unwrap_or(target);
        println!("[RunDock] restarted '{name}'");
    }
    Ok(())
}
