// @group BusinessLogic : `alter auth` command handler

use crate::cli::args::AuthAction;
use crate::client::daemon_client::DaemonClient;
use anyhow::{anyhow, Result};

pub async fn run(client: &DaemonClient, action: AuthAction, json_mode: bool) -> Result<()> {
    if !client.is_alive().await {
        return Err(anyhow!("alter daemon is not running"));
    }

    match action {
        AuthAction::Disable => {
            let result = client.delete("/api/v1/auth/password").await?;
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("[alter] dashboard password disabled");
            }
        }
    }

    Ok(())
}
