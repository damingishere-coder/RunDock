// @group BusinessLogic : `alter delete` command handler

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
            match client.delete(&format!("/api/v1/processes/{id}")).await {
                Ok(_) if !json_mode => println!("[RunDock] deleted '{name}'"),
                Ok(_) => {}
                Err(error) => failures.push(format!("{name}: {error}")),
            }
        }
        if !failures.is_empty() {
            anyhow::bail!("failed to delete: {}", failures.join("; "));
        }
        return Ok(());
    }

    let id = resolve_id(client, target).await?;
    let _ = client.delete(&format!("/api/v1/processes/{id}")).await?;

    if !json_mode {
        println!("[RunDock] deleted '{target}'");
    }
    Ok(())
}
