// @group APIEndpoints : Tunnel routes — create, list, stop tunnels and manage tunnel settings

// @group Utilities > InstallStream : Strip CR-based spinner frames from winget/brew output lines.
// Winget uses \r to overwrite the current line (spinner animation); split on \r and take the last
// non-empty segment so the UI sees only the final visible text for each line.
pub(crate) fn clean_install_line(raw: &str) -> Option<String> {
    const MAX_INSTALL_LINE_CHARS: usize = 4 * 1024;
    let s = raw.trim_end_matches(['\n', '\r']);
    let last = s.split('\r').rfind(|p| !p.trim().is_empty())?;
    let mut characters = last.trim().chars();
    let mut clean: String = characters.by_ref().take(MAX_INSTALL_LINE_CHARS).collect();
    if characters.next().is_some() {
        clean.push_str("…[truncated]");
    }
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use crate::models::tunnel::{CreateTunnelRequest, InstallProviderRequest, TestProviderRequest};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        // @group APIEndpoints > Tunnels : Active tunnel management
        .route("/", get(list_tunnels))
        .route("/", post(create_tunnel))
        .route("/{id}/stop", post(stop_tunnel))
        .route("/{id}", delete(remove_tunnel))
        // @group APIEndpoints > TunnelSettings : Provider configuration
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
        .route("/settings/test", post(test_provider))
        .route("/settings/install", post(install_provider))
        .route("/settings/install/stream", get(install_provider_stream))
        .with_state(state)
}

// @group APIEndpoints > Tunnels : GET /tunnels — list all tracked tunnels
async fn list_tunnels(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let tunnels = state.tunnel_manager.list();
    Json(json!({ "tunnels": tunnels }))
}

// @group APIEndpoints > Tunnels : POST /tunnels — create a new tunnel for a port
async fn create_tunnel(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<CreateTunnelRequest>,
) -> Result<Json<Value>, ApiError> {
    let settings = state.tunnel_settings.read().await.clone();
    match state.tunnel_manager.create(req, &settings).await {
        Ok(entry) => Ok(Json(json!({ "tunnel": entry }))),
        Err(error) => Err(ApiError::internal(error)),
    }
}

// @group APIEndpoints > Tunnels : DELETE /tunnels/:id — stop a running tunnel
async fn stop_tunnel(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match state.tunnel_manager.stop(&id).await {
        Ok(true) => Ok(Json(json!({ "success": true }))),
        Ok(false) => Err(ApiError::not_found("Tunnel not found")),
        Err(error) => Err(ApiError::internal(error)),
    }
}

// @group APIEndpoints > Tunnels : DELETE /tunnels/:id/remove — remove a stopped/failed tunnel from the list
async fn remove_tunnel(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Stop first (no-op if already stopped), then remove from list
    if let Err(error) = state.tunnel_manager.stop(&id).await {
        return Err(ApiError::internal(error));
    }
    if state.tunnel_manager.remove(&id) {
        Ok(Json(json!({ "success": true })))
    } else {
        Err(ApiError::not_found("Tunnel not found"))
    }
}

// @group APIEndpoints > TunnelSettings : GET /tunnels/settings
async fn get_settings(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let mut settings = state.tunnel_settings.read().await.clone();
    if settings
        .cloudflare
        .token
        .as_deref()
        .is_some_and(|token| !token.is_empty())
    {
        settings.cloudflare.token = Some(crate::models::notification::MASKED_SECRET.to_string());
    }
    if settings
        .ngrok
        .auth_token
        .as_deref()
        .is_some_and(|token| !token.is_empty())
    {
        settings.ngrok.auth_token = Some(crate::models::notification::MASKED_SECRET.to_string());
    }
    Json(json!(settings))
}

// @group APIEndpoints > TunnelSettings : PUT /tunnels/settings — persist provider config
async fn update_settings(
    State(state): State<Arc<DaemonState>>,
    Json(mut new_settings): Json<crate::models::tunnel::TunnelSettings>,
) -> Result<Json<Value>, ApiError> {
    let _config_guard = state.config_mutation_lock.lock().await;
    let current = state.tunnel_settings.read().await.clone();
    if new_settings.cloudflare.token.as_deref() == Some(crate::models::notification::MASKED_SECRET)
    {
        new_settings.cloudflare.token = current.cloudflare.token;
    }
    if new_settings.ngrok.auth_token.as_deref() == Some(crate::models::notification::MASKED_SECRET)
    {
        new_settings.ngrok.auth_token = current.ngrok.auth_token;
    }
    new_settings.normalize();
    new_settings
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let settings_for_save = new_settings.clone();
    let save_result =
        tokio::task::spawn_blocking(move || crate::config::tunnel_config::save(&settings_for_save))
            .await
            .map_err(|error| ApiError::internal(format!("tunnel save task failed: {error}")))?;
    match save_result {
        Ok(()) => {
            *state.tunnel_settings.write().await = new_settings;
            Ok(Json(json!({ "success": true })))
        }
        Err(error) => Err(ApiError::internal(error)),
    }
}

// @group APIEndpoints > TunnelSettings : POST /tunnels/settings/test — check if provider binary is installed
async fn test_provider(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<TestProviderRequest>,
) -> Json<Value> {
    let _permit = match state.tunnel_probe_limit.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Json(json!({
                "ok": false,
                "message": "Provider check capacity is busy; retry shortly"
            }));
        }
    };
    let settings = state.tunnel_settings.read().await.clone();
    let (ok, message) = crate::tunnel::check_provider(&req.provider, &settings).await;
    Json(json!({ "ok": ok, "message": message }))
}

// @group APIEndpoints > TunnelSettings : POST /tunnels/settings/install — install a provider binary via package manager
async fn install_provider(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<InstallProviderRequest>,
) -> Json<Value> {
    use std::process::Stdio;

    let (program, args) = match tunnel_install_command(req.provider) {
        Ok(command) => command,
        Err(message) => {
            return Json(json!({ "ok": false, "output": message }));
        }
    };
    let _install_guard = state.tunnel_install_lock.lock().await;

    let mut command = tokio::process::Command::new(&program);
    command
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000 | 0x0100_0000);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Json(json!({
                "ok": false,
                "output": format!("Failed to run installer: {error}")
            }));
        }
    };
    let Some(pid) = child.id() else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Json(json!({ "ok": false, "output": "Installer did not expose a process id." }));
    };
    let process_tree = match crate::process::tree::ProcessTreeGuard::attach_or_terminate(
        &mut child,
        pid,
        &format!("tunnel-installer-{pid}"),
    )
    .await
    {
        Ok(tree) => tree,
        Err(error) => {
            return Json(json!({
                "ok": false,
                "output": format!("Failed to contain installer process tree: {error}")
            }));
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(15 * 60), child.wait()).await {
        Ok(Ok(status)) => {
            drop(process_tree);
            Json(json!({
                "ok": status.success(),
                "output": if status.success() {
                    "Provider installation completed."
                } else {
                    "Provider installer exited unsuccessfully."
                }
            }))
        }
        Ok(Err(error)) => {
            let terminate_error = crate::process::identity::kill_spawned_process(&mut child, pid)
                .await
                .err()
                .map(|cleanup| cleanup.to_string());
            let tree_error = process_tree
                .terminate_and_wait()
                .await
                .err()
                .map(|cleanup| cleanup.to_string());
            drop(process_tree);
            let final_wait =
                tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            Json(json!({
                "ok": false,
                "output": format!(
                    "Installer wait failed: {error}; terminate={terminate_error:?}; tree={tree_error:?}; final_wait={final_wait:?}"
                )
            }))
        }
        Err(_) => {
            let terminate_error = crate::process::identity::kill_spawned_process(&mut child, pid)
                .await
                .err()
                .map(|cleanup| cleanup.to_string());
            let tree_error = process_tree
                .terminate_and_wait()
                .await
                .err()
                .map(|cleanup| cleanup.to_string());
            drop(process_tree);
            let final_wait =
                tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            Json(json!({
                "ok": false,
                "output": format!(
                    "Provider installation timed out after 15 minutes; terminate={terminate_error:?}; tree={tree_error:?}; final_wait={final_wait:?}"
                )
            }))
        }
    }
}

fn tunnel_install_command(
    provider: crate::models::tunnel::TunnelProvider,
) -> Result<(String, Vec<String>), String> {
    use crate::models::tunnel::TunnelProvider;

    match provider {
        TunnelProvider::Custom => Err(
            "Custom provider — install the binary yourself and set the binary path above.".into(),
        ),
        TunnelProvider::Cloudflare => {
            #[cfg(windows)]
            {
                Ok((
                    "winget".into(),
                    [
                        "install",
                        "--id",
                        "Cloudflare.cloudflared",
                        "-e",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                ))
            }
            #[cfg(target_os = "macos")]
            {
                Ok((
                    "brew".into(),
                    ["install", "cloudflare/cloudflare/cloudflared"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                ))
            }
            #[cfg(target_os = "linux")]
            {
                Err("Automatic installation is disabled on Linux. Install cloudflared with your distribution's trusted package manager, then test it again.".into())
            }
        }
        TunnelProvider::Ngrok => {
            #[cfg(windows)]
            {
                Ok((
                    "winget".into(),
                    [
                        "install",
                        "--id",
                        "ngrok.ngrok",
                        "-e",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                ))
            }
            #[cfg(target_os = "macos")]
            {
                Ok((
                    "brew".into(),
                    ["install", "ngrok/ngrok/ngrok"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                ))
            }
            #[cfg(target_os = "linux")]
            {
                Err("Automatic installation is disabled on Linux. Install ngrok with your distribution's trusted package manager, then test it again.".into())
            }
        }
    }
}

async fn forward_install_output<R>(
    reader: R,
    stream: &'static str,
    sender: tokio::sync::mpsc::Sender<(String, &'static str)>,
    diagnostic_error: Arc<std::sync::atomic::AtomicBool>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    const MAX_INSTALL_STREAM_BYTES: usize = 256 * 1024;
    const MAX_INSTALL_LINE_BYTES: usize = 64 * 1024;
    let mut reader = reader;
    let mut read_buffer = [0u8; 8 * 1024];
    let mut line = Vec::with_capacity(8 * 1024);
    let mut forwarded_bytes = 0usize;
    let mut truncation_reported = false;
    let mut sender_open = true;
    loop {
        match reader.read(&mut read_buffer).await {
            Ok(0) => {
                if sender_open && !line.is_empty() {
                    let content = String::from_utf8_lossy(&line);
                    if let Some(clean) = clean_install_line(&content) {
                        let _ = sender.try_send((clean, stream));
                    }
                }
                return;
            }
            Ok(read) => {
                let remaining = MAX_INSTALL_STREAM_BYTES.saturating_sub(forwarded_bytes);
                let accepted = read.min(remaining);
                for byte in &read_buffer[..accepted] {
                    if *byte == b'\n' {
                        if line.last() == Some(&b'\r') {
                            line.pop();
                        }
                        if sender_open {
                            let content = String::from_utf8_lossy(&line);
                            if let Some(clean) = clean_install_line(&content) {
                                match sender.try_send((clean, stream)) {
                                    Ok(())
                                    | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                        sender_open = false;
                                    }
                                }
                            }
                        }
                        line.clear();
                    } else if line.len() < MAX_INSTALL_LINE_BYTES {
                        line.push(*byte);
                    }
                }
                forwarded_bytes = forwarded_bytes.saturating_add(accepted);
                if accepted < read && !truncation_reported {
                    truncation_reported = true;
                    if sender_open {
                        let message = format!(
                            "Installer {stream} output exceeded 256 KiB; additional output is being drained but not forwarded."
                        );
                        if matches!(
                            sender.try_send((message, "stderr")),
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
                        ) {
                            sender_open = false;
                        }
                    }
                }
                // Even after the diagnostic forwarding budget is exhausted,
                // keep reading so the installer cannot block on a full pipe.
            }
            Err(error) => {
                diagnostic_error.store(true, std::sync::atomic::Ordering::Release);
                if sender_open {
                    let _ = sender.try_send((
                        format!("Installer {stream} could not be read: {error}"),
                        "stderr",
                    ));
                }
                return;
            }
        }
    }
}

// @group APIEndpoints > TunnelSettings : GET /tunnels/settings/install/stream?provider=... — SSE stream of install output
#[derive(Deserialize)]
struct InstallStreamQuery {
    provider: String,
}

async fn install_provider_stream(
    State(state): State<Arc<DaemonState>>,
    Query(q): Query<InstallStreamQuery>,
) -> axum::response::Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use crate::models::tunnel::TunnelProvider;
    use axum::response::sse::Event;
    use axum::response::Sse;
    use std::process::Stdio;

    let provider: TunnelProvider = match q.provider.as_str() {
        "cloudflare" => TunnelProvider::Cloudflare,
        "ngrok" => TunnelProvider::Ngrok,
        _ => TunnelProvider::Custom,
    };

    let install_cmd = tunnel_install_command(provider);

    let stream = async_stream::stream! {
        let (program, args) = match install_cmd {
            Err(msg) => {
                yield Ok(Event::default().data(json!({"line": msg, "stream": "stderr"}).to_string()));
                yield Ok(Event::default().data(json!({"done": true, "ok": false}).to_string()));
                return;
            }
            Ok(cmd) => cmd,
        };
        let _install_guard = state.tunnel_install_lock.lock().await;

        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args)
           .stdout(Stdio::piped())
           .stderr(Stdio::piped())
           .kill_on_drop(true);

        #[cfg(windows)]
        {
            cmd.creation_flags(0x0800_0000 | 0x0100_0000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                yield Ok(Event::default().data(json!({"line": format!("Failed to start installer: {e}"), "stream": "stderr"}).to_string()));
                yield Ok(Event::default().data(json!({"done": true, "ok": false}).to_string()));
                return;
            }
        };
        let Some(pid) = child.id() else {
            let _ = child.kill().await;
            let _ = child.wait().await;
            yield Ok(Event::default().data(json!({"line": "Installer did not expose a process id.", "stream": "stderr"}).to_string()));
            yield Ok(Event::default().data(json!({"done": true, "ok": false}).to_string()));
            return;
        };
        let process_tree = match crate::process::tree::ProcessTreeGuard::attach_or_terminate(&mut child, pid, &format!("tunnel-installer-stream-{pid}")).await {
            Ok(tree) => tree,
            Err(error) => {
                yield Ok(Event::default().data(json!({"line": format!("Failed to contain installer process tree: {error}"), "stream": "stderr"}).to_string()));
                yield Ok(Event::default().data(json!({"done": true, "ok": false}).to_string()));
                return;
            }
        };

        let (line_tx, mut line_rx) = tokio::sync::mpsc::channel(64);
        let diagnostic_error = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(forward_install_output(
                stdout,
                "stdout",
                line_tx.clone(),
                Arc::clone(&diagnostic_error),
            ));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_install_output(
                stderr,
                "stderr",
                line_tx.clone(),
                Arc::clone(&diagnostic_error),
            ));
        }
        drop(line_tx);

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
        loop {
            match tokio::time::timeout_at(deadline, line_rx.recv()).await {
                Ok(Some((line, stream))) => {
                    yield Ok(Event::default().data(json!({"line": line, "stream": stream}).to_string()));
                }
                Ok(None) => break,
                Err(_) => {
                    let cleanup_error = crate::process::identity::kill_spawned_process(&mut child, pid)
                        .await
                        .err()
                        .map(|error| error.to_string());
                    drop(process_tree);
                    let wait_error = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.err();
                    if cleanup_error.is_some() || wait_error.is_some() {
                        yield Ok(Event::default().data(json!({
                            "line": format!("Installer cleanup failed: terminate={cleanup_error:?}, wait={wait_error:?}"),
                            "stream": "stderr"
                        }).to_string()));
                    }
                    yield Ok(Event::default().data(json!({"line": "Provider installation timed out after 15 minutes.", "stream": "stderr"}).to_string()));
                    yield Ok(Event::default().data(json!({"done": true, "ok": false}).to_string()));
                    return;
                }
            }
        }

        let ok = match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(Ok(status)) => {
                drop(process_tree);
                status.success() && !diagnostic_error.load(std::sync::atomic::Ordering::Acquire)
            },
            wait_result => {
                let cleanup_error = crate::process::identity::kill_spawned_process(&mut child, pid)
                    .await
                    .err()
                    .map(|error| error.to_string());
                drop(process_tree);
                let wait_error = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.err();
                if cleanup_error.is_some() || wait_error.is_some() {
                    yield Ok(Event::default().data(json!({
                        "line": format!("Installer wait/cleanup failed: wait={wait_result:?}, terminate={cleanup_error:?}, final_wait={wait_error:?}"),
                        "stream": "stderr"
                    }).to_string()));
                }
                false
            }
        };
        yield Ok(Event::default().data(json!({"done": true, "ok": ok}).to_string()));
    };

    Sse::new(stream)
}

// @group UnitTests : clean_install_line — CR spinner stripping
#[cfg(test)]
mod tests {
    use super::*;

    // @group UnitTests > CleanLine : Plain line with no CR passes through unchanged
    #[test]
    fn test_clean_plain_line() {
        let result = clean_install_line("Successfully installed cloudflared\n");
        assert_eq!(result.unwrap(), "Successfully installed cloudflared");
    }

    // @group UnitTests > CleanLine : Last non-empty CR segment is kept, spinner frames discarded
    #[test]
    fn test_clean_spinner_frames_discarded() {
        let raw = "\r   - \r   \\ \r   | \r   / \rFound cloudflared Version 2025.8.1\n";
        let result = clean_install_line(raw).unwrap();
        assert_eq!(result, "Found cloudflared Version 2025.8.1");
    }

    // @group UnitTests > CleanLine : Windows CRLF line endings are stripped
    #[test]
    fn test_clean_crlf_endings() {
        let result = clean_install_line("Downloading installer\r\n");
        assert_eq!(result.unwrap(), "Downloading installer");
    }

    // @group UnitTests > CleanLine : Line with only whitespace / CR returns None
    #[test]
    fn test_clean_whitespace_only_returns_none() {
        assert!(clean_install_line("   \r   \r   \n").is_none());
    }

    // @group UnitTests > CleanLine : Completely empty string returns None
    #[test]
    fn test_clean_empty_returns_none() {
        assert!(clean_install_line("").is_none());
    }

    // @group UnitTests > CleanLine : Leading and trailing spaces are trimmed from the result
    #[test]
    fn test_clean_trims_whitespace() {
        let result = clean_install_line("  padded content  \n");
        assert_eq!(result.unwrap(), "padded content");
    }

    // @group UnitTests > CleanLine : Multiple CR segments — only the last meaningful one is returned
    #[test]
    fn test_clean_multiple_cr_segments() {
        let raw = "\rfirst\rsecond\rthird\n";
        assert_eq!(clean_install_line(raw).unwrap(), "third");
    }

    #[test]
    fn test_clean_line_is_bounded() {
        let result = clean_install_line(&"x".repeat(8 * 1024)).unwrap();
        assert!(result.chars().count() < 4 * 1024 + 20);
        assert!(result.ends_with("…[truncated]"));
    }
}
