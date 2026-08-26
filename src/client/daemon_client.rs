// @group APIEndpoints : HTTP client wrapper — CLI to daemon communication

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::Value;

const MAX_DAEMON_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_HEALTH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_SSE_BUFFER_BYTES: usize = 1024 * 1024;

fn drain_sse_lines(buf: &mut String, on_line: &mut impl FnMut(String)) -> Result<()> {
    while let Some(pos) = buf.find('\n') {
        if pos > MAX_SSE_BUFFER_BYTES {
            anyhow::bail!("log stream exceeded the 1 MiB line buffer limit");
        }
        let remainder = buf.split_off(pos + 1);
        let line = buf[..pos].trim().to_string();
        *buf = remainder;
        if let Some(data) = line.strip_prefix("data: ") {
            on_line(data.to_string());
        }
    }
    if buf.len() > MAX_SSE_BUFFER_BYTES {
        anyhow::bail!("log stream exceeded the 1 MiB line buffer limit");
    }
    Ok(())
}

async fn read_bounded_response(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        anyhow::bail!("daemon response exceeded the {limit}-byte limit");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read daemon response")?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("daemon response exceeded the {limit}-byte limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn plaintext_loopback_authority(host: &str) -> Result<String> {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return Ok("127.0.0.1".to_string());
    }
    let literal = host.trim_matches(['[', ']']);
    let address = literal.parse::<std::net::IpAddr>().map_err(|_| {
        anyhow!(
            "refusing to send daemon credentials to hostname '{host}'; use a literal loopback IP"
        )
    })?;
    if !address.is_loopback() {
        anyhow::bail!(
            "refusing to send daemon credentials over plaintext to non-loopback host '{host}'; use a local SSH tunnel or HTTPS client"
        );
    }
    Ok(if address.is_ipv6() {
        format!("[{address}]")
    } else {
        address.to_string()
    })
}

fn health_payload_is_alive(health: &Value) -> bool {
    matches!(
        health.get("status").and_then(Value::as_str),
        Some("ok" | "degraded")
    ) && health.get("version").and_then(Value::as_str).is_some()
}

pub struct DaemonClient {
    base_url: String,
    client: Client,
    probe_client: Client,
    stream_client: Client,
}

impl DaemonClient {
    pub fn new(host: &str, port: u16) -> Result<Self> {
        if port == 0 {
            anyhow::bail!("daemon port 0 is not supported");
        }
        let authority = plaintext_loopback_authority(host)?;
        let base_url = format!("http://{authority}:{port}");
        reqwest::Url::parse(&base_url).context("invalid daemon URL")?;

        // @group Authentication : Inject master token so the CLI authenticates with the daemon
        let token = crate::config::auth_config::load().master_token;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }

        Ok(Self {
            base_url,
            client: Client::builder()
                .default_headers(headers.clone())
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .context("failed to build daemon HTTP client")?,
            probe_client: Client::builder()
                .default_headers(headers.clone())
                .connect_timeout(std::time::Duration::from_millis(300))
                .timeout(std::time::Duration::from_millis(500))
                .build()
                .context("failed to build daemon probe client")?,
            stream_client: Client::builder()
                .default_headers(headers)
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .context("failed to build daemon stream client")?,
        })
    }

    // @group APIEndpoints > Client : Check if daemon is reachable
    pub async fn is_alive(&self) -> bool {
        self.is_alive_with_timeout(std::time::Duration::from_millis(500))
            .await
    }

    pub async fn is_alive_with_timeout(&self, timeout: std::time::Duration) -> bool {
        let probe = async {
            let response = self
                .probe_client
                .get(format!("{}/api/v1/system/health", self.base_url))
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                return None;
            }
            let body = read_bounded_response(response, MAX_HEALTH_RESPONSE_BYTES)
                .await
                .ok()?;
            serde_json::from_slice::<Value>(&body).ok()
        };
        tokio::time::timeout(timeout, probe)
            .await
            .ok()
            .flatten()
            .is_some_and(|health| {
                let health_pid = health
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|pid| u32::try_from(pid).ok());
                health_payload_is_alive(&health)
                    && health_pid.is_some()
                    && health_pid == crate::utils::pid::read_pid()
                    && crate::utils::pid::is_daemon_running()
            })
    }

    // @group APIEndpoints > Client : GET request helper
    pub async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;
        self.handle_response(resp).await
    }

    // @group APIEndpoints > Client : POST request helper
    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;
        self.handle_response(resp).await
    }

    // @group APIEndpoints > Client : DELETE request helper
    pub async fn delete(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .send()
            .await
            .with_context(|| format!("DELETE {url} failed"))?;
        self.handle_response(resp).await
    }

    // @group APIEndpoints > Client : Stream SSE logs — calls callback for each line
    pub async fn stream_logs(
        &self,
        process_id: &str,
        mut on_line: impl FnMut(String),
    ) -> Result<()> {
        use futures::StreamExt;

        let url = format!(
            "{}/api/v1/processes/{process_id}/logs/stream",
            self.base_url
        );
        let resp = self
            .stream_client
            .get(&url)
            .send()
            .await
            .with_context(|| "failed to connect to log stream")?;
        let resp = resp
            .error_for_status()
            .with_context(|| format!("log stream request for process {process_id} was rejected"))?;

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        loop {
            let next = tokio::time::timeout(std::time::Duration::from_secs(5 * 60), stream.next())
                .await
                .map_err(|_| anyhow::anyhow!("log stream was idle for 5 minutes"))?;
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            drain_sse_lines(&mut buf, &mut on_line)?;
        }
        Ok(())
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let body = read_bounded_response(resp, MAX_DAEMON_RESPONSE_BYTES).await?;
        let text = String::from_utf8_lossy(&body);

        if status.is_success() {
            serde_json::from_slice(&body)
                .with_context(|| format!("daemon returned non-JSON success response ({status})"))
        } else {
            let parsed = serde_json::from_slice::<Value>(&body).ok();
            let msg = parsed
                .as_ref()
                .and_then(|body| body.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let mut detail: String = text.chars().take(512).collect();
                    if detail.trim().is_empty() {
                        detail = "empty response body".to_string();
                    }
                    detail
                });
            Err(anyhow!("daemon request failed ({status}): {msg}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{drain_sse_lines, health_payload_is_alive, plaintext_loopback_authority};

    #[test]
    fn plaintext_daemon_credentials_are_loopback_only() {
        assert_eq!(
            plaintext_loopback_authority("127.0.0.1").unwrap(),
            "127.0.0.1"
        );
        assert_eq!(
            plaintext_loopback_authority("localhost").unwrap(),
            "127.0.0.1"
        );
        assert_eq!(plaintext_loopback_authority("::1").unwrap(), "[::1]");
        assert!(plaintext_loopback_authority("192.168.1.4").is_err());
        assert!(plaintext_loopback_authority("example.com").is_err());
    }

    #[test]
    fn degraded_daemon_health_still_proves_liveness() {
        assert!(health_payload_is_alive(
            &serde_json::json!({ "status": "ok", "version": "1.1.0" })
        ));
        assert!(health_payload_is_alive(
            &serde_json::json!({ "status": "degraded", "version": "1.1.0" })
        ));
        assert!(!health_payload_is_alive(
            &serde_json::json!({ "status": "failed", "version": "1.1.0" })
        ));
        assert!(!health_payload_is_alive(
            &serde_json::json!({ "status": "ok" })
        ));
    }

    #[test]
    fn oversized_complete_sse_line_is_rejected_before_callback() {
        let mut buffer = format!("data: {}\n", "x".repeat(1024 * 1024));
        let mut delivered = Vec::new();

        let result = drain_sse_lines(&mut buffer, &mut |line| delivered.push(line));

        assert!(result.is_err());
        assert!(delivered.is_empty());
    }
}
