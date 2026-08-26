// @group BusinessLogic : Notification sender — dispatches webhook / Slack / Teams payloads

use crate::config::notification_store::NotificationsStore;
use crate::models::notification::{
    DiscordTarget, NotificationConfig, SlackTarget, TeamsTarget, WebhookTarget,
};
use crate::models::process_info::ProcessInfo;
use chrono::Utc;
use futures::{stream::FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

fn notification_event_limit() -> &'static Arc<tokio::sync::Semaphore> {
    static LIMIT: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    LIMIT.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(16)))
}

async fn post_json_safely(raw_url: &str, payload: &Value) -> anyhow::Result<()> {
    use crate::utils::outbound::{client_for_url, validate_url, OutboundPolicy};

    let url = validate_url(raw_url, OutboundPolicy::PublicHttps)?;
    let client = client_for_url(&url, OutboundPolicy::PublicHttps).await?;
    let response = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("request failed: {}", error.without_url()))?;
    if !response.status().is_success() {
        anyhow::bail!("endpoint returned HTTP {}", response.status());
    }
    Ok(())
}

// @group Types > ProcessEvent : Lifecycle event that can trigger a notification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessEvent {
    // Process lifecycle
    Started,
    Stopped,
    Crashed,
    Restarted,
    Unhealthy,
    HealthRecovered,
    // Cron lifecycle
    CronRun,
    CronFailed,
}

#[derive(Debug, Default)]
pub struct DeliveryReport {
    pub attempted: usize,
    pub delivered: usize,
    pub errors: Vec<String>,
}

impl ProcessEvent {
    pub fn label(self) -> &'static str {
        match self {
            ProcessEvent::Started => "started",
            ProcessEvent::Stopped => "stopped",
            ProcessEvent::Crashed => "crashed",
            ProcessEvent::Restarted => "restarted",
            ProcessEvent::Unhealthy => "unhealthy",
            ProcessEvent::HealthRecovered => "health_recovered",
            ProcessEvent::CronRun => "cron_run",
            ProcessEvent::CronFailed => "cron_failed",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            ProcessEvent::Started => "🟢",
            ProcessEvent::Stopped => "⚪",
            ProcessEvent::Crashed => "🔴",
            ProcessEvent::Restarted => "🔄",
            ProcessEvent::Unhealthy => "⚠️",
            ProcessEvent::HealthRecovered => "💚",
            ProcessEvent::CronRun => "⏰",
            ProcessEvent::CronFailed => "❌",
        }
    }
}

// @group BusinessLogic > FireLogAlert : Send a log-spike alert through all configured global channels
pub async fn fire_log_alert(
    store: &NotificationsStore,
    proc: &ProcessInfo,
    stderr_count: u64,
    threshold: u64,
) {
    let _event_permit = match notification_event_limit().clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(process = %proc.name, "log-alert delivery capacity is busy; event dropped");
            return;
        }
    };
    let effective = store.global.clone();
    type AlertDeliveryFuture =
        Pin<Box<dyn Future<Output = (&'static str, anyhow::Result<()>)> + Send>>;
    let mut deliveries: FuturesUnordered<AlertDeliveryFuture> = FuturesUnordered::new();

    if let Some(wh) = &effective.webhook {
        if wh.enabled && !wh.url.is_empty() {
            let payload = serde_json::json!({
                "event": "log_alert",
                "timestamp": Utc::now().to_rfc3339(),
                "process": { "id": proc.id, "name": proc.name, "namespace": proc.namespace },
                "stderr_count": stderr_count,
                "threshold": threshold,
            });
            let url = wh.url.clone();
            deliveries.push(Box::pin(async move {
                ("webhook", post_json_safely(&url, &payload).await)
            }));
        }
    }

    if let Some(sl) = &effective.slack {
        if sl.enabled && !sl.webhook_url.is_empty() {
            let text = format!(
                "⚠️ *{}* — {} stderr lines in the last 5 min (threshold: {})",
                proc.name, stderr_count, threshold
            );
            let mut payload = serde_json::json!({
                "text": text,
                "attachments": [{
                    "color": "#ef4444",
                    "fields": [
                        { "title": "Process",    "value": &proc.name,      "short": true },
                        { "title": "Namespace",  "value": &proc.namespace, "short": true },
                        { "title": "Stderr",     "value": stderr_count.to_string(), "short": true },
                        { "title": "Threshold",  "value": threshold.to_string(),    "short": true },
                    ],
                    "footer": "RunDock · log alert",
                    "ts": Utc::now().timestamp(),
                }]
            });
            if let Some(ch) = &sl.channel {
                if !ch.is_empty() {
                    payload["channel"] = serde_json::Value::String(ch.clone());
                }
            }
            let url = sl.webhook_url.clone();
            deliveries.push(Box::pin(async move {
                ("Slack", post_json_safely(&url, &payload).await)
            }));
        }
    }

    if let Some(tm) = &effective.teams {
        if tm.enabled && !tm.webhook_url.is_empty() {
            let payload = serde_json::json!({
                "@type": "MessageCard",
                "@context": "http://schema.org/extensions",
                "summary": format!("{} — log alert", proc.name),
                "themeColor": "ef4444",
                "title": format!("⚠️ {} — log spike detected", proc.name),
                "sections": [{ "facts": [
                    { "name": "Process",    "value": &proc.name },
                    { "name": "Namespace",  "value": &proc.namespace },
                    { "name": "Stderr count", "value": stderr_count.to_string() },
                    { "name": "Threshold",  "value": threshold.to_string() },
                    { "name": "Timestamp",  "value": Utc::now().to_rfc3339() },
                ]}]
            });
            let url = tm.webhook_url.clone();
            deliveries.push(Box::pin(async move {
                ("Teams", post_json_safely(&url, &payload).await)
            }));
        }
    }

    if let Some(dc) = &effective.discord {
        if dc.enabled && !dc.webhook_url.is_empty() {
            let payload = serde_json::json!({
                "embeds": [{
                    "title": format!("⚠️ {} — log spike detected", proc.name),
                    "color": 15624260u32,  // #ef4444
                    "fields": [
                        { "name": "Process",    "value": &proc.name,              "inline": true },
                        { "name": "Namespace",  "value": &proc.namespace,         "inline": true },
                        { "name": "Stderr",     "value": stderr_count.to_string(), "inline": true },
                        { "name": "Threshold",  "value": threshold.to_string(),   "inline": true },
                    ],
                    "footer": { "text": "RunDock · log alert" },
                    "timestamp": Utc::now().to_rfc3339(),
                }]
            });
            let url = dc.webhook_url.clone();
            deliveries.push(Box::pin(async move {
                ("Discord", post_json_safely(&url, &payload).await)
            }));
        }
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while !deliveries.is_empty() {
        match tokio::time::timeout_at(deadline, deliveries.next()).await {
            Ok(Some((_, Ok(())))) => {}
            Ok(Some((channel, Err(error)))) => {
                tracing::warn!(channel, %error, "log-alert delivery failed");
            }
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(
                    pending = deliveries.len(),
                    "log-alert delivery exceeded 15-second deadline"
                );
                break;
            }
        }
    }
}

// @group BusinessLogic > FireNamespaceEvent : Fire one summary notification for a bulk namespace operation
pub async fn fire_namespace_event(
    store: &NotificationsStore,
    namespace: &str,
    processes: &[ProcessInfo],
    event: ProcessEvent,
) {
    if processes.is_empty() {
        return;
    }

    let ns_config = store.namespaces.get(namespace);
    let effective = merge_configs(None, ns_config, Some(&store.global));

    let should_fire = match event {
        ProcessEvent::Started => effective.events.on_start,
        ProcessEvent::Stopped => effective.events.on_stop,
        ProcessEvent::Crashed => effective.events.on_crash,
        ProcessEvent::Restarted => effective.events.on_restart,
        ProcessEvent::Unhealthy => effective.events.on_unhealthy,
        ProcessEvent::HealthRecovered => effective.events.on_health_recovered,
        ProcessEvent::CronRun => effective.events.on_cron_run,
        ProcessEvent::CronFailed => effective.events.on_cron_fail,
    };

    if !should_fire {
        return;
    }

    let _event_permit = match notification_event_limit().clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(
                namespace,
                "namespace notification capacity is busy; event dropped"
            );
            return;
        }
    };

    type NamespaceDeliveryFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
    let mut deliveries: FuturesUnordered<NamespaceDeliveryFuture<'_>> = FuturesUnordered::new();
    if let Some(wh) = &effective.webhook {
        if wh.enabled && !wh.url.is_empty() {
            deliveries.push(Box::pin(send_namespace_webhook(
                wh, namespace, processes, event,
            )));
        }
    }
    if let Some(sl) = &effective.slack {
        if sl.enabled && !sl.webhook_url.is_empty() {
            deliveries.push(Box::pin(send_namespace_slack(
                sl, namespace, processes, event,
            )));
        }
    }
    if let Some(tm) = &effective.teams {
        if tm.enabled && !tm.webhook_url.is_empty() {
            deliveries.push(Box::pin(send_namespace_teams(
                tm, namespace, processes, event,
            )));
        }
    }
    if let Some(dc) = &effective.discord {
        if dc.enabled && !dc.webhook_url.is_empty() {
            deliveries.push(Box::pin(send_namespace_discord(
                dc, namespace, processes, event,
            )));
        }
    }
    if tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while deliveries.next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tracing::warn!(
            namespace,
            "namespace notification delivery exceeded 15-second deadline"
        );
    }
}

// @group BusinessLogic > FireEvent : Resolve effective config and dispatch all enabled channels
pub async fn fire_event_report(
    store: &NotificationsStore,
    proc: &ProcessInfo,
    event: ProcessEvent,
) -> DeliveryReport {
    // Cascade: process → namespace → global (first non-None wins per channel)
    let ns_config = store.namespaces.get(&proc.namespace);

    let effective = merge_configs(proc.notify.as_ref(), ns_config, Some(&store.global));

    // Check event flag
    let should_fire = match event {
        ProcessEvent::Started => effective.events.on_start,
        ProcessEvent::Stopped => effective.events.on_stop,
        ProcessEvent::Crashed => effective.events.on_crash,
        ProcessEvent::Restarted => effective.events.on_restart,
        ProcessEvent::Unhealthy => effective.events.on_unhealthy,
        ProcessEvent::HealthRecovered => effective.events.on_health_recovered,
        ProcessEvent::CronRun => effective.events.on_cron_run,
        ProcessEvent::CronFailed => effective.events.on_cron_fail,
    };

    if !should_fire {
        return DeliveryReport::default();
    }

    let _event_permit = match notification_event_limit().clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return DeliveryReport {
                attempted: 0,
                delivered: 0,
                errors: vec!["notification delivery capacity is busy; event was dropped".into()],
            };
        }
    };

    type DeliveryFuture<'a> =
        Pin<Box<dyn Future<Output = (&'static str, anyhow::Result<()>)> + Send + 'a>>;

    let mut report = DeliveryReport::default();
    let mut deliveries: FuturesUnordered<DeliveryFuture<'_>> = FuturesUnordered::new();
    if let Some(wh) = &effective.webhook {
        if wh.enabled && !wh.url.is_empty() {
            report.attempted += 1;
            deliveries.push(Box::pin(async move {
                ("webhook", send_webhook(wh, proc, event).await)
            }));
        }
    }
    if let Some(sl) = &effective.slack {
        if sl.enabled && !sl.webhook_url.is_empty() {
            report.attempted += 1;
            deliveries.push(Box::pin(async move {
                ("Slack", send_slack(sl, proc, event).await)
            }));
        }
    }
    if let Some(tm) = &effective.teams {
        if tm.enabled && !tm.webhook_url.is_empty() {
            report.attempted += 1;
            deliveries.push(Box::pin(async move {
                ("Teams", send_teams(tm, proc, event).await)
            }));
        }
    }
    if let Some(dc) = &effective.discord {
        if dc.enabled && !dc.webhook_url.is_empty() {
            report.attempted += 1;
            deliveries.push(Box::pin(async move {
                ("Discord", send_discord(dc, proc, event).await)
            }));
        }
    }

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while !deliveries.is_empty() {
        match tokio::time::timeout_at(deadline, deliveries.next()).await {
            Ok(Some((_, Ok(())))) => report.delivered += 1,
            Ok(Some((channel, Err(error)))) => {
                report.errors.push(format!("{channel}: {error}"));
            }
            Ok(None) => break,
            Err(_) => {
                report.errors.push(format!(
                    "{} notification channel(s) exceeded the 15-second delivery deadline",
                    deliveries.len()
                ));
                break;
            }
        }
    }
    report
}

pub async fn fire_event(store: &NotificationsStore, proc: &ProcessInfo, event: ProcessEvent) {
    let report = fire_event_report(store, proc, event).await;
    for error in report.errors {
        tracing::warn!(process = %proc.name, %error, "notification delivery failed");
    }
}

// @group BusinessLogic > MergeConfigs : Cascade process → namespace → global, first non-None wins per channel
fn merge_configs(
    process: Option<&NotificationConfig>,
    namespace: Option<&NotificationConfig>,
    global: Option<&NotificationConfig>,
) -> NotificationConfig {
    let sources: Vec<&NotificationConfig> = [process, namespace, global]
        .iter()
        .filter_map(|o| *o)
        .collect();

    // For events: use the most-specific explicit override. Legacy configs did
    // not persist events_override, so an existing non-empty event set remains
    // an implicit override for backwards compatibility.
    let events = sources
        .iter()
        .find(|c| {
            c.events_override
                || c.events.on_crash
                || c.events.on_restart
                || c.events.on_start
                || c.events.on_stop
                || c.events.on_unhealthy
                || c.events.on_health_recovered
                || c.events.on_cron_run
                || c.events.on_cron_fail
        })
        .map(|c| c.events.clone())
        .unwrap_or_default();

    let webhook = sources.iter().find_map(|c| c.webhook.clone());
    let slack = sources.iter().find_map(|c| c.slack.clone());
    let teams = sources.iter().find_map(|c| c.teams.clone());
    let discord = sources.iter().find_map(|c| c.discord.clone());

    NotificationConfig {
        webhook,
        slack,
        teams,
        discord,
        events,
        events_override: sources.iter().any(|config| config.events_override),
    }
}

// @group BusinessLogic > SendWebhook : POST generic JSON payload to webhook URL
async fn send_webhook(
    wh: &WebhookTarget,
    proc: &ProcessInfo,
    event: ProcessEvent,
) -> anyhow::Result<()> {
    let payload = json!({
        "event":     event.label(),
        "timestamp": Utc::now().to_rfc3339(),
        "process": {
            "id":        proc.id,
            "name":      proc.name,
            "namespace": proc.namespace,
            "status":    format!("{:?}", proc.status).to_lowercase(),
            "pid":       proc.pid,
            "restart_count": proc.restart_count,
        }
    });

    post_json_safely(&wh.url, &payload).await
}

// @group BusinessLogic > SendSlack : POST Slack-formatted message card
async fn send_slack(
    sl: &SlackTarget,
    proc: &ProcessInfo,
    event: ProcessEvent,
) -> anyhow::Result<()> {
    let color = match event {
        ProcessEvent::Started => "#36a64f",
        ProcessEvent::Stopped => "#aaaaaa",
        ProcessEvent::Crashed => "#ff0000",
        ProcessEvent::Restarted => "#f0ad4e",
        ProcessEvent::Unhealthy => "#ef4444",
        ProcessEvent::HealthRecovered => "#22c55e",
        ProcessEvent::CronRun => "#fbbf24",
        ProcessEvent::CronFailed => "#ef4444",
    };

    let text = format!("{} *{}* {}", event.emoji(), proc.name, event.label());

    let mut payload = json!({
        "text": text,
        "attachments": [{
            "color": color,
            "fields": [
                { "title": "Process",   "value": &proc.name,                              "short": true },
                { "title": "Namespace", "value": &proc.namespace,                         "short": true },
                { "title": "Event",     "value": event.label(),                           "short": true },
                { "title": "Status",    "value": format!("{:?}", proc.status).to_lowercase(), "short": true },
            ],
            "footer": "RunDock",
            "ts": Utc::now().timestamp(),
        }]
    });

    if let Some(channel) = &sl.channel {
        if !channel.is_empty() {
            payload["channel"] = Value::String(channel.clone());
        }
    }

    post_json_safely(&sl.webhook_url, &payload).await
}

// @group BusinessLogic > SendTeams : POST Microsoft Teams adaptive card
async fn send_teams(
    tm: &TeamsTarget,
    proc: &ProcessInfo,
    event: ProcessEvent,
) -> anyhow::Result<()> {
    let summary = format!("{} {} — RunDock", proc.name, event.label());

    let payload = json!({
        "@type":      "MessageCard",
        "@context":   "http://schema.org/extensions",
        "summary":    &summary,
        "themeColor": match event {
            ProcessEvent::Crashed    => "FF0000",
            ProcessEvent::Started    => "36a64f",
            ProcessEvent::Restarted  => "f0ad4e",
            ProcessEvent::Unhealthy  => "ef4444",
            ProcessEvent::HealthRecovered => "22c55e",
            ProcessEvent::Stopped    => "aaaaaa",
            ProcessEvent::CronRun    => "fbbf24",
            ProcessEvent::CronFailed => "ef4444",
        },
        "title": format!("{} {}", event.emoji(), &summary),
        "sections": [{
            "facts": [
                { "name": "Process",    "value": &proc.name },
                { "name": "Namespace",  "value": &proc.namespace },
                { "name": "Event",      "value": event.label() },
                { "name": "Status",     "value": format!("{:?}", proc.status).to_lowercase() },
                { "name": "Timestamp",  "value": Utc::now().to_rfc3339() },
            ]
        }]
    });

    post_json_safely(&tm.webhook_url, &payload).await
}

const MAX_NAMESPACE_PROCESS_NAMES: usize = 64;
const MAX_NAMESPACE_PROCESS_TEXT_BYTES: usize = 900;

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn bounded_namespace_process_names(processes: &[ProcessInfo]) -> (Vec<String>, usize) {
    let mut names = Vec::new();
    let mut used_bytes = 0usize;
    for process in processes.iter().take(MAX_NAMESPACE_PROCESS_NAMES) {
        let name = truncate_utf8(&process.name, 256);
        let separator_bytes = usize::from(!names.is_empty()) * 2;
        if used_bytes
            .saturating_add(separator_bytes)
            .saturating_add(name.len())
            > MAX_NAMESPACE_PROCESS_TEXT_BYTES
        {
            break;
        }
        used_bytes += separator_bytes + name.len();
        names.push(name);
    }
    let omitted = processes.len().saturating_sub(names.len());
    (names, omitted)
}

fn namespace_process_text(processes: &[ProcessInfo]) -> String {
    let (names, omitted) = bounded_namespace_process_names(processes);
    let mut text = names.join(", ");
    if omitted > 0 {
        text.push_str(&format!(" … (+{omitted} omitted)"));
    }
    text
}

// @group BusinessLogic > SendNamespaceWebhook : POST namespace-level JSON payload to webhook URL
async fn send_namespace_webhook(
    wh: &WebhookTarget,
    namespace: &str,
    processes: &[ProcessInfo],
    event: ProcessEvent,
) {
    let (names, omitted) = bounded_namespace_process_names(processes);
    let payload = json!({
        "event":     event.label(),
        "timestamp": Utc::now().to_rfc3339(),
        "namespace": namespace,
        "count":     processes.len(),
        "processes": names,
        "omitted_processes": omitted,
    });
    if let Err(e) = post_json_safely(&wh.url, &payload).await {
        tracing::warn!(
            "webhook namespace notification failed for ns '{}': {e}",
            namespace
        );
    }
}

// @group BusinessLogic > SendNamespaceSlack : POST Slack-formatted namespace summary card
async fn send_namespace_slack(
    sl: &SlackTarget,
    namespace: &str,
    processes: &[ProcessInfo],
    event: ProcessEvent,
) {
    let color = match event {
        ProcessEvent::Started => "#36a64f",
        ProcessEvent::Stopped => "#aaaaaa",
        ProcessEvent::Crashed => "#ff0000",
        ProcessEvent::Restarted => "#f0ad4e",
        ProcessEvent::Unhealthy => "#ef4444",
        ProcessEvent::HealthRecovered => "#22c55e",
        ProcessEvent::CronRun => "#fbbf24",
        ProcessEvent::CronFailed => "#ef4444",
    };
    let names = namespace_process_text(processes);
    let text = format!(
        "{} *{}* — {} process{} {}",
        event.emoji(),
        namespace,
        processes.len(),
        if processes.len() == 1 { "" } else { "es" },
        event.label(),
    );
    let mut payload = json!({
        "text": text,
        "attachments": [{
            "color": color,
            "fields": [
                { "title": "Namespace", "value": namespace,          "short": true },
                { "title": "Event",     "value": event.label(),      "short": true },
                { "title": "Processes", "value": names,              "short": false },
            ],
            "footer": "RunDock",
            "ts": Utc::now().timestamp(),
        }]
    });
    if let Some(channel) = &sl.channel {
        if !channel.is_empty() {
            payload["channel"] = Value::String(channel.clone());
        }
    }
    if let Err(e) = post_json_safely(&sl.webhook_url, &payload).await {
        tracing::warn!(
            "Slack namespace notification failed for ns '{}': {e}",
            namespace
        );
    }
}

// @group BusinessLogic > SendDiscord : POST Discord embed card via incoming webhook
async fn send_discord(
    dc: &DiscordTarget,
    proc: &ProcessInfo,
    event: ProcessEvent,
) -> anyhow::Result<()> {
    let color: u32 = match event {
        ProcessEvent::Started => 3580751,         // #36a64f green
        ProcessEvent::Stopped => 11184810,        // #aaaaaa gray
        ProcessEvent::Crashed => 16711680,        // #FF0000 red
        ProcessEvent::Restarted => 15774030,      // #f0ad4e orange
        ProcessEvent::Unhealthy => 15624260,      // #ef4444 red
        ProcessEvent::HealthRecovered => 2278750, // #22c55e green
        ProcessEvent::CronRun => 16498468,        // #fbbf24 yellow
        ProcessEvent::CronFailed => 15624260,     // #ef4444 red
    };

    let payload = json!({
        "embeds": [{
            "title": format!("{} {} — {}", event.emoji(), proc.name, event.label()),
            "color": color,
            "fields": [
                { "name": "Process",   "value": &proc.name,                                    "inline": true },
                { "name": "Namespace", "value": &proc.namespace,                               "inline": true },
                { "name": "Status",    "value": format!("{:?}", proc.status).to_lowercase(),   "inline": true },
                { "name": "Restarts",  "value": proc.restart_count.to_string(),                "inline": true },
            ],
            "footer": { "text": "RunDock" },
            "timestamp": Utc::now().to_rfc3339(),
        }]
    });

    post_json_safely(&dc.webhook_url, &payload).await
}

// @group BusinessLogic > SendNamespaceDiscord : POST Discord embed for bulk namespace operation
async fn send_namespace_discord(
    dc: &DiscordTarget,
    namespace: &str,
    processes: &[ProcessInfo],
    event: ProcessEvent,
) {
    let color: u32 = match event {
        ProcessEvent::Started => 3580751,
        ProcessEvent::Stopped => 11184810,
        ProcessEvent::Crashed => 16711680,
        ProcessEvent::Restarted => 15774030,
        ProcessEvent::Unhealthy => 15624260,
        ProcessEvent::HealthRecovered => 2278750,
        ProcessEvent::CronRun => 16498468,
        ProcessEvent::CronFailed => 15624260,
    };
    let names = namespace_process_text(processes);
    let payload = json!({
        "embeds": [{
            "title": format!("{} Namespace {} — {} {}", event.emoji(), namespace, processes.len(), event.label()),
            "color": color,
            "fields": [
                { "name": "Namespace", "value": namespace,        "inline": true },
                { "name": "Count",     "value": processes.len().to_string(), "inline": true },
                { "name": "Processes", "value": names,            "inline": false },
            ],
            "footer": { "text": "RunDock" },
            "timestamp": Utc::now().to_rfc3339(),
        }]
    });
    if let Err(e) = post_json_safely(&dc.webhook_url, &payload).await {
        tracing::warn!(
            "Discord namespace notification failed for ns '{}': {e}",
            namespace
        );
    }
}

// @group BusinessLogic > SendNamespaceTeams : POST Microsoft Teams namespace summary card
async fn send_namespace_teams(
    tm: &TeamsTarget,
    namespace: &str,
    processes: &[ProcessInfo],
    event: ProcessEvent,
) {
    let names = namespace_process_text(processes);
    let summary = format!(
        "Namespace {} — {} {}",
        namespace,
        event.label(),
        processes.len()
    );
    let payload = json!({
        "@type":      "MessageCard",
        "@context":   "http://schema.org/extensions",
        "summary":    &summary,
        "themeColor": match event {
            ProcessEvent::Crashed    => "FF0000",
            ProcessEvent::Started    => "36a64f",
            ProcessEvent::Restarted  => "f0ad4e",
            ProcessEvent::Unhealthy  => "ef4444",
            ProcessEvent::HealthRecovered => "22c55e",
            ProcessEvent::Stopped    => "aaaaaa",
            ProcessEvent::CronRun    => "fbbf24",
            ProcessEvent::CronFailed => "ef4444",
        },
        "title": format!("{} {}", event.emoji(), &summary),
        "sections": [{
            "facts": [
                { "name": "Namespace",  "value": namespace },
                { "name": "Event",      "value": event.label() },
                { "name": "Count",      "value": processes.len().to_string() },
                { "name": "Processes",  "value": names },
                { "name": "Timestamp",  "value": Utc::now().to_rfc3339() },
            ]
        }]
    });
    if let Err(e) = post_json_safely(&tm.webhook_url, &payload).await {
        tracing::warn!(
            "Teams namespace notification failed for ns '{}': {e}",
            namespace
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_only_event_config_wins_cascade() {
        let mut process = NotificationConfig::default();
        process.events.on_unhealthy = true;
        process.events.on_health_recovered = true;
        let mut global = NotificationConfig::default();
        global.events.on_start = true;

        let merged = merge_configs(Some(&process), None, Some(&global));

        assert!(merged.events.on_unhealthy);
        assert!(merged.events.on_health_recovered);
        assert!(!merged.events.on_start);
    }

    #[test]
    fn explicit_all_false_event_scope_disables_parent_events() {
        let process = NotificationConfig {
            events_override: true,
            ..NotificationConfig::default()
        };
        let mut global = NotificationConfig::default();
        global.events.on_crash = true;

        let merged = merge_configs(Some(&process), None, Some(&global));

        assert!(!merged.events.on_crash);
        assert!(merged.events_override);
    }
}
