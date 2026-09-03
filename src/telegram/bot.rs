// @group BusinessLogic : Telegram long-polling bot loop

use crate::daemon::state::DaemonState;
use crate::telegram::commands;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const TG_API: &str = "https://api.telegram.org";
const MAX_TG_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

async fn decode_bounded_response<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> anyhow::Result<T> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_TG_RESPONSE_BYTES as u64)
    {
        anyhow::bail!("Telegram response exceeded the size limit");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > MAX_TG_RESPONSE_BYTES {
            anyhow::bail!("Telegram response exceeded the size limit");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(Into::into)
}

// @group Types : Telegram API response wrappers
#[derive(Deserialize)]
struct TgResponse<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    chat: Chat,
    text: Option<String>,
    from: Option<From>,
}

#[derive(Deserialize)]
struct Chat {
    id: i64,
}

#[derive(Deserialize)]
struct From {
    id: i64,
}

// @group BusinessLogic : Register bot commands with Telegram for autocomplete (setMyCommands)
async fn register_commands(client: &reqwest::Client, token: &str) -> anyhow::Result<()> {
    let url = format!("{TG_API}/bot{token}/setMyCommands");
    let commands = serde_json::json!({
        "commands": [
            { "command": "list",      "description": "List all processes and their status" },
            { "command": "status",    "description": "Status of a process or namespace: /status <name> | /status ns <ns>" },
            { "command": "start",   "description": "Start process or namespace: /start <name> | /start ns <ns>" },
            { "command": "stop",    "description": "Stop process or namespace: /stop <name> | /stop ns <ns>" },
            { "command": "restart", "description": "Restart process or namespace: /restart <name> | /restart ns <ns>" },
            { "command": "logs",    "description": "Get recent logs: /logs <name> [lines]" },
            { "command": "ping",      "description": "Check if the daemon is responsive" },
            { "command": "help",      "description": "Show available commands" }
        ]
    });

    let response = client
        .post(&url)
        .json(&commands)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("setMyCommands failed: {}", error.without_url()))?;
    if !response.status().is_success() {
        anyhow::bail!("setMyCommands returned {}", response.status());
    }
    let result: TgResponse<serde_json::Value> = decode_bounded_response(response).await?;
    if !result.ok {
        anyhow::bail!("setMyCommands returned ok=false");
    }
    tracing::info!("telegram: commands registered for autocomplete");
    Ok(())
}

// @group BusinessLogic : Entry point — run the polling loop as a background task
pub async fn run(state: Arc<DaemonState>) {
    let mut shutdown = state.subscribe_shutdown();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(45))
        .build()
        .expect("failed to build reqwest client for telegram bot");

    let mut checkpoint = match crate::config::telegram_checkpoint::load() {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            tracing::error!(
                "telegram: refusing to poll with an unreadable update checkpoint: {error}"
            );
            return;
        }
    };
    let mut offset: i64 = 0;
    let mut active_token_fingerprint: Option<String> = None;
    // @group BusinessLogic > State : Track which token we last registered commands for,
    // so we re-register automatically if the token changes at runtime.
    let mut registered_for_token: Option<String> = None;
    let mut warned_invalid_allowlist = false;
    loop {
        if state.is_shutdown_requested() {
            return;
        }
        // @group BusinessLogic > Config : Re-read config each cycle so hot changes take effect
        let (enabled, token, allowed_chat_ids) = {
            let cfg = state.telegram.read().await;
            (
                cfg.enabled,
                cfg.bot_token.clone(),
                cfg.allowed_chat_ids.clone(),
            )
        };

        if !enabled || token.is_none() {
            warned_invalid_allowlist = false;
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(5)).await {
                return;
            }
            continue;
        }

        if allowed_chat_ids.is_empty() {
            if !warned_invalid_allowlist {
                tracing::error!(
                    "telegram: bot is enabled without an allowlist; polling is disabled"
                );
                warned_invalid_allowlist = true;
            }
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(30)).await {
                return;
            }
            continue;
        }
        warned_invalid_allowlist = false;

        let token = token.unwrap();
        let token_fingerprint = crate::config::telegram_checkpoint::token_fingerprint(&token);
        if active_token_fingerprint.as_deref() != Some(&token_fingerprint) {
            offset = if checkpoint.token_fingerprint == token_fingerprint {
                checkpoint.next_update_id
            } else {
                0
            };
            active_token_fingerprint = Some(token_fingerprint.clone());
        }

        // @group BusinessLogic > Commands : Register autocomplete commands once per token
        if registered_for_token.as_deref() != Some(&token) {
            let registration = tokio::select! {
                _ = shutdown.recv() => return,
                result = register_commands(&client, &token) => result,
            };
            match registration {
                Ok(()) => registered_for_token = Some(token.clone()),
                Err(error) => tracing::warn!("telegram: {error}"),
            }
        }

        // @group BusinessLogic > Polling : Fetch updates via long poll (timeout=30s)
        let url = format!("{TG_API}/bot{token}/getUpdates?offset={offset}&timeout=30");

        let response = tokio::select! {
            _ = shutdown.recv() => return,
            result = client.get(&url).send() => result,
        };
        let resp = match response {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("telegram: getUpdates failed: {}", e.without_url());
                if wait_or_shutdown(&mut shutdown, Duration::from_secs(5)).await {
                    return;
                }
                continue;
            }
        };

        if !resp.status().is_success() {
            tracing::warn!("telegram: getUpdates returned HTTP {}", resp.status());
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(10)).await {
                return;
            }
            continue;
        }

        let decoded = tokio::select! {
            _ = shutdown.recv() => return,
            result = decode_bounded_response(resp) => result,
        };
        let updates: TgResponse<Vec<Update>> = match decoded {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("telegram: failed to parse getUpdates response: {e}");
                if wait_or_shutdown(&mut shutdown, Duration::from_secs(5)).await {
                    return;
                }
                continue;
            }
        };

        if !updates.ok {
            // Log full response body so the user can diagnose token/auth issues
            tracing::warn!("telegram: getUpdates returned ok=false — check your bot token");
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(10)).await {
                return;
            }
            continue;
        }

        // @group BusinessLogic > Updates : Process each update
        for update in updates.result.unwrap_or_default() {
            let update_id = update.update_id;
            if update_id < 0 || update_id < offset {
                tracing::warn!(
                    update_id,
                    offset,
                    "telegram: ignoring a negative or stale update ID"
                );
                continue;
            }
            let Some(next_offset) = next_update_offset(update_id) else {
                tracing::error!(
                    update_id,
                    "telegram: refusing an update whose ID cannot be checkpointed safely"
                );
                return;
            };
            if let Some(message) = update.message {
                let chat_id = message.chat.id;
                let sender_id = message.from.as_ref().map(|f| f.id).unwrap_or(chat_id);

                // Commands are processed in update order. This makes the durable
                // checkpoint mean "processing finished", not merely "task spawned".
                if !is_authorized(&allowed_chat_ids, chat_id, sender_id) {
                    tracing::debug!(
                        "telegram: ignoring message — chat_id={} sender_id={} not in whitelist",
                        chat_id,
                        sender_id
                    );
                } else if let Some(text) = message.text {
                    if let Err(error) = dispatch_command(&state, &token, chat_id, &text).await {
                        tracing::warn!(update_id, %error, "telegram: command error");
                    }
                }
            }

            let next_checkpoint = crate::config::telegram_checkpoint::TelegramCheckpoint {
                token_fingerprint: token_fingerprint.clone(),
                next_update_id: next_offset,
            };
            let checkpoint_to_save = next_checkpoint.clone();
            let checkpoint_result = tokio::task::spawn_blocking(move || {
                crate::config::telegram_checkpoint::save(&checkpoint_to_save)
            })
            .await;
            if let Err(error) = checkpoint_result
                .map_err(anyhow::Error::from)
                .and_then(|result| result)
            {
                tracing::error!(
                    "telegram: update {} was processed but its checkpoint could not be persisted; stopping polling to prevent duplicate command execution: {error}",
                    update_id
                );
                return;
            }
            checkpoint = next_checkpoint;
            offset = next_offset;
            if state.is_shutdown_requested() {
                return;
            }
        }
    }
}

fn next_update_offset(update_id: i64) -> Option<i64> {
    update_id.checked_add(1)
}

async fn wait_or_shutdown(
    shutdown: &mut tokio::sync::broadcast::Receiver<()>,
    duration: Duration,
) -> bool {
    tokio::select! {
        _ = sleep(duration) => false,
        _ = shutdown.recv() => true,
    }
}

fn is_authorized(allowed_chat_ids: &[i64], chat_id: i64, sender_id: i64) -> bool {
    !allowed_chat_ids.is_empty()
        && (allowed_chat_ids.contains(&chat_id) || allowed_chat_ids.contains(&sender_id))
}

fn normalize_command_token(token: &str) -> String {
    token
        .split_once('@')
        .map_or(token, |(command, _)| command)
        .to_lowercase()
}

// @group BusinessLogic > Dispatch : Parse command text and call the appropriate handler
async fn dispatch_command(
    state: &Arc<DaemonState>,
    token: &str,
    chat_id: i64,
    text: &str,
) -> anyhow::Result<()> {
    let parts: Vec<&str> = text.splitn(3, ' ').collect();
    // Strip @BotName only from the command token. Process and namespace
    // arguments may legitimately contain '@' and must remain byte-for-byte intact.
    let cmd = normalize_command_token(parts[0]);

    match cmd.as_str() {
        "/ping" => commands::cmd_ping(token, chat_id).await,
        "/help" | "/start" if parts.len() == 1 => commands::cmd_help(token, chat_id).await,
        "/list" => commands::cmd_list(state, token, chat_id).await,
        "/status" => match parts.get(1) {
            Some(&"ns") => match parts.get(2) {
                Some(ns) => commands::cmd_status_namespace(state, token, chat_id, ns).await,
                None => {
                    commands::send_message(token, chat_id, "Usage: /status ns &lt;namespace&gt;")
                        .await
                }
            },
            Some(name) => commands::cmd_status(state, token, chat_id, name).await,
            None => {
                commands::send_message(
                    token,
                    chat_id,
                    "Usage: /status &lt;name&gt; | /status ns &lt;ns&gt;",
                )
                .await
            }
        },
        "/start" => {
            // /start with an argument — "ns <namespace>" targets a namespace, otherwise a process name
            match parts.get(1) {
                Some(&"ns") => match parts.get(2) {
                    Some(ns) => commands::cmd_start_namespace(state, token, chat_id, ns).await,
                    None => {
                        commands::send_message(token, chat_id, "Usage: /start ns &lt;namespace&gt;")
                            .await
                    }
                },
                Some(name) => commands::cmd_start(state, token, chat_id, name).await,
                None => commands::cmd_help(token, chat_id).await,
            }
        }
        "/stop" => match parts.get(1) {
            Some(&"ns") => match parts.get(2) {
                Some(ns) => commands::cmd_stop_namespace(state, token, chat_id, ns).await,
                None => {
                    commands::send_message(token, chat_id, "Usage: /stop ns &lt;namespace&gt;")
                        .await
                }
            },
            Some(name) => commands::cmd_stop(state, token, chat_id, name).await,
            None => {
                commands::send_message(
                    token,
                    chat_id,
                    "Usage: /stop &lt;name&gt; | /stop ns &lt;namespace&gt;",
                )
                .await
            }
        },
        "/restart" => match parts.get(1) {
            Some(&"ns") => match parts.get(2) {
                Some(ns) => commands::cmd_restart_namespace(state, token, chat_id, ns).await,
                None => {
                    commands::send_message(token, chat_id, "Usage: /restart ns &lt;namespace&gt;")
                        .await
                }
            },
            Some(name) => commands::cmd_restart(state, token, chat_id, name).await,
            None => {
                commands::send_message(
                    token,
                    chat_id,
                    "Usage: /restart &lt;name&gt; | /restart ns &lt;namespace&gt;",
                )
                .await
            }
        },
        "/logs" => {
            if parts.len() < 2 {
                commands::send_message(token, chat_id, "Usage: /logs &lt;name&gt; [lines]").await
            } else {
                let name = parts[1];
                let lines: usize = parts
                    .get(2)
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(20)
                    .clamp(1, 200);
                commands::cmd_logs(state, token, chat_id, name, lines).await
            }
        }
        _ => {
            // Unknown command — send help
            commands::cmd_help(token, chat_id).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_authorized, next_update_offset, normalize_command_token};

    #[test]
    fn empty_allowlist_is_fail_closed() {
        assert!(!is_authorized(&[], 1, 1));
    }

    #[test]
    fn chat_or_sender_must_be_explicitly_allowed() {
        assert!(is_authorized(&[10], 10, 20));
        assert!(is_authorized(&[20], 10, 20));
        assert!(!is_authorized(&[30], 10, 20));
    }

    #[test]
    fn maximum_update_id_is_rejected_before_dispatch() {
        assert_eq!(next_update_offset(i64::MAX), None);
        assert_eq!(next_update_offset(i64::MAX - 1), Some(i64::MAX));
    }

    #[test]
    fn bot_suffix_is_removed_only_from_command_token() {
        let parts = "/stop@RunDockBot foo@bar"
            .splitn(3, ' ')
            .collect::<Vec<_>>();
        assert_eq!(normalize_command_token(parts[0]), "/stop");
        assert_eq!(parts[1], "foo@bar");
    }
}
