// @group Types : Tunnel configuration and active tunnel state models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_TUNNEL_TOKEN_BYTES: usize = 8 * 1024;
const MAX_CUSTOM_BINARY_BYTES: usize = 4 * 1024;
const MAX_CUSTOM_ARGUMENT_BYTES: usize = 8 * 1024;

// @group Types > TunnelProvider : Which tunneling tool to use
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TunnelProvider {
    #[default]
    Cloudflare,
    Ngrok,
    Custom,
}

// @group Types > TunnelStatus : Lifecycle state of a single tunnel instance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TunnelStatus {
    Starting,
    Active,
    Failed,
    Stopped,
}

// @group Types > TunnelEntry : A single running (or recently stopped) tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelEntry {
    pub id: String,
    pub port: u16,
    pub process_name: Option<String>,
    pub process_id: Option<String>,
    pub provider: TunnelProvider,
    pub public_url: Option<String>,
    pub status: TunnelStatus,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

// @group Types > CloudflareSettings : Cloudflare-specific tunnel config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudflareSettings {
    /// Leave empty for quick tunnels (trycloudflare.com). Fill for named tunnels.
    #[serde(default)]
    pub token: Option<String>,
}

// @group Types > NgrokSettings : ngrok-specific tunnel config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NgrokSettings {
    /// ngrok auth token — required for stable subdomain URLs
    #[serde(default)]
    pub auth_token: Option<String>,
}

// @group Types > CustomTunnelSettings : User-defined tunnel binary config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomTunnelSettings {
    /// Path to the tunnel binary (e.g. "/usr/local/bin/bore" or "C:\\tools\\mytunnel.exe")
    #[serde(default)]
    pub binary_path: String,
    /// Args template — use {port} as placeholder (e.g. "local {port}")
    #[serde(default)]
    pub args_template: String,
}

// @group Types > TunnelSettings : Full tunnel configuration persisted to tunnel.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelSettings {
    #[serde(default)]
    pub provider: TunnelProvider,
    #[serde(default)]
    pub cloudflare: CloudflareSettings,
    #[serde(default)]
    pub ngrok: NgrokSettings,
    #[serde(default)]
    pub custom: CustomTunnelSettings,
}

impl Default for TunnelSettings {
    fn default() -> Self {
        Self {
            provider: TunnelProvider::Cloudflare,
            cloudflare: CloudflareSettings::default(),
            ngrok: NgrokSettings::default(),
            custom: CustomTunnelSettings::default(),
        }
    }
}

impl TunnelSettings {
    pub fn normalize(&mut self) {
        for token in [&mut self.cloudflare.token, &mut self.ngrok.auth_token] {
            if let Some(value) = token.as_mut() {
                *value = value.trim().to_string();
                if value.is_empty() {
                    *token = None;
                }
            }
        }
        self.custom.binary_path = self.custom.binary_path.trim().to_string();
        self.custom.args_template = self.custom.args_template.trim().to_string();
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for token in [
            self.cloudflare.token.as_deref(),
            self.ngrok.auth_token.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            anyhow::ensure!(
                token.len() <= MAX_TUNNEL_TOKEN_BYTES && !token.chars().any(char::is_control),
                "tunnel tokens must be at most 8192 bytes and contain no control characters"
            );
        }
        anyhow::ensure!(
            self.custom.binary_path.len() <= MAX_CUSTOM_BINARY_BYTES
                && !self.custom.binary_path.chars().any(char::is_control),
            "custom tunnel binary path is invalid or too long"
        );
        anyhow::ensure!(
            self.custom.args_template.len() <= MAX_CUSTOM_ARGUMENT_BYTES
                && !self.custom.args_template.contains('\0'),
            "custom tunnel argument template is invalid or too long"
        );
        if self.provider == TunnelProvider::Custom {
            anyhow::ensure!(
                !self.custom.binary_path.is_empty(),
                "custom tunnel binary path is required for the custom provider"
            );
        }
        Ok(())
    }
}

// @group Types > CreateTunnelRequest : Body for POST /tunnels
#[derive(Debug, Deserialize)]
pub struct CreateTunnelRequest {
    pub port: u16,
    pub process_name: Option<String>,
    pub process_id: Option<String>,
    /// Override the globally configured provider for this tunnel only
    pub provider: Option<TunnelProvider>,
}

// @group Types > TestProviderRequest : Body for POST /tunnel-settings/test
#[derive(Debug, Deserialize)]
pub struct TestProviderRequest {
    pub provider: TunnelProvider,
}

// @group Types > InstallProviderRequest : Body for POST /tunnel-settings/install
#[derive(Debug, Deserialize)]
pub struct InstallProviderRequest {
    pub provider: TunnelProvider,
}
