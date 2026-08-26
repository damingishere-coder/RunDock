// @group BusinessLogic : `alter reset` command handler — reset restart counter

use crate::cli::commands::stop::{require_alive, resolve_id};
use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub async fn run(client: &DaemonClient, target: &str, json_mode: bool) -> Result<()> {
    require_alive(client).await;
    let id = resolve_id(client, target).await?;
    let result = client
        .post(
            &format!("/api/v1/processes/{id}/reset"),
            serde_json::json!({}),
        )
        .await?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("[alter] reset restart counter for '{target}'");
    }
    Ok(())
}
