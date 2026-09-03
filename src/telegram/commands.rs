// @group BusinessLogic : Telegram bot command handlers and message utilities

use crate::daemon::state::DaemonState;
use crate::logging::reader::read_merged_logs;
use crate::models::process_info::ProcessInfo;
use crate::models::process_status::ProcessStatus;
use crate::notifications::sender::{fire_namespace_event, ProcessEvent};
use anyhow::Result;
use futures::{stream::FuturesUnordered, StreamExt};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

const TG_API: &str = "https://api.telegram.org";

fn telegram_send_limit() -> &'static Arc<tokio::sync::Semaphore> {
    static LIMIT: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    LIMIT.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(8)))
}

// @group Utilities : Send a plain text message to a Telegram chat
pub async fn send_message(bot_token: &str, chat_id: i64, text: &str) -> Result<()> {
    for page in paginate_message(text)? {
        send_message_page(bot_token, chat_id, &page).await?;
    }
    Ok(())
}

fn paginate_message(text: &str) -> Result<Vec<String>> {
    const MAX_PAGE_BYTES: usize = 4_000;
    const MAX_MESSAGE_BYTES: usize = 64 * 1024;
    anyhow::ensure!(
        text.len() <= MAX_MESSAGE_BYTES,
        "Telegram message exceeds the 64 KiB safety limit"
    );
    if text.len() <= MAX_PAGE_BYTES {
        return Ok(vec![text.to_string()]);
    }

    #[derive(Clone)]
    struct OpenTag {
        name: String,
        opening: String,
    }

    let mut pages = Vec::new();
    let mut page = String::new();
    let mut open_tags: Vec<OpenTag> = Vec::new();
    let mut offset = 0usize;
    while offset < text.len() {
        let remaining = &text[offset..];
        let token_len = if remaining.starts_with('<') {
            remaining.find('>').map_or_else(
                || remaining.chars().next().map(char::len_utf8).unwrap_or(0),
                |end| end + 1,
            )
        } else if remaining.starts_with('&') {
            remaining.find(';').filter(|end| *end <= 16).map_or_else(
                || remaining.chars().next().map(char::len_utf8).unwrap_or(0),
                |end| end + 1,
            )
        } else {
            remaining.chars().next().map(char::len_utf8).unwrap_or(0)
        };
        let token = &remaining[..token_len];
        let mut next_tags = open_tags.clone();
        if token.starts_with('<') && token.ends_with('>') {
            let inner = token[1..token.len() - 1].trim();
            if let Some(closing) = inner.strip_prefix('/') {
                let name = closing
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if let Some(index) = next_tags.iter().rposition(|tag| tag.name == name) {
                    next_tags.truncate(index);
                }
            } else if !inner.starts_with('!') && !inner.ends_with('/') {
                let name = inner
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "b" | "strong" | "i" | "em" | "u" | "s" | "code" | "pre" | "a"
                ) {
                    next_tags.push(OpenTag {
                        name,
                        opening: token.to_string(),
                    });
                }
            }
        }
        let closing_bytes: usize = next_tags.iter().map(|tag| tag.name.len() + 3).sum();
        if page.len() + token.len() + closing_bytes > MAX_PAGE_BYTES && !page.is_empty() {
            for tag in open_tags.iter().rev() {
                page.push_str("</");
                page.push_str(&tag.name);
                page.push('>');
            }
            pages.push(std::mem::take(&mut page));
            for tag in &open_tags {
                page.push_str(&tag.opening);
            }
            continue;
        }
        page.push_str(token);
        open_tags = next_tags;
        offset += token_len;
    }
    if !page.is_empty() {
        for tag in open_tags.iter().rev() {
            page.push_str("</");
            page.push_str(&tag.name);
            page.push('>');
        }
        pages.push(page);
    }
    Ok(pages)
}

async fn send_message_page(bot_token: &str, chat_id: i64, text: &str) -> Result<()> {
    let _permit = telegram_send_limit()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram delivery queue is unavailable"))?;
    let url = format!("{TG_API}/bot{bot_token}/sendMessage");
    let mut response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| anyhow::anyhow!("failed to build Telegram client: {error}"))?
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML"
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Telegram request failed: {}", error.without_url()))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("Telegram request returned HTTP {status}");
    }
    if response
        .content_length()
        .is_some_and(|length| length > 64 * 1024)
    {
        anyhow::bail!("Telegram returned an oversized response");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > 64 * 1024 {
            anyhow::bail!("Telegram returned an oversized response");
        }
        body.extend_from_slice(&chunk);
    }
    let result: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| anyhow::anyhow!("Telegram returned invalid JSON: {error}"))?;
    if result.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let description = result
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Telegram rejected the message");
        anyhow::bail!("{description}");
    }
    Ok(())
}

// @group BusinessLogic > Commands : /ping — liveness check
pub async fn cmd_ping(bot_token: &str, chat_id: i64) -> Result<()> {
    send_message(bot_token, chat_id, "🏓 Pong! RunDock daemon is running.").await
}

// @group BusinessLogic > Commands : /help — list available commands
pub async fn cmd_help(bot_token: &str, chat_id: i64) -> Result<()> {
    let text = concat!(
        "🤖 <b>RunDock Bot Commands</b>\n\n",
        "/list — list all processes\n",
        "/status &lt;name&gt; — detailed info for a process\n",
        "/status ns &lt;ns&gt; — status of all processes in namespace\n",
        "/logs &lt;name&gt; [lines] — tail logs (default 20)\n\n",
        "<b>/start &lt;name&gt;</b> — start a process\n",
        "<b>/start ns &lt;ns&gt;</b> — start all in namespace\n\n",
        "<b>/stop &lt;name&gt;</b> — stop a process\n",
        "<b>/stop ns &lt;ns&gt;</b> — stop all in namespace\n\n",
        "<b>/restart &lt;name&gt;</b> — restart a process\n",
        "<b>/restart ns &lt;ns&gt;</b> — restart all in namespace\n\n",
        "/ping — check if daemon is alive\n",
        "/help — show this message"
    );
    send_message(bot_token, chat_id, text).await
}

// @group BusinessLogic > Commands : /list — show all processes grouped by namespace
pub async fn cmd_list(state: &Arc<DaemonState>, bot_token: &str, chat_id: i64) -> Result<()> {
    let mut processes = state.manager.list().await;

    if processes.is_empty() {
        return send_message(bot_token, chat_id, "No processes registered.").await;
    }

    // Sort by namespace then name for stable output
    processes.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));

    let mut lines = vec![];
    let mut current_ns: Option<&str> = None;

    for p in &processes {
        let ns = p.namespace.as_str();
        if current_ns != Some(ns) {
            if current_ns.is_some() {
                lines.push(String::new()); // blank line between namespaces
            }
            lines.push(format!("📁 <b>{}</b>", escape_html(ns)));
            current_ns = Some(ns);
        }
        let emoji = status_emoji(&p.status);
        let uptime = p
            .uptime_secs
            .map(format_uptime)
            .unwrap_or_else(|| "—".to_string());
        lines.push(format!(
            "  {emoji} <b>{}</b> · {} · ↺{} · ⏱{}",
            escape_html(&p.name),
            p.status,
            p.restart_count,
            uptime
        ));
    }

    send_message(bot_token, chat_id, &lines.join("\n")).await
}

// @group BusinessLogic > Commands : /status <name> — detailed single-process info
pub async fn cmd_status(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    name: &str,
) -> Result<()> {
    let processes = state.manager.list().await;
    let Some(p) = processes.iter().find(|p| p.name == name) else {
        return send_message(
            bot_token,
            chat_id,
            &format!("❌ No process named <b>{}</b>", escape_html(name)),
        )
        .await;
    };

    let emoji = status_emoji(&p.status);
    let uptime = p
        .uptime_secs
        .map(format_uptime)
        .unwrap_or_else(|| "—".to_string());
    let pid = p
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "—".to_string());
    let cpu = p
        .cpu_percent
        .map(|c| format!("{:.1}%", c))
        .unwrap_or_else(|| "—".to_string());
    let mem = p
        .memory_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "—".to_string());

    let text = format!(
        "{emoji} <b>{}</b>\nStatus: {}\nPID: {}\nUptime: {}\nRestarts: {}\nCPU: {}\nRAM: {}",
        escape_html(&p.name),
        p.status,
        pid,
        uptime,
        p.restart_count,
        cpu,
        mem,
    );
    send_message(bot_token, chat_id, &text).await
}

// @group BusinessLogic > Commands : /status ns <namespace> — status summary for all processes in a namespace
pub async fn cmd_status_namespace(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    namespace: &str,
) -> Result<()> {
    let mut processes = state.manager.list().await;
    processes.retain(|p| p.namespace == namespace);

    if processes.is_empty() {
        return send_message(
            bot_token,
            chat_id,
            &format!(
                "❌ No processes in namespace <b>{}</b>",
                escape_html(namespace)
            ),
        )
        .await;
    }

    processes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut lines = vec![format!("📁 <b>{}</b>\n", escape_html(namespace))];

    for p in &processes {
        let emoji = status_emoji(&p.status);
        let uptime = p
            .uptime_secs
            .map(format_uptime)
            .unwrap_or_else(|| "—".to_string());
        let pid = p
            .pid
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".to_string());
        let cpu = p
            .cpu_percent
            .map(|c| format!("{:.1}%", c))
            .unwrap_or_else(|| "—".to_string());
        let mem = p
            .memory_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "—".to_string());
        lines.push(format!(
            "{emoji} <b>{}</b>\nStatus: {} · PID: {} · ↺{}\nUptime: {} · CPU: {} · RAM: {}\n",
            escape_html(&p.name),
            p.status,
            pid,
            p.restart_count,
            uptime,
            cpu,
            mem,
        ));
    }

    send_message(bot_token, chat_id, &lines.join("\n")).await
}

// @group BusinessLogic > Commands : /start <name> — start a stopped process
pub async fn cmd_start(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    name: &str,
) -> Result<()> {
    let processes = state.manager.list().await;
    let Some(p) = processes.iter().find(|p| p.name == name) else {
        return send_message(
            bot_token,
            chat_id,
            &format!("❌ No process named <b>{}</b>", escape_html(name)),
        )
        .await;
    };

    let operation = {
        let _mutation_guard = state.state_mutation_lock.lock().await;
        match state.manager.start_existing(p.id).await {
            Ok(info) => match state.save_to_disk().await {
                Ok(()) => Ok(info),
                Err(error) => {
                    let rollback = state.manager.stop(p.id).await;
                    let rollback_save = state.save_to_disk().await;
                    match (rollback, rollback_save) {
                        (Ok(_), Ok(())) => Err(anyhow::anyhow!(
                            "start was not persisted and was rolled back: {error}"
                        )),
                        (rollback, rollback_save) => Err(anyhow::anyhow!(
                            "start was not persisted ({error}); rollback failed: runtime={rollback:?}, persistence={rollback_save:?}"
                        )),
                    }
                }
            },
            Err(error) => Err(error),
        }
    };
    match operation {
        Ok(_) => {
            send_message(
                bot_token,
                chat_id,
                &format!("✅ Started <b>{}</b>", escape_html(name)),
            )
            .await
        }
        Err(e) => {
            send_message(
                bot_token,
                chat_id,
                &format!(
                    "❌ Failed to start <b>{}</b>: {}",
                    escape_html(name),
                    escape_html(&e.to_string())
                ),
            )
            .await
        }
    }
}

// @group BusinessLogic > Commands : /stop <name> — stop a running process
pub async fn cmd_stop(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    name: &str,
) -> Result<()> {
    let processes = state.manager.list().await;
    let Some(p) = processes.iter().find(|p| p.name == name) else {
        return send_message(
            bot_token,
            chat_id,
            &format!("❌ No process named <b>{}</b>", escape_html(name)),
        )
        .await;
    };

    let operation = {
        let _mutation_guard = state.state_mutation_lock.lock().await;
        match state.manager.stop(p.id).await {
            Ok(info) => match state.save_to_disk().await {
                Ok(()) => Ok(info),
                Err(error) => {
                    let rollback = state.manager.start_existing(p.id).await;
                    let rollback_save = state.save_to_disk().await;
                    match (rollback, rollback_save) {
                        (Ok(_), Ok(())) => Err(anyhow::anyhow!(
                            "stop was not persisted and was rolled back: {error}"
                        )),
                        (rollback, rollback_save) => Err(anyhow::anyhow!(
                            "stop was not persisted ({error}); rollback failed: runtime={rollback:?}, persistence={rollback_save:?}"
                        )),
                    }
                }
            },
            Err(error) => Err(error),
        }
    };
    match operation {
        Ok(_) => {
            send_message(
                bot_token,
                chat_id,
                &format!("🛑 Stopped <b>{}</b>", escape_html(name)),
            )
            .await
        }
        Err(e) => {
            send_message(
                bot_token,
                chat_id,
                &format!(
                    "❌ Failed to stop <b>{}</b>: {}",
                    escape_html(name),
                    escape_html(&e.to_string())
                ),
            )
            .await
        }
    }
}

// @group BusinessLogic > Commands : /restart <name> — restart a process
pub async fn cmd_restart(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    name: &str,
) -> Result<()> {
    let processes = state.manager.list().await;
    let Some(p) = processes.iter().find(|p| p.name == name) else {
        return send_message(
            bot_token,
            chat_id,
            &format!("❌ No process named <b>{}</b>", escape_html(name)),
        )
        .await;
    };

    let operation = {
        let _mutation_guard = state.state_mutation_lock.lock().await;
        let before = state.manager.get(p.id).await;
        match state.manager.restart(p.id).await {
            Ok(info) => match state.save_to_disk().await {
                Ok(()) => Ok(info),
                Err(error) => {
                    let runtime_rollback = state.manager.stop(p.id).await;
                    let counter_rollback = match before {
                        Ok(ref previous) => {
                            state
                                .manager
                                .set_restart_count(p.id, previous.restart_count)
                                .await
                        }
                        Err(ref previous_error) => Err(anyhow::anyhow!(previous_error.to_string())),
                    };
                    let persistence_rollback = state.save_to_disk().await;
                    Err(anyhow::anyhow!(
                        "restart could not be persisted ({error}); process was stopped to preserve consistency: runtime={runtime_rollback:?}, counter={counter_rollback:?}, persistence={persistence_rollback:?}"
                    ))
                }
            },
            Err(error) => Err(error),
        }
    };
    match operation {
        Ok(_) => {
            send_message(
                bot_token,
                chat_id,
                &format!("🔄 Restarted <b>{}</b>", escape_html(name)),
            )
            .await
        }
        Err(e) => {
            send_message(
                bot_token,
                chat_id,
                &format!(
                    "❌ Failed to restart <b>{}</b>: {}",
                    escape_html(name),
                    escape_html(&e.to_string())
                ),
            )
            .await
        }
    }
}

// @group BusinessLogic > Commands : /startns <namespace> — start all stopped/crashed processes in a namespace
pub async fn cmd_start_namespace(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    namespace: &str,
) -> Result<()> {
    let mutation_guard = state.state_mutation_lock.lock().await;
    let before = telegram_namespace_baseline(state, namespace).await;
    let affected = state.manager.start_namespace(namespace).await;
    if affected.attempted == 0 {
        drop(mutation_guard);
        return send_message(
            bot_token,
            chat_id,
            &format!(
                "⚠️ No stopped/crashed processes found in namespace <b>{}</b>",
                escape_html(namespace)
            ),
        )
        .await;
    }

    let persistence_error =
        persist_or_rollback_bulk(state, &affected, &before, BulkRollbackMode::RestoreActivity)
            .await;
    let notification_store = if persistence_error.is_none() {
        Some(state.notifications.read().await.clone())
    } else {
        None
    };
    drop(mutation_guard);
    // External delivery happens after the state transaction lock is released.
    if let Some(store) = notification_store {
        fire_namespace_event(
            &store,
            namespace,
            &affected.processes,
            ProcessEvent::Started,
        )
        .await;
    }

    // Wait for processes to settle, then re-query for accurate status
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    let current = state.manager.list().await;
    let names: Vec<String> = affected
        .processes
        .iter()
        .map(|p| {
            let status = current
                .iter()
                .find(|c| c.id == p.id)
                .map(|c| format!(" · {}", c.status))
                .unwrap_or_default();
            format!("  • <b>{}</b>{}", escape_html(&p.name), status)
        })
        .collect();

    send_message(
        bot_token,
        chat_id,
        &format!(
            "{} Started {} of {} process{} in namespace <b>{}</b>:\n{}{}",
            if affected.failures.is_empty() && persistence_error.is_none() {
                "✅"
            } else {
                "⚠️"
            },
            affected.processes.len(),
            affected.attempted,
            if affected.attempted == 1 { "" } else { "es" },
            escape_html(namespace),
            names.join("\n"),
            bulk_issue_suffix(&affected.failures, persistence_error.as_ref())
        ),
    )
    .await
}

// @group BusinessLogic > Commands : /stopns <namespace> — stop all running processes in a namespace
pub async fn cmd_stop_namespace(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    namespace: &str,
) -> Result<()> {
    let mutation_guard = state.state_mutation_lock.lock().await;
    let before = telegram_namespace_baseline(state, namespace).await;
    let affected = state.manager.stop_namespace(namespace).await;
    if affected.attempted == 0 {
        drop(mutation_guard);
        send_message(
            bot_token,
            chat_id,
            &format!(
                "⚠️ No running processes found in namespace <b>{}</b>",
                escape_html(namespace)
            ),
        )
        .await
    } else {
        let persistence_error =
            persist_or_rollback_bulk(state, &affected, &before, BulkRollbackMode::RestoreActivity)
                .await;
        let notification_store = if persistence_error.is_none() {
            Some(state.notifications.read().await.clone())
        } else {
            None
        };
        drop(mutation_guard);
        // External delivery happens after the state transaction lock is released.
        if let Some(store) = notification_store {
            fire_namespace_event(
                &store,
                namespace,
                &affected.processes,
                ProcessEvent::Stopped,
            )
            .await;
        }
        let names: Vec<String> = affected
            .processes
            .iter()
            .map(|p| format!("  • <b>{}</b>", escape_html(&p.name)))
            .collect();
        send_message(
            bot_token,
            chat_id,
            &format!(
                "{} Stopped {} of {} process{} in namespace <b>{}</b>:\n{}{}",
                if affected.failures.is_empty() && persistence_error.is_none() {
                    "🛑"
                } else {
                    "⚠️"
                },
                affected.processes.len(),
                affected.attempted,
                if affected.attempted == 1 { "" } else { "es" },
                escape_html(namespace),
                names.join("\n"),
                bulk_issue_suffix(&affected.failures, persistence_error.as_ref())
            ),
        )
        .await
    }
}

// @group BusinessLogic > Commands : /restartns <namespace> — restart all processes in a namespace
pub async fn cmd_restart_namespace(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    namespace: &str,
) -> Result<()> {
    let mutation_guard = state.state_mutation_lock.lock().await;
    let before = telegram_namespace_baseline(state, namespace).await;
    let affected = state.manager.restart_namespace(namespace).await;
    if affected.attempted == 0 {
        drop(mutation_guard);
        return send_message(
            bot_token,
            chat_id,
            &format!(
                "⚠️ No processes found in namespace <b>{}</b>",
                escape_html(namespace)
            ),
        )
        .await;
    }

    let persistence_error =
        persist_or_rollback_bulk(state, &affected, &before, BulkRollbackMode::StopRestarted).await;
    let notification_store = if persistence_error.is_none() {
        Some(state.notifications.read().await.clone())
    } else {
        None
    };
    drop(mutation_guard);
    // External delivery happens after the state transaction lock is released.
    if let Some(store) = notification_store {
        fire_namespace_event(
            &store,
            namespace,
            &affected.processes,
            ProcessEvent::Restarted,
        )
        .await;
    }

    // Wait for processes to settle after stop+start, then re-query for accurate status
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    let current = state.manager.list().await;
    let names: Vec<String> = affected
        .processes
        .iter()
        .map(|p| {
            let status = current
                .iter()
                .find(|c| c.id == p.id)
                .map(|c| format!(" · {}", c.status))
                .unwrap_or_default();
            format!("  • <b>{}</b>{}", escape_html(&p.name), status)
        })
        .collect();

    send_message(
        bot_token,
        chat_id,
        &format!(
            "{} Restarted {} of {} process{} in namespace <b>{}</b>:\n{}{}",
            if affected.failures.is_empty() && persistence_error.is_none() {
                "🔄"
            } else {
                "⚠️"
            },
            affected.processes.len(),
            affected.attempted,
            if affected.attempted == 1 { "" } else { "es" },
            escape_html(namespace),
            names.join("\n"),
            bulk_issue_suffix(&affected.failures, persistence_error.as_ref())
        ),
    )
    .await
}

#[derive(Clone, Copy)]
enum BulkRollbackMode {
    RestoreActivity,
    StopRestarted,
}

async fn telegram_namespace_baseline(
    state: &DaemonState,
    namespace: &str,
) -> HashMap<Uuid, (bool, u32)> {
    state
        .manager
        .list()
        .await
        .into_iter()
        .filter(|process| process.namespace == namespace)
        .map(|process| {
            let active = matches!(
                process.status,
                ProcessStatus::Starting
                    | ProcessStatus::Running
                    | ProcessStatus::Watching
                    | ProcessStatus::Sleeping
            );
            (process.id, (active, process.restart_count))
        })
        .collect()
}

async fn persist_or_rollback_bulk(
    state: &DaemonState,
    affected: &crate::process::manager::BulkProcessResult,
    before: &HashMap<Uuid, (bool, u32)>,
    mode: BulkRollbackMode,
) -> Option<anyhow::Error> {
    let error = state.save_to_disk().await.err()?;
    let mut rollback_errors = Vec::new();
    for process in &affected.processes {
        let Some((was_active, restart_count)) = before.get(&process.id).copied() else {
            continue;
        };
        let current = match state.manager.get(process.id).await {
            Ok(current) => current,
            Err(rollback_error) => {
                rollback_errors.push(format!("{}: {rollback_error}", process.name));
                continue;
            }
        };
        let currently_active = matches!(
            current.status,
            ProcessStatus::Starting
                | ProcessStatus::Running
                | ProcessStatus::Watching
                | ProcessStatus::Sleeping
        );
        let runtime_rollback = match mode {
            BulkRollbackMode::StopRestarted if currently_active => {
                state.manager.stop(process.id).await.map(|_| ())
            }
            BulkRollbackMode::RestoreActivity if was_active && !currently_active => {
                state.manager.start_existing(process.id).await.map(|_| ())
            }
            BulkRollbackMode::RestoreActivity if !was_active && currently_active => {
                state.manager.stop(process.id).await.map(|_| ())
            }
            _ => Ok(()),
        };
        if let Err(rollback_error) = runtime_rollback {
            rollback_errors.push(format!("{}: {rollback_error}", process.name));
        }
        if let Err(rollback_error) = state
            .manager
            .set_restart_count(process.id, restart_count)
            .await
        {
            rollback_errors.push(format!("{} counter: {rollback_error}", process.name));
        }
    }
    if let Err(rollback_error) = state.save_to_disk().await {
        rollback_errors.push(format!("rollback persistence: {rollback_error}"));
    }
    Some(anyhow::anyhow!(if rollback_errors.is_empty() {
        format!("{error}; runtime rollback completed")
    } else {
        format!("{error}; rollback issues: {}", rollback_errors.join("; "))
    }))
}

fn bulk_issue_suffix(
    failures: &[crate::process::manager::BulkProcessFailure],
    persistence_error: Option<&anyhow::Error>,
) -> String {
    let mut issues = Vec::new();
    if !failures.is_empty() {
        issues.push(format!("{} process action(s) failed", failures.len()));
    }
    if persistence_error.is_some() {
        issues.push("the new state could not be persisted".to_string());
    }
    if issues.is_empty() {
        String::new()
    } else {
        format!("\n⚠️ {}.", issues.join("; "))
    }
}

// @group BusinessLogic > Commands : /logs <name> [N] — tail last N log lines
pub async fn cmd_logs(
    state: &Arc<DaemonState>,
    bot_token: &str,
    chat_id: i64,
    name: &str,
    lines: usize,
) -> Result<()> {
    let lines = lines.clamp(1, 200);
    let processes = state.manager.list().await;
    let Some(p) = processes.iter().find(|p| p.name == name) else {
        return send_message(
            bot_token,
            chat_id,
            &format!("❌ No process named <b>{}</b>", escape_html(name)),
        )
        .await;
    };

    let log_dir = crate::config::paths::process_log_dir(&p.name);
    let merged = match read_merged_logs(&log_dir, lines) {
        Ok(l) => l,
        Err(e) => {
            return send_message(
                bot_token,
                chat_id,
                &format!("❌ Could not read logs: {}", escape_html(&e.to_string())),
            )
            .await;
        }
    };

    if merged.is_empty() {
        return send_message(
            bot_token,
            chat_id,
            &format!("📭 No logs yet for <b>{}</b>", escape_html(name)),
        )
        .await;
    }

    // Format: stream [timestamp] content — keep it concise for Telegram
    let log_text: Vec<String> = merged
        .iter()
        .map(|(stream, _ts, content)| {
            let prefix = if stream == "stderr" { "ERR" } else { "OUT" };
            format!("[{prefix}] {content}")
        })
        .collect();

    let header = format!(
        "📋 <b>{}</b> — last {} lines:\n",
        escape_html(name),
        merged.len()
    );
    let body = log_text.join("\n");
    let truncation_notice = "\n<i>(truncated)</i>";
    let closing_tag = "</code>";
    let body_budget = 4_000usize.saturating_sub(
        header.len() + "<code>".len() + closing_tag.len() + truncation_notice.len(),
    );
    let (escaped_body, truncated) = escape_html_bounded(&body, body_budget);
    let notice = if truncated { truncation_notice } else { "" };
    let message = format!("{header}<code>{escaped_body}{closing_tag}{notice}");

    send_message(bot_token, chat_id, &message).await
}

// @group Utilities : Map ProcessStatus to an emoji indicator
fn status_emoji(status: &ProcessStatus) -> &'static str {
    match status {
        ProcessStatus::Running => "🟢",
        ProcessStatus::Watching => "👁",
        ProcessStatus::Sleeping => "😴",
        ProcessStatus::Stopped => "🔴",
        ProcessStatus::Crashed => "💥",
        ProcessStatus::Errored => "❌",
        ProcessStatus::Starting => "🔵",
        ProcessStatus::Stopping => "🟡",
    }
}

// @group Utilities : Format uptime in seconds to human-readable string
fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{}h{}m", h, m)
    }
}

// @group Utilities : Format bytes to human-readable string
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// @group Utilities : Escape HTML special characters for Telegram HTML parse mode
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_bounded(value: &str, max_bytes: usize) -> (String, bool) {
    let mut output = String::new();
    for character in value.chars() {
        let mut buffer = [0u8; 4];
        let escaped = match character {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => character.encode_utf8(&mut buffer),
        };
        if output.len().saturating_add(escaped.len()) > max_bytes {
            return (output, true);
        }
        output.push_str(escaped);
    }
    (output, false)
}

async fn send_notification_fanout(
    token: &str,
    chat_ids: &[i64],
    message: &str,
    notification_kind: &str,
) {
    let mut deliveries = chat_ids
        .iter()
        .copied()
        .map(|chat_id| async move { (chat_id, send_message(token, chat_id, message).await) })
        .collect::<FuturesUnordered<_>>();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    while !deliveries.is_empty() {
        match tokio::time::timeout_at(deadline, deliveries.next()).await {
            Ok(Some((_, Ok(())))) => {}
            Ok(Some((chat_id, Err(error)))) => {
                tracing::warn!("telegram: failed to send {notification_kind} to {chat_id}: {error}")
            }
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(
                    "telegram: {} {notification_kind} delivery(s) exceeded the 30-second fanout deadline",
                    deliveries.len()
                );
                break;
            }
        }
    }
}

async fn load_notification_config() -> anyhow::Result<crate::config::telegram_config::TelegramConfig>
{
    tokio::task::spawn_blocking(crate::config::telegram_config::load)
        .await
        .map_err(anyhow::Error::from)?
}

// @group BusinessLogic > Notifications : Fire a single Telegram notification for a bulk namespace operation.
// Sends one message listing all affected processes instead of one per process.
pub async fn fire_telegram_namespace_notification(
    namespace: &str,
    event: ProcessEvent,
    processes: &[ProcessInfo],
) {
    if processes.is_empty() {
        return;
    }

    let cfg = match load_notification_config().await {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("telegram config is unreadable; notification suppressed: {error}");
            return;
        }
    };
    if !cfg.enabled {
        return;
    }
    let token = match cfg.bot_token {
        Some(ref t) => t.clone(),
        None => return,
    };

    let should_send = match event {
        ProcessEvent::Crashed | ProcessEvent::CronFailed | ProcessEvent::Unhealthy => {
            cfg.notify_on_crash
        }
        ProcessEvent::Started | ProcessEvent::CronRun | ProcessEvent::HealthRecovered => {
            cfg.notify_on_start
        }
        ProcessEvent::Stopped => cfg.notify_on_stop,
        ProcessEvent::Restarted => cfg.notify_on_restart,
    };

    if !should_send || cfg.allowed_chat_ids.is_empty() {
        return;
    }

    let (emoji, verb) = match event {
        ProcessEvent::Started => ("🟢", "started"),
        ProcessEvent::Stopped => ("⚪", "stopped"),
        ProcessEvent::Restarted => ("🔄", "restarted"),
        ProcessEvent::Crashed => ("💥", "crashed"),
        ProcessEvent::CronRun => ("⏰", "cron started"),
        ProcessEvent::CronFailed => ("❌", "cron failed"),
        ProcessEvent::Unhealthy => ("⚠️", "became unhealthy"),
        ProcessEvent::HealthRecovered => ("💚", "health recovered"),
    };

    const MAX_NAMESPACE_PROCESSES: usize = 64;
    const MAX_NOTIFICATION_NAME_BYTES: usize = 256;
    let (mut ns, namespace_truncated) = escape_html_bounded(namespace, MAX_NOTIFICATION_NAME_BYTES);
    if namespace_truncated {
        ns.push('…');
    }
    let count = processes.len();
    let header = format!(
        "{emoji} <b>Namespace: {ns}</b> — {count} process{} {verb}",
        if count == 1 { "" } else { "es" }
    );

    let items: Vec<String> = processes
        .iter()
        .take(MAX_NAMESPACE_PROCESSES)
        .map(|p| {
            let pid_str = p.pid.map(|pid| format!(" · PID {pid}")).unwrap_or_default();
            let (name, truncated) = escape_html_bounded(&p.name, MAX_NOTIFICATION_NAME_BYTES);
            format!(
                "  • <b>{}{}</b>{}",
                name,
                if truncated { "…" } else { "" },
                pid_str
            )
        })
        .collect();
    let omitted = count.saturating_sub(items.len());
    let msg = format!(
        "{}\n{}{}",
        header,
        items.join("\n"),
        if omitted > 0 {
            format!("\n  … and {omitted} more processes")
        } else {
            String::new()
        }
    );

    send_notification_fanout(
        &token,
        &cfg.allowed_chat_ids,
        &msg,
        "namespace notification",
    )
    .await;
}

// @group BusinessLogic > Notifications : Fire a Telegram push notification for a process event.
// Reads config from disk on each call — cheap for infrequent events.
pub async fn fire_telegram_notification(proc: &ProcessInfo, event: ProcessEvent) {
    let cfg = match load_notification_config().await {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("telegram config is unreadable; notification suppressed: {error}");
            return;
        }
    };

    if !cfg.enabled {
        return;
    }

    let token = match cfg.bot_token {
        Some(ref t) => t.clone(),
        None => return,
    };

    let should_send = match event {
        ProcessEvent::Crashed | ProcessEvent::CronFailed | ProcessEvent::Unhealthy => {
            cfg.notify_on_crash
        }
        ProcessEvent::Started | ProcessEvent::CronRun | ProcessEvent::HealthRecovered => {
            cfg.notify_on_start
        }
        ProcessEvent::Stopped => cfg.notify_on_stop,
        ProcessEvent::Restarted => cfg.notify_on_restart,
    };

    if !should_send || cfg.allowed_chat_ids.is_empty() {
        return;
    }

    let name = escape_html(&proc.name);
    let msg = match event {
        ProcessEvent::Crashed => format!(
            "🔴 <b>{name}</b> crashed\nRestarts: {}\nExit code: {}",
            proc.restart_count,
            proc.last_exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".to_string())
        ),
        ProcessEvent::Started => format!(
            "🟢 <b>{name}</b> started\nPID: {}",
            proc.pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "—".to_string())
        ),
        ProcessEvent::Stopped => format!("⚪ <b>{name}</b> stopped"),
        ProcessEvent::Restarted => format!(
            "🔄 <b>{name}</b> restarted (#{} restart)",
            proc.restart_count
        ),
        ProcessEvent::CronRun => format!("⏰ <b>{name}</b> cron job started"),
        ProcessEvent::CronFailed => format!(
            "❌ <b>{name}</b> cron job failed\nExit code: {}",
            proc.last_exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".to_string())
        ),
        ProcessEvent::Unhealthy => format!("⚠️ <b>{name}</b> health check failed"),
        ProcessEvent::HealthRecovered => {
            format!("💚 <b>{name}</b> health check recovered")
        }
    };

    send_notification_fanout(&token, &cfg.allowed_chat_ids, &msg, "process notification").await;
}

// @group BusinessLogic > LogAlertTelegram : Send a log-spike alert to all allowed Telegram chats
pub async fn fire_log_alert_telegram(process_name: &str, stderr_count: u64, threshold: u64) {
    let cfg = match load_notification_config().await {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("telegram config is unreadable; log alert suppressed: {error}");
            return;
        }
    };
    if !cfg.enabled || cfg.allowed_chat_ids.is_empty() {
        return;
    }
    let token = match cfg.bot_token {
        Some(ref t) => t.clone(),
        None => return,
    };

    let name = escape_html(process_name);
    let msg = format!(
        "⚠️ <b>{name}</b> — log spike detected\n\
         Stderr lines: <b>{stderr_count}</b> in the last 5 min\n\
         Threshold: {threshold}"
    );

    send_notification_fanout(&token, &cfg.allowed_chat_ids, &msg, "log alert").await;
}

#[cfg(test)]
mod message_tests {
    use super::*;

    #[test]
    fn bounded_html_preserves_utf8_and_complete_entities() {
        let (value, truncated) = escape_html_bounded("进程<&输出", 10);
        assert!(truncated);
        assert_eq!(value, "进程&lt;");
        assert!(std::str::from_utf8(value.as_bytes()).is_ok());
    }

    #[test]
    fn long_multiline_messages_are_paginated_without_losing_lines() {
        let input = (0..200)
            .map(|index| format!("<b>process-{index}</b> {}\n", "x".repeat(30)))
            .collect::<String>();
        let pages = paginate_message(&input).unwrap();
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| page.len() <= 4_000));
        assert_eq!(pages.concat().replace("</b><b>", ""), input);
    }

    #[test]
    fn oversized_utf8_line_is_split_without_data_loss() {
        let input = "进".repeat(2_000);
        let pages = paginate_message(&input).unwrap();
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| page.len() <= 4_000));
        assert_eq!(pages.concat(), input);
    }

    #[test]
    fn pagination_closes_and_reopens_html_formatting() {
        let input = format!("<code>{}</code>", "x".repeat(8_000));
        let pages = paginate_message(&input).unwrap();
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| {
            page.len() <= 4_000 && page.starts_with("<code>") && page.ends_with("</code>")
        }));
        assert_eq!(pages.concat().replace("</code><code>", ""), input);
    }
}
