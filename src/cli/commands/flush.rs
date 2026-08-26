// @group BusinessLogic : `alter flush` command handler — delete log files

use crate::cli::commands::stop::{process_targets, require_alive, resolve_id};
use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub async fn run(client: &DaemonClient, target: Option<&str>, json_mode: bool) -> Result<()> {
    require_alive(client).await;

    let targets: Vec<String> = if let Some(t) = target {
        if t == "all" {
            let list = client.get("/api/v1/processes").await?;
            process_targets(&list)?
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        } else {
            vec![resolve_id(client, t).await?]
        }
    } else {
        let list = client.get("/api/v1/processes").await?;
        process_targets(&list)?
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };

    let mut failures = Vec::new();
    for id in targets {
        match client.delete(&format!("/api/v1/processes/{id}/logs")).await {
            Ok(_) => {
                if !json_mode {
                    println!("[alter] flushed logs for {id}");
                }
            }
            Err(error) => {
                eprintln!("[alter] failed to flush logs for {id}: {error}");
                failures.push(format!("{id}: {error}"));
            }
        }
    }
    if !failures.is_empty() {
        anyhow::bail!(
            "failed to flush {} process log set(s): {}",
            failures.len(),
            failures.join("; ")
        );
    }
    Ok(())
}
