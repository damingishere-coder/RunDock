// @group BusinessLogic : Tunnel manager — spawn and track cloudflared / ngrok / custom tunnel subprocesses

use crate::models::tunnel::{
    CreateTunnelRequest, TunnelEntry, TunnelProvider, TunnelSettings, TunnelStatus,
};
use crate::process::instance::ProcessIdentity;
use crate::process::tree::ProcessTreeGuard;
use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use uuid::Uuid;

// @group BusinessLogic > TunnelManager : Shared handle — cheap to clone, backed by Arc
#[derive(Clone)]
pub struct TunnelManager {
    pub entries: Arc<DashMap<String, TunnelEntry>>,
    pids: Arc<DashMap<String, TunnelProcess>>,
    create_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone)]
struct TunnelProcess {
    pid: u32,
    identity: ProcessIdentity,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            pids: Arc::new(DashMap::new()),
            create_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    // @group BusinessLogic > TunnelManager : Spawn a new tunnel subprocess and track it
    pub async fn create(
        &self,
        req: CreateTunnelRequest,
        settings: &TunnelSettings,
    ) -> Result<TunnelEntry, String> {
        const MAX_TRACKED_TUNNELS: usize = 256;
        const MAX_ACTIVE_TUNNELS: usize = 32;
        const MAX_TUNNEL_LABEL_BYTES: usize = 256;

        if req.port == 0 {
            return Err("Tunnel port must be between 1 and 65535".to_string());
        }
        for (label, value) in [
            ("process_name", req.process_name.as_deref()),
            ("process_id", req.process_id.as_deref()),
        ] {
            if value.is_some_and(|value| {
                value.len() > MAX_TUNNEL_LABEL_BYTES || value.chars().any(char::is_control)
            }) {
                return Err(format!(
                    "Tunnel {label} must be at most {MAX_TUNNEL_LABEL_BYTES} bytes and contain no control characters"
                ));
            }
        }

        let _create_guard = self.create_lock.lock().await;
        if self.entries.len() >= MAX_TRACKED_TUNNELS {
            return Err(format!(
                "At most {MAX_TRACKED_TUNNELS} tunnel records may be retained; remove an old stopped or failed tunnel first"
            ));
        }
        if self.pids.len() >= MAX_ACTIVE_TUNNELS {
            return Err(format!(
                "At most {MAX_ACTIVE_TUNNELS} tunnels may run at the same time"
            ));
        }
        let id = Uuid::new_v4().to_string();
        let provider = req
            .provider
            .clone()
            .unwrap_or_else(|| settings.provider.clone());

        let entry = TunnelEntry {
            id: id.clone(),
            port: req.port,
            process_name: req.process_name.clone(),
            process_id: req.process_id.clone(),
            provider: provider.clone(),
            public_url: None,
            status: TunnelStatus::Starting,
            error: None,
            created_at: Utc::now(),
        };

        self.entries.insert(id.clone(), entry.clone());

        // Build the tokio::process::Command for the selected provider
        let mut cmd = match build_command(&provider, req.port, settings) {
            Ok(c) => c,
            Err(e) => {
                self.entries.remove(&id);
                return Err(e);
            }
        };

        cmd.kill_on_drop(true);

        // @group BusinessLogic > TunnelManager : Own the complete tunnel process tree
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB lets this manager own
            // a dedicated kill-on-close Job Object instead of inheriting an
            // unrelated parent job.
            cmd.creation_flags(0x0900_0000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        let spawn_result = match cmd.spawn() {
            Ok(child) => Ok(child),
            Err(initial_error) if initial_error.kind() == std::io::ErrorKind::PermissionDenied => {
                tracing::warn!(
                    "tunnel process breakaway was denied; retrying without CREATE_BREAKAWAY_FROM_JOB"
                );
                cmd.creation_flags(0x0800_0000);
                cmd.spawn()
            }
            Err(error) => Err(error),
        };
        #[cfg(not(windows))]
        let spawn_result = cmd.spawn();

        let mut child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("Failed to spawn tunnel process: {e}");
                self.entries.remove(&id);
                return Err(msg);
            }
        };

        let pid = child
            .id()
            .ok_or_else(|| "Tunnel process did not expose a PID".to_string())?;
        let process_tree = match ProcessTreeGuard::new(pid, &format!("tunnel-{id}")) {
            Ok(guard) => guard,
            Err(error) => {
                let cleanup = crate::process::identity::kill_spawned_process(&mut child, pid).await;
                let message = format!(
                    "Could not establish tunnel process-tree ownership: {error}; cleanup result: {cleanup:?}"
                );
                self.entries.remove(&id);
                return Err(message);
            }
        };
        let Some(identity) =
            crate::process::identity::capture_process_identity_with_retry(pid).await
        else {
            let cleanup = crate::process::identity::kill_spawned_process(&mut child, pid).await;
            let message = format!(
                "Could not capture a verifiable tunnel process identity; cleanup result: {cleanup:?}"
            );
            self.entries.remove(&id);
            return Err(message);
        };
        self.pids
            .insert(id.clone(), TunnelProcess { pid, identity });

        // Spawn background task: scan output for URL, then monitor process exit
        let entries = Arc::clone(&self.entries);
        let pids = Arc::clone(&self.pids);
        let tunnel_id = id.clone();
        let provider_for_task = provider.clone();

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        tokio::spawn(async move {
            let _process_tree = process_tree;
            let url = watch_output(stdout, stderr, &provider_for_task).await;

            match url {
                Ok(Some(found_url)) => {
                    if pids.contains_key(&tunnel_id) {
                        if let Some(mut entry) = entries.get_mut(&tunnel_id) {
                            if entry.status == TunnelStatus::Starting {
                                entry.public_url = Some(found_url);
                                entry.status = TunnelStatus::Active;
                            }
                        }
                    }
                }
                outcome @ (Ok(None) | Err(_)) => {
                    // Timed out or process exited before URL was found
                    let cleanup = crate::process::identity::kill_spawned_process(&mut child, pid)
                        .await
                        .err()
                        .map(|error| format!("; cleanup failed: {error}"))
                        .unwrap_or_default();
                    if let Some(mut e) = entries.get_mut(&tunnel_id) {
                        if e.status == TunnelStatus::Starting {
                            e.status = TunnelStatus::Failed;
                            let reason = match &outcome {
                                Err(error) => format!("Tunnel output could not be read: {error}"),
                                Ok(_) => "Process exited or timed out before a public URL was found. Check that the binary is installed and in PATH.".to_string(),
                            };
                            e.error = Some(format!("{reason}{cleanup}"));
                        }
                    }
                    pids.remove(&tunnel_id);
                    return;
                }
            }

            // After URL found, wait for the process to exit and mark it failed
            let _ = child.wait().await;
            pids.remove(&tunnel_id);
            if let Some(mut e) = entries.get_mut(&tunnel_id) {
                if e.status == TunnelStatus::Active {
                    e.status = TunnelStatus::Failed;
                    e.error = Some("Tunnel process exited unexpectedly".into());
                }
            }
        });

        Ok(entry)
    }

    // @group BusinessLogic > TunnelManager : Return all tunnel entries (any status)
    pub fn list(&self) -> Vec<TunnelEntry> {
        let mut list: Vec<TunnelEntry> = self.entries.iter().map(|e| e.value().clone()).collect();
        list.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
        list
    }

    // @group BusinessLogic > TunnelManager : Kill a tunnel by ID and mark it stopped
    pub async fn stop(&self, id: &str) -> Result<bool, String> {
        // Serialize against create so a stop cannot mark a Starting entry as
        // stopped before its owned PID has been registered.
        let _create_guard = self.create_lock.lock().await;
        if let Some(process) = self.pids.get(id).map(|entry| entry.value().clone()) {
            crate::process::identity::kill_orphan_pid(process.pid, &process.identity)
                .await
                .map_err(|error| format!("Refusing to stop unverified tunnel PID: {error}"))?;
            self.pids.remove(id);
        }
        Ok(match self.entries.get_mut(id) {
            Some(mut e) => {
                e.status = TunnelStatus::Stopped;
                e.error = None;
                true
            }
            None => false,
        })
    }

    // @group BusinessLogic > TunnelManager : Remove a stopped/failed tunnel from the list
    pub fn remove(&self, id: &str) -> bool {
        self.pids.remove(id);
        self.entries.remove(id).is_some()
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

// @group Utilities > TunnelManager : Build the tokio Command for each provider
fn build_command(
    provider: &TunnelProvider,
    port: u16,
    settings: &TunnelSettings,
) -> Result<tokio::process::Command, String> {
    match provider {
        TunnelProvider::Cloudflare => {
            let mut cmd = tokio::process::Command::new("cloudflared");
            // Named tunnel when a token is configured; quick tunnel otherwise
            if let Some(token) = &settings.cloudflare.token {
                if !token.is_empty() {
                    cmd.env("TUNNEL_TOKEN", token);
                    cmd.args(["tunnel", "run"]);
                    return Ok(cmd);
                }
            }
            cmd.args([
                "tunnel",
                "--url",
                &format!("http://localhost:{port}"),
                "--no-autoupdate",
            ]);
            Ok(cmd)
        }
        TunnelProvider::Ngrok => {
            let mut cmd = tokio::process::Command::new("ngrok");
            cmd.args([
                "http",
                &port.to_string(),
                "--log=stdout",
                "--log-format=json",
            ]);
            if let Some(token) = &settings.ngrok.auth_token {
                if !token.is_empty() {
                    cmd.env("NGROK_AUTHTOKEN", token);
                }
            }
            Ok(cmd)
        }
        TunnelProvider::Custom => {
            let binary = &settings.custom.binary_path;
            if binary.is_empty() {
                return Err(
                    "Custom tunnel binary path is not configured. Go to Settings → Tunnels.".into(),
                );
            }
            let mut cmd = tokio::process::Command::new(binary);
            let args_raw = settings
                .custom
                .args_template
                .replace("{port}", &port.to_string());
            if !args_raw.is_empty() {
                let args = parse_argument_template(&args_raw)?;
                cmd.args(&args);
            }
            Ok(cmd)
        }
    }
}

fn parse_argument_template(input: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(expected), value) if value == expected => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            (_, '\\')
                if chars
                    .peek()
                    .is_some_and(|next| matches!(*next, '\\' | '\'' | '"')) =>
            {
                current.push(chars.next().expect("peeked argument character"));
            }
            (_, value) => current.push(value),
        }
    }
    if let Some(unclosed) = quote {
        return Err(format!(
            "Custom tunnel arguments contain an unclosed {unclosed} quote"
        ));
    }
    if !current.is_empty() {
        args.push(current);
    }
    if args.len() > 128 || args.iter().any(|arg| arg.len() > 4_096) {
        return Err("Custom tunnel arguments exceed the configured safety limit".into());
    }
    Ok(args)
}

const MAX_TUNNEL_OUTPUT_LINE_BYTES: usize = 64 * 1024;

enum OutputEvent {
    Line(String),
    ReadError(String),
}

enum BoundedOutputLine {
    Line(String),
    Oversized,
    Eof,
}

async fn read_bounded_output_line<R>(reader: &mut R) -> Result<BoundedOutputLine, String>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf().await.map_err(|error| error.to_string())?;
        if available.is_empty() {
            if line.is_empty() && !oversized {
                return Ok(BoundedOutputLine::Eof);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |position| position + 1);
        let content_len = newline.unwrap_or(available.len());
        if !oversized {
            if line.len().saturating_add(content_len) > MAX_TUNNEL_OUTPUT_LINE_BYTES {
                oversized = true;
            } else {
                line.extend_from_slice(&available[..content_len]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    if oversized {
        Ok(BoundedOutputLine::Oversized)
    } else {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Ok(BoundedOutputLine::Line(
            String::from_utf8_lossy(&line).into_owned(),
        ))
    }
}

async fn forward_tunnel_output<R>(reader: R, tx: tokio::sync::mpsc::Sender<OutputEvent>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    loop {
        match read_bounded_output_line(&mut reader).await {
            Ok(BoundedOutputLine::Line(line)) => {
                // Once the URL watcher returns, keep draining the pipe so the
                // long-running tunnel cannot deadlock on a full stdout/stderr buffer.
                let _ = tx.send(OutputEvent::Line(line)).await;
            }
            Ok(BoundedOutputLine::Oversized) => {
                if tx
                    .send(OutputEvent::ReadError(
                        "tunnel output line exceeded the 64 KiB safety limit".into(),
                    ))
                    .await
                    .is_ok()
                {
                    break;
                }
            }
            Ok(BoundedOutputLine::Eof) => break,
            Err(error) => {
                let _ = tx.send(OutputEvent::ReadError(error)).await;
                break;
            }
        }
    }
}

// @group Utilities > TunnelManager : Scan subprocess stdout+stderr for a public URL
async fn watch_output(
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    provider: &TunnelProvider,
) -> Result<Option<String>, String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<OutputEvent>(128);

    if let Some(out) = stdout {
        let tx2 = tx.clone();
        tokio::spawn(async move {
            forward_tunnel_output(out, tx2).await;
        });
    }

    if let Some(err) = stderr {
        let tx2 = tx.clone();
        tokio::spawn(async move {
            forward_tunnel_output(err, tx2).await;
        });
    }
    drop(tx); // close sender so channel closes when both readers finish

    let timeout = tokio::time::Duration::from_secs(45);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(OutputEvent::Line(line))) => {
                if let Some(url) = extract_url(&line, provider) {
                    return Ok(Some(url));
                }
            }
            Ok(Some(OutputEvent::ReadError(error))) => return Err(error),
            Ok(None) => return Ok(None), // channel closed — process exited
            Err(_) => return Ok(None),   // 45-second timeout
        }
    }
}

// @group Utilities > TunnelManager : Provider-specific URL extraction from one output line
fn validated_tunnel_url(candidate: &str) -> Option<String> {
    let candidate = candidate.trim_end_matches(['.', ',', ';', ')', ']', '}']);
    let parsed = reqwest::Url::parse(candidate).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none_or(str::is_empty)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let mut normalized = parsed.to_string();
    if parsed.path() == "/" && parsed.query().is_none() && parsed.fragment().is_none() {
        normalized.pop();
    }
    Some(normalized)
}

fn extract_url(line: &str, provider: &TunnelProvider) -> Option<String> {
    match provider {
        TunnelProvider::Cloudflare => {
            // Quick tunnel: "https://abc-def-123.trycloudflare.com"
            // Named tunnel: "https://your-domain.example.com"
            // The URL appears on a line containing the domain
            if let Some(start) = line.find("https://") {
                let rest = &line[start..];
                let end = rest
                    .find(|c: char| {
                        c.is_whitespace() || c == '"' || c == '|' || c == '\'' || c == '>'
                    })
                    .unwrap_or(rest.len());
                return validated_tunnel_url(&rest[..end]);
            }
            None
        }
        TunnelProvider::Ngrok => {
            // JSON log line: {...,"msg":"started tunnel",...,"url":"https://abc.ngrok-free.app"}
            if let Some(idx) = line.find("\"url\":\"") {
                let rest = &line[idx + 7..];
                if rest.starts_with("https://") {
                    let end = rest.find('"').unwrap_or(rest.len());
                    return validated_tunnel_url(&rest[..end]);
                }
            }
            // Fallback: any https:// URL on a line mentioning ngrok or tunnel
            if line.contains("https://") && (line.contains("ngrok") || line.contains("tunnel")) {
                if let Some(start) = line.find("https://") {
                    let rest = &line[start..];
                    let end = rest
                        .find(|c: char| c.is_whitespace() || c == '"')
                        .unwrap_or(rest.len());
                    return validated_tunnel_url(&rest[..end]);
                }
            }
            None
        }
        TunnelProvider::Custom => {
            // Generic: grab the first https:// URL found on the line
            if let Some(start) = line.find("https://") {
                let rest = &line[start..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(rest.len());
                return validated_tunnel_url(&rest[..end]);
            }
            None
        }
    }
}

// @group Utilities > TunnelManager : Check whether a provider binary is reachable
pub async fn check_provider(
    provider: &TunnelProvider,
    settings: &TunnelSettings,
) -> (bool, String) {
    let binary = match provider {
        TunnelProvider::Cloudflare => "cloudflared".to_string(),
        TunnelProvider::Ngrok => "ngrok".to_string(),
        TunnelProvider::Custom => settings.custom.binary_path.clone(),
    };

    if binary.is_empty() {
        return (false, "Binary path is not configured".into());
    }

    let mut cmd = tokio::process::Command::new(&binary);
    cmd.arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.kill_on_drop(true);

    #[cfg(windows)]
    {
        cmd.creation_flags(0x0900_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => {
            return (
                false,
                format!("`{binary}` not found — install it and make sure it is in your PATH"),
            )
        }
    };
    let Some(pid) = child.id() else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return (false, format!("`{binary}` did not expose a process ID"));
    };
    let _tree = match ProcessTreeGuard::new(pid, &format!("tunnel-probe-{pid}")) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = crate::process::identity::kill_spawned_process(&mut child, pid).await;
            return (
                false,
                format!("`{binary}` probe could not be contained: {error}"),
            );
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) if status.success() => {
            (true, format!("`{binary}` is installed and reachable"))
        }
        Ok(Ok(status)) => (
            false,
            format!("`{binary}` returned a non-success status: {status}"),
        ),
        Ok(Err(_)) => (false, format!("`{binary}` version check failed")),
        Err(_) => {
            let _ = crate::process::identity::kill_spawned_process(&mut child, pid).await;
            (false, format!("`{binary}` version check timed out"))
        }
    }
}

// @group UnitTests : extract_url — Cloudflare / ngrok / custom provider URL parsing
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_argument_parser_preserves_quoted_values_and_windows_paths() {
        assert_eq!(
            parse_argument_template(r#"--header "foo bar" --bin C:\tools\tunnel.exe"#).unwrap(),
            vec!["--header", "foo bar", "--bin", r"C:\tools\tunnel.exe"]
        );
    }

    #[test]
    fn custom_argument_parser_rejects_unclosed_quotes_and_oversized_sets() {
        assert!(parse_argument_template("--header 'unfinished").is_err());
        assert!(parse_argument_template(&vec!["x"; 129].join(" ")).is_err());
    }

    // @group UnitTests > Cloudflare : Quick-tunnel line yields the trycloudflare URL
    #[test]
    fn test_cloudflare_quick_tunnel_url() {
        let line = "2026-03-30T00:00:00Z INF | https://abc-def-123.trycloudflare.com |";
        let url = extract_url(line, &TunnelProvider::Cloudflare).unwrap();
        assert_eq!(url, "https://abc-def-123.trycloudflare.com");
    }

    // @group UnitTests > Cloudflare : Named tunnel line yields the custom domain URL
    #[test]
    fn test_cloudflare_named_tunnel_url() {
        let line = "Registered tunnel connection tunnelID=xxx url=https://my.example.com";
        let url = extract_url(line, &TunnelProvider::Cloudflare).unwrap();
        assert_eq!(url, "https://my.example.com");
    }

    // @group UnitTests > Cloudflare : Trailing slash is stripped from the URL
    #[test]
    fn test_cloudflare_strips_trailing_slash() {
        let line = "https://trailing.trycloudflare.com/";
        let url = extract_url(line, &TunnelProvider::Cloudflare).unwrap();
        assert_eq!(url, "https://trailing.trycloudflare.com");
    }

    // @group UnitTests > Cloudflare : Line with no URL returns None
    #[test]
    fn test_cloudflare_no_url_returns_none() {
        let url = extract_url("starting cloudflared process", &TunnelProvider::Cloudflare);
        assert!(url.is_none());
    }

    // @group UnitTests > Ngrok : JSON log line with "url" key yields the tunnel URL
    #[test]
    fn test_ngrok_json_url() {
        let line =
            r#"{"level":"info","msg":"started tunnel","url":"https://abc123.ngrok-free.app"}"#;
        let url = extract_url(line, &TunnelProvider::Ngrok).unwrap();
        assert_eq!(url, "https://abc123.ngrok-free.app");
    }

    // @group UnitTests > Ngrok : Fallback path — plain line mentioning ngrok + https URL
    #[test]
    fn test_ngrok_fallback_url() {
        let line = "started ngrok tunnel at https://abc.ngrok.io";
        let url = extract_url(line, &TunnelProvider::Ngrok).unwrap();
        assert_eq!(url, "https://abc.ngrok.io");
    }

    // @group UnitTests > Ngrok : Line with no URL returns None
    #[test]
    fn test_ngrok_no_url_returns_none() {
        let url = extract_url("ngrok connecting...", &TunnelProvider::Ngrok);
        assert!(url.is_none());
    }

    // @group UnitTests > Custom : Any line with an https:// URL returns it
    #[test]
    fn test_custom_picks_first_https_url() {
        let line = "tunnel ready at https://custom-tool.example.io/path";
        let url = extract_url(line, &TunnelProvider::Custom).unwrap();
        assert_eq!(url, "https://custom-tool.example.io/path");
    }

    // @group UnitTests > Custom : URL surrounded by quotes is extracted without them
    #[test]
    fn test_custom_quoted_url() {
        let line = r#"url="https://quoted.example.com" status=ok"#;
        let url = extract_url(line, &TunnelProvider::Custom).unwrap();
        assert_eq!(url, "https://quoted.example.com");
    }

    // @group UnitTests > Custom : Line without https returns None
    #[test]
    fn test_custom_no_url_returns_none() {
        let url = extract_url("starting custom tool...", &TunnelProvider::Custom);
        assert!(url.is_none());
    }

    #[test]
    fn test_custom_rejects_malformed_or_credentialed_urls() {
        assert!(extract_url("ready https://", &TunnelProvider::Custom).is_none());
        assert!(extract_url(
            "ready https://user:pass@example.com",
            &TunnelProvider::Custom
        )
        .is_none());
    }

    #[test]
    fn test_custom_trims_log_punctuation() {
        assert_eq!(
            extract_url(
                "ready (https://custom.example.com/path).",
                &TunnelProvider::Custom
            )
            .as_deref(),
            Some("https://custom.example.com/path")
        );
    }

    // @group UnitTests > EdgeCases : Empty line returns None for all providers
    #[test]
    fn test_empty_line_all_providers() {
        for provider in [
            TunnelProvider::Cloudflare,
            TunnelProvider::Ngrok,
            TunnelProvider::Custom,
        ] {
            assert!(extract_url("", &provider).is_none());
        }
    }
}
