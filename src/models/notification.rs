// @group Types : Notification configuration — webhook, Slack, Teams targets and event flags

use serde::{Deserialize, Serialize};

pub const MASKED_SECRET: &str = "__RUNDOCK_SECRET_SET__";

// @group Types > NotificationEvents : Which process lifecycle events trigger notifications
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationEvents {
    // Process lifecycle events
    #[serde(default)]
    pub on_crash: bool,
    #[serde(default)]
    pub on_restart: bool,
    #[serde(default)]
    pub on_start: bool,
    #[serde(default)]
    pub on_stop: bool,
    #[serde(default)]
    pub on_unhealthy: bool,
    #[serde(default)]
    pub on_health_recovered: bool,
    // Cron job lifecycle events
    #[serde(default)]
    pub on_cron_run: bool,
    #[serde(default)]
    pub on_cron_fail: bool,
}

// @group Types > WebhookTarget : Generic HTTP webhook target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTarget {
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
}

// @group Types > SlackTarget : Slack incoming webhook target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackTarget {
    pub webhook_url: String,
    #[serde(default)]
    pub enabled: bool,
    pub channel: Option<String>,
}

// @group Types > TeamsTarget : Microsoft Teams incoming webhook target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsTarget {
    pub webhook_url: String,
    #[serde(default)]
    pub enabled: bool,
}

// @group Types > DiscordTarget : Discord incoming webhook target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordTarget {
    pub webhook_url: String,
    #[serde(default)]
    pub enabled: bool,
}

// @group Types > NotificationConfig : Full notification configuration for one scope (global / namespace / process)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationConfig {
    pub webhook: Option<WebhookTarget>,
    pub slack: Option<SlackTarget>,
    pub teams: Option<TeamsTarget>,
    pub discord: Option<DiscordTarget>,
    #[serde(default)]
    pub events: NotificationEvents,
    /// Explicitly distinguishes "all events disabled at this scope" from a
    /// legacy config that omitted event override semantics and should inherit.
    #[serde(default)]
    pub events_override: bool,
}

impl NotificationConfig {
    pub fn redacted(&self) -> Self {
        let mut redacted = self.clone();
        if let Some(target) = redacted.webhook.as_mut() {
            target.url = mask_if_set(&target.url);
        }
        if let Some(target) = redacted.slack.as_mut() {
            target.webhook_url = mask_if_set(&target.webhook_url);
        }
        if let Some(target) = redacted.teams.as_mut() {
            target.webhook_url = mask_if_set(&target.webhook_url);
        }
        if let Some(target) = redacted.discord.as_mut() {
            target.webhook_url = mask_if_set(&target.webhook_url);
        }
        redacted
    }

    pub fn preserve_masked_secrets(&mut self, current: &Self) {
        preserve_url(
            self.webhook.as_mut().map(|target| &mut target.url),
            current.webhook.as_ref().map(|target| target.url.as_str()),
        );
        preserve_url(
            self.slack.as_mut().map(|target| &mut target.webhook_url),
            current
                .slack
                .as_ref()
                .map(|target| target.webhook_url.as_str()),
        );
        preserve_url(
            self.teams.as_mut().map(|target| &mut target.webhook_url),
            current
                .teams
                .as_ref()
                .map(|target| target.webhook_url.as_str()),
        );
        preserve_url(
            self.discord.as_mut().map(|target| &mut target.webhook_url),
            current
                .discord
                .as_ref()
                .map(|target| target.webhook_url.as_str()),
        );
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let targets = [
            self.webhook
                .as_ref()
                .map(|target| (target.enabled, target.url.as_str(), "webhook")),
            self.slack
                .as_ref()
                .map(|target| (target.enabled, target.webhook_url.as_str(), "Slack webhook")),
            self.teams
                .as_ref()
                .map(|target| (target.enabled, target.webhook_url.as_str(), "Teams webhook")),
            self.discord.as_ref().map(|target| {
                (
                    target.enabled,
                    target.webhook_url.as_str(),
                    "Discord webhook",
                )
            }),
        ];
        for (enabled, url, label) in targets.into_iter().flatten() {
            if url.len() > 2_048 {
                anyhow::bail!("{label} URL cannot exceed 2048 bytes");
            }
            if enabled && url.is_empty() {
                anyhow::bail!("{label} URL is required when enabled");
            }
            if !url.is_empty() {
                crate::utils::outbound::validate_url(
                    url,
                    crate::utils::outbound::OutboundPolicy::PublicHttps,
                )?;
            }
        }
        if let Some(channel) = self
            .slack
            .as_ref()
            .and_then(|target| target.channel.as_deref())
        {
            anyhow::ensure!(
                channel.len() <= 128 && !channel.chars().any(char::is_control),
                "Slack channel cannot exceed 128 bytes or contain control characters"
            );
        }
        Ok(())
    }
}

fn mask_if_set(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        MASKED_SECRET.to_string()
    }
}

fn preserve_url(candidate: Option<&mut String>, current: Option<&str>) {
    if let Some(candidate) = candidate {
        if candidate == MASKED_SECRET {
            *candidate = current.unwrap_or_default().to_string();
        }
    }
}
