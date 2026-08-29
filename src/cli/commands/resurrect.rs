// @group BusinessLogic : `alter resurrect` command handler

use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

pub async fn run(client: &DaemonClient, json_mode: bool) -> Result<()> {
    if !client.is_alive().await {
        eprintln!("[RunDock] daemon is not running");
        std::process::exit(1);
    }
    let result = client
        .post("/api/v1/system/resurrect", serde_json::json!({}))
        .await?;
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let msg = result["message"].as_str().unwrap_or("done");
        println!("[RunDock] {msg}");
    }
    Ok(())
}
