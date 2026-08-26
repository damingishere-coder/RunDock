// @group BusinessLogic : `alter stop` command handler

use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub fn process_targets(response: &serde_json::Value) -> Result<Vec<(String, String)>> {
    let processes = response
        .get("processes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("daemon process list response is malformed"))?;
    processes
        .iter()
        .map(|process| {
            let id = process
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("daemon process list contains an invalid id"))?;
            let name = process
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("daemon process list contains an invalid name"))?;
            Ok((id.to_string(), name.to_string()))
        })
        .collect()
}

pub async fn run(client: &DaemonClient, target: &str, json_mode: bool) -> Result<()> {
    require_alive(client).await;

    if target == "all" {
        let list = client.get("/api/v1/processes").await?;
        let processes = process_targets(&list)?;
        let mut failures = Vec::new();
        for (id, name) in &processes {
            if let Err(e) = client
                .post(
                    &format!("/api/v1/processes/{id}/stop"),
                    serde_json::json!({}),
                )
                .await
            {
                failures.push(format!("{name}: {e}"));
            } else if !json_mode {
                println!("[alter] stopped '{name}'");
            }
        }
        if !failures.is_empty() {
            anyhow::bail!("failed to stop: {}", failures.join("; "));
        }
        return Ok(());
    }

    let id = resolve_id(client, target).await?;
    let result = client
        .post(
            &format!("/api/v1/processes/{id}/stop"),
            serde_json::json!({}),
        )
        .await?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let name = result["name"].as_str().unwrap_or(target);
        println!("[alter] stopped '{name}'");
    }
    Ok(())
}

pub async fn require_alive(client: &DaemonClient) {
    if !client.is_alive().await {
        eprintln!("[alter] daemon is not running. Start it with: alter daemon start");
        std::process::exit(1);
    }
}

pub async fn resolve_id(client: &DaemonClient, name_or_id: &str) -> Result<String> {
    // Try direct UUID first
    if name_or_id.len() == 36 && name_or_id.contains('-') {
        return Ok(name_or_id.to_string());
    }
    // Search by name
    let list = client.get("/api/v1/processes").await?;
    for (id, name) in process_targets(&list)? {
        if name == name_or_id {
            return Ok(id);
        }
    }
    // Fall back: pass as-is and let the server resolve
    Ok(name_or_id.to_string())
}
