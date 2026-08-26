// @group APIEndpoints : System / daemon management endpoints

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_ENV_FILES_PER_PROCESS: usize = 200;

fn acquire_blocking_io(
    state: &Arc<DaemonState>,
    operation: &'static str,
) -> Result<tokio::sync::OwnedSemaphorePermit, ApiError> {
    state
        .blocking_io_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: axum::http::StatusCode::TOO_MANY_REQUESTS,
            message: format!("{operation} capacity is busy; retry shortly"),
        })
}

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/stats", get(system_stats))
        .route("/paths", get(paths))
        .route("/check-env", get(check_env))
        .route("/list-env", get(list_env_files))
        .route("/read-env", get(read_env_file))
        .route("/write-env", post(write_env_file))
        .route("/sync-env", post(sync_env_files))
        .route("/browse", get(browse_dir))
        .route("/save", post(save_state))
        .route("/resurrect", post(resurrect_state))
        .route("/shutdown", post(shutdown))
        .route("/restart", post(restart))
        .route("/open-folder", post(open_folder))
        .with_state(state)
}

// @group Utilities > EnvFiles : Returns true if a filename is an env-style file (.env, .env.*, *.env)
pub fn is_env_filename(name: &str) -> bool {
    crate::config::env_file::is_safe_env_filename(name)
}

// @group Utilities > EnvFiles : Lists all env-style files in a directory (sorted alphabetically)
pub fn list_env_files_in(dir: &str) -> anyhow::Result<Vec<(String, String)>> {
    let path = std::path::Path::new(dir);
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_env_filename(&name) {
            anyhow::ensure!(
                files.len() < MAX_ENV_FILES_PER_PROCESS,
                "registered process directory contains more than {MAX_ENV_FILES_PER_PROCESS} env files"
            );
            files.push((name, entry.path().to_string_lossy().to_string()));
        }
    }
    // .env first, then alphabetical
    files.sort_by(|a, b| {
        if a.0 == ".env" {
            std::cmp::Ordering::Less
        } else if b.0 == ".env" {
            std::cmp::Ordering::Greater
        } else {
            a.0.cmp(&b.0)
        }
    });
    Ok(files)
}

// @group APIEndpoints > System : GET /system/paths
async fn paths() -> Json<Value> {
    Json(json!({
        "data_dir": crate::config::paths::data_dir().to_string_lossy(),
        "log_dir":  crate::config::paths::log_dir().to_string_lossy(),
    }))
}

// @group APIEndpoints > System : GET /system/health
async fn health(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let uptime = (Utc::now() - state.started_at).num_seconds().max(0) as u64;
    let count = state.manager.list().await.len();
    let persistence_error = state.background_persistence_error.read().await.clone();
    Json(json!({
        "status": if persistence_error.is_some() { "degraded" } else { "ok" },
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "port": state.config.port,
        "uptime_secs": uptime,
        "process_count": count,
        "persistence_healthy": persistence_error.is_none(),
        "persistence_error": persistence_error.as_ref().map(|_| "background persistence failed"),
    }))
}

// @group APIEndpoints > System : POST /system/save
async fn save_state(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    state.save_to_disk().await.map_err(ApiError::from)?;
    Ok(Json(json!({ "success": true, "message": "state saved" })))
}

// @group APIEndpoints > System : POST /system/resurrect
async fn resurrect_state(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    if !state.manager.list().await.is_empty() {
        return Err(ApiError::conflict(
            "resurrect is only allowed when the runtime registry is empty",
        ));
    }
    let saved = state.load_from_disk().await.map_err(ApiError::from)?;
    let count = saved.apps.len();
    state.restore(saved).await;
    match state.save_to_disk().await {
        Ok(()) => Ok(Json(json!({
            "success": true,
            "status": "complete",
            "message": format!("restored {count} processes"),
            "persistence": { "status": "committed", "error": null }
        }))),
        Err(error) => {
            let reference = uuid::Uuid::new_v4();
            tracing::error!(%reference, %error, "resurrect persistence failed after runtime restore");
            let internal_message = format!(
                "restored {count} processes in memory, but normalized persistence failed: {error}"
            );
            *state.background_persistence_error.write().await = Some(internal_message);
            let message = format!(
                "restored {count} processes in memory, but normalized persistence failed (reference: {reference})"
            );
            Ok(Json(json!({
                "success": false,
                "status": "partial",
                "message": message,
                "persistence": { "status": "failed", "error": format!("persistence failed (reference: {reference})") }
            })))
        }
    }
}

// @group APIEndpoints > System : GET /system/browse?path=<dir>
// Lists directory contents. Empty path → Windows drive list. Dirs sorted first, then alpha.
async fn browse_dir(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let path_str = params.get("path").cloned().unwrap_or_default();

    // Windows: empty path → enumerate all present drive letters
    #[cfg(target_os = "windows")]
    if path_str.is_empty() {
        let drives: Vec<Value> = (b'A'..=b'Z')
            .filter_map(|c| {
                let drive = format!("{}:\\", c as char);
                if std::path::Path::new(&drive).exists() {
                    Some(json!({ "name": drive, "path": drive, "is_dir": true }))
                } else {
                    None
                }
            })
            .collect();
        return Ok(Json(
            json!({ "path": "", "parent": Value::Null, "entries": drives, "truncated": false }),
        ));
    }

    // Unix: empty path → root
    #[cfg(not(target_os = "windows"))]
    let path_str = if path_str.is_empty() {
        "/".to_string()
    } else {
        path_str
    };

    let listing = tokio::task::spawn_blocking(move || browse_directory(path_str))
        .await
        .map_err(|error| ApiError::internal(format!("directory browse task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("failed to browse directory: {error}")))?;
    Ok(Json(listing))
}

fn browse_directory(path_str: String) -> anyhow::Result<Value> {
    const MAX_BROWSE_ENTRIES: usize = 2_000;
    let path = std::path::Path::new(&path_str);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().to_string());
    let mut items = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(path)? {
        if items.len() == MAX_BROWSE_ENTRIES {
            truncated = true;
            break;
        }
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type()?.is_dir();
        let entry_path = entry.path().to_string_lossy().to_string();
        items.push(json!({ "name": name, "path": entry_path, "is_dir": is_dir }));
    }
    items.sort_by(|a, b| {
        let a_dir = a["is_dir"].as_bool().unwrap_or(false);
        let b_dir = b["is_dir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Ok(json!({ "path": path_str, "parent": parent, "entries": items, "truncated": truncated }))
}

// @group Security > EnvFiles : Resolve only direct env-file children of registered process cwd values
async fn registered_env_root(
    state: &DaemonState,
    requested: &std::path::Path,
) -> Result<std::path::PathBuf, ApiError> {
    let requested = requested.to_path_buf();
    let registered_cwds = state
        .manager
        .snapshot()
        .await
        .into_iter()
        .map(|snapshot| snapshot.config.cwd.unwrap_or_else(|| ".".into()))
        .collect::<Vec<_>>();
    let _permit = state
        .blocking_io_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: axum::http::StatusCode::TOO_MANY_REQUESTS,
            message: "env path validation capacity is busy; retry shortly".into(),
        })?;
    let matched = tokio::task::spawn_blocking(move || -> std::io::Result<Option<_>> {
        let requested = std::fs::canonicalize(requested)?;
        Ok(registered_cwds
            .into_iter()
            .any(|cwd| std::fs::canonicalize(cwd).ok().as_ref() == Some(&requested))
            .then_some(requested))
    })
    .await
    .map_err(|error| ApiError::internal(format!("env path validation task failed: {error}")))?
    .map_err(|error| ApiError::bad_request(format!("invalid process cwd: {error}")))?;
    matched.ok_or_else(|| {
        ApiError::bad_request("env access is limited to the cwd of a registered process")
    })
}

async fn registered_env_path(
    state: &DaemonState,
    requested: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let requested = std::path::Path::new(requested);
    if !requested.is_absolute() {
        return Err(ApiError::bad_request("env path must be absolute"));
    }
    let filename = requested
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ApiError::bad_request("env path has no valid filename"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| ApiError::bad_request("env path has no parent directory"))?;
    let root = registered_env_root(state, parent).await?;
    crate::config::env_file::resolve_process_env_path(&root, filename)
        .map_err(|error| ApiError::bad_request(error.to_string()))
}

// @group APIEndpoints > System : GET /system/check-env?path=<registered-process-cwd>
async fn check_env(
    State(state): State<Arc<DaemonState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let directory = params.get("path").cloned().unwrap_or_default();
    let root = registered_env_root(&state, std::path::Path::new(&directory)).await?;
    let env_path = crate::config::env_file::resolve_process_env_path(&root, ".env")
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(json!({
        "exists": env_path.is_file(),
        "path": env_path.to_string_lossy(),
    })))
}

// @group APIEndpoints > System : GET /system/list-env?path=<registered-process-cwd>
async fn list_env_files(
    State(state): State<Arc<DaemonState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let directory = params.get("path").cloned().unwrap_or_default();
    let root = registered_env_root(&state, std::path::Path::new(&directory)).await?;
    let _permit = acquire_blocking_io(&state, "env listing")?;
    let files = tokio::task::spawn_blocking(move || list_env_files_in(&root.to_string_lossy()))
        .await
        .map_err(|error| ApiError::internal(format!("env listing task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("failed to list env files: {error}")))?;
    let result: Vec<Value> = files
        .into_iter()
        .map(|(name, path)| json!({ "name": name, "path": path }))
        .collect();
    Ok(Json(json!({ "files": result })))
}

// @group APIEndpoints > System : GET /system/read-env?path=<registered-process-env-file>
async fn read_env_file(
    State(state): State<Arc<DaemonState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let file_path = params.get("path").cloned().unwrap_or_default();
    let path = registered_env_path(&state, &file_path).await?;
    if !path.exists() {
        return Ok(Json(json!({ "content": "", "exists": false })));
    }
    let _permit = acquire_blocking_io(&state, "env read")?;
    let content = tokio::task::spawn_blocking(move || {
        crate::config::env_file::read_env_file_text(
            &path,
            crate::config::env_file::MAX_ENV_FILE_BYTES,
        )
    })
    .await
    .map_err(|error| ApiError::internal(format!("env read task failed: {error}")))?
    .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(json!({ "content": content, "exists": true })))
}

// @group APIEndpoints > System : POST /system/write-env
async fn write_env_file(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    const MAX_ENV_BYTES: usize = 1024 * 1024;
    let file_path = body["path"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("path is required"))?;
    let content = body["content"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("content must be a string"))?;
    if content.len() > MAX_ENV_BYTES {
        return Err(ApiError::bad_request("env file exceeds the 1 MiB limit"));
    }
    let _config_guard = state.config_mutation_lock.lock().await;
    let path = registered_env_path(&state, file_path).await?;
    let owned_content = content.as_bytes().to_vec();
    let write_path = path.clone();
    tokio::task::spawn_blocking(move || {
        crate::config::atomic_file::write_with_backup(&write_path, &owned_content, None)
    })
    .await
    .map_err(|error| ApiError::internal(format!("env write task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("failed to write env file: {error}")))?;
    Ok(Json(
        json!({ "success": true, "path": path.to_string_lossy() }),
    ))
}

// @group APIEndpoints > System : POST /system/sync-env
async fn sync_env_files(
    State(state): State<Arc<DaemonState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let source_path_str = body["source_path"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("source_path is required"))?;
    let _config_guard = state.config_mutation_lock.lock().await;
    let source_path = registered_env_path(&state, source_path_str).await?;
    let root = source_path
        .parent()
        .ok_or_else(|| ApiError::bad_request("cannot determine env directory"))?
        .to_path_buf();
    let _permit = acquire_blocking_io(&state, "env synchronization")?;
    let (synced, errors) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        const MAX_SYNC_INPUT_BYTES: u64 = 16 * 1024 * 1024;
        const SYNC_TIME_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);
        let started = std::time::Instant::now();
        let source_content = crate::config::env_file::read_env_file_text(
            &source_path,
            crate::config::env_file::MAX_ENV_FILE_BYTES,
        )?;
        let source_keys: Vec<String> = source_content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return None;
                }
                trimmed
                    .split_once('=')
                    .map(|(key, _)| key.trim().to_string())
            })
            .collect();
        let source_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let env_files = list_env_files_in(&root.to_string_lossy())?;
        let mut aggregate_bytes = source_content.len() as u64;
        for (name, path) in &env_files {
            if name == source_name {
                continue;
            }
            let size = std::fs::symlink_metadata(path)?.len();
            aggregate_bytes = aggregate_bytes
                .checked_add(size)
                .ok_or_else(|| anyhow::anyhow!("env synchronization input size overflow"))?;
            anyhow::ensure!(
                aggregate_bytes <= MAX_SYNC_INPUT_BYTES,
                "env synchronization input exceeds the {MAX_SYNC_INPUT_BYTES} byte aggregate limit"
            );
        }
        let mut synced = 0usize;
        let mut errors = Vec::new();
        for (name, _) in env_files {
            if name == source_name {
                continue;
            }
            if started.elapsed() >= SYNC_TIME_BUDGET {
                errors.push(
                    "env synchronization exceeded its 10-second budget; remaining files were skipped"
                        .to_string(),
                );
                break;
            }
            let target_path = match crate::config::env_file::resolve_process_env_path(&root, &name)
            {
                Ok(path) => path,
                Err(error) => {
                    errors.push(format!("{name}: {error}"));
                    continue;
                }
            };
            let existing_content = match crate::config::env_file::read_env_file_text(
                &target_path,
                crate::config::env_file::MAX_ENV_FILE_BYTES,
            ) {
                Ok(content) => content,
                Err(error) => {
                    errors.push(format!("{name}: {error}"));
                    continue;
                }
            };
            let existing_keys: std::collections::HashSet<String> = existing_content
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        return None;
                    }
                    trimmed
                        .split_once('=')
                        .map(|(key, _)| key.trim().to_string())
                })
                .collect();
            let mut additions = String::new();
            for key in &source_keys {
                if !existing_keys.contains(key) {
                    additions.push_str(&format!("{key}=\n"));
                }
            }
            if additions.is_empty() {
                continue;
            }
            let separator = if existing_content.ends_with('\n') || existing_content.is_empty() {
                ""
            } else {
                "\n"
            };
            let new_content = format!("{existing_content}{separator}{additions}");
            if new_content.len() as u64 > crate::config::env_file::MAX_ENV_FILE_BYTES {
                errors.push(format!(
                    "{name}: synchronized env content would exceed the {} byte limit",
                    crate::config::env_file::MAX_ENV_FILE_BYTES
                ));
                continue;
            }
            match crate::config::atomic_file::write_with_backup(
                &target_path,
                new_content.as_bytes(),
                None,
            ) {
                Ok(()) => synced += 1,
                Err(error) => errors.push(format!("{name}: {error}")),
            }
        }
        Ok((synced, errors))
    })
    .await
    .map_err(|error| ApiError::internal(format!("env sync task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("env sync failed: {error}")))?;

    Ok(Json(json!({
        "success": errors.is_empty(),
        "status": if errors.is_empty() { "complete" } else { "partial" },
        "synced_files": synced,
        "errors": errors,
    })))
}

// @group APIEndpoints > System : POST /system/shutdown
async fn shutdown(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    state
        .save_to_disk()
        .await
        .map_err(|error| ApiError::internal(format!("shutdown cancelled: {error}")))?;
    let shutdown_state = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        shutdown_state.request_shutdown();
    });
    Ok(Json(
        json!({ "success": true, "message": "daemon shutting down" }),
    ))
}

// @group APIEndpoints > System : GET /system/stats — system-wide CPU %, RAM, and GPU (nvidia-smi)
async fn system_stats(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _permit = state
        .blocking_io_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: axum::http::StatusCode::TOO_MANY_REQUESTS,
            message: "system metrics capacity is busy; retry shortly".into(),
        })?;
    // sysinfo is sync and needs a sleep between CPU reads for an accurate %; use spawn_blocking
    let system_task = tokio::task::spawn_blocking(|| {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        std::thread::sleep(std::time::Duration::from_millis(500));
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        (
            sys.global_cpu_usage(),
            sys.used_memory(),
            sys.total_memory(),
        )
    });
    let (system_result, gpu) = tokio::join!(system_task, gpu_stats());
    let (cpu, ram_used, ram_total) = system_result
        .map_err(|error| ApiError::internal(format!("system metrics task failed: {error}")))?;

    Ok(Json(json!({
        "cpu_percent": cpu,
        "ram_used_bytes": ram_used,
        "ram_total_bytes": ram_total,
        "gpu": gpu,
    })))
}

// @group Utilities > System : Parse nvidia-smi output for GPU utilization and VRAM — returns None if unavailable
async fn gpu_stats() -> Option<Value> {
    use tokio::io::AsyncReadExt;

    const MAX_GPU_OUTPUT_BYTES: u64 = 64 * 1024;
    let mut cmd = tokio::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    // Suppress console window flash on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.as_std_mut().creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.kill_on_drop(true);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut bytes = Vec::new();
    let read_result = tokio::time::timeout_at(
        deadline,
        stdout
            .take(MAX_GPU_OUTPUT_BYTES + 1)
            .read_to_end(&mut bytes),
    )
    .await;
    if !matches!(read_result, Ok(Ok(_))) || bytes.len() as u64 > MAX_GPU_OUTPUT_BYTES {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return None;
    }
    let status = tokio::time::timeout_at(deadline, child.wait())
        .await
        .ok()?
        .ok()?;
    if !status.success() {
        return None;
    }
    let stdout = String::from_utf8(bytes).ok()?;
    let line = stdout.lines().next()?.trim();
    let parts: Vec<&str> = line.splitn(4, ',').map(str::trim).collect();
    if parts.len() < 4 {
        return None;
    }
    let util: f32 = parts[1].parse().ok()?;
    let vram_used_mb: u64 = parts[2].parse().ok()?;
    let vram_total_mb: u64 = parts[3].parse().ok()?;
    Some(json!({
        "name": parts[0],
        "utilization_percent": util,
        "vram_used_bytes": vram_used_mb * 1024 * 1024,
        "vram_total_bytes": vram_total_mb * 1024 * 1024,
    }))
}

// @group APIEndpoints > System : POST /system/restart
// Saves state, spawns a delayed replacement, then requests graceful shutdown.
// Managed processes survive because runner uses CREATE_BREAKAWAY_FROM_JOB on Windows.
async fn wait_for_replacement_handoff(
    child: &mut std::process::Child,
    path: &std::path::Path,
    expected_token: &str,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("replacement daemon exited before handoff with status {status}");
        }
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                anyhow::ensure!(bytes.len() <= 1_024, "restart handoff was oversized");
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(
                    value.get("token").and_then(serde_json::Value::as_str) == Some(expected_token)
                        && value.get("pid").and_then(serde_json::Value::as_u64)
                            == Some(child.id() as u64)
                        && value.get("phase").and_then(serde_json::Value::as_str)
                            == Some("prepared"),
                    "restart handoff identity did not match the spawned replacement"
                );
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("replacement daemon did not acknowledge startup within 10 seconds");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

async fn restart(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let mutation_guard = state.state_mutation_lock.lock().await;
    let host = state.config.host.clone();
    let port = state.config.port;
    state
        .save_to_disk()
        .await
        .map_err(|error| ApiError::internal(format!("restart cancelled: {error}")))?;
    let exe = std::env::current_exe()
        .map_err(|error| ApiError::internal(format!("restart cancelled: {error}")))?;
    let handoff_token = uuid::Uuid::new_v4().to_string();
    let handoff_path =
        crate::config::paths::data_dir().join(format!(".restart-handoff-{handoff_token}.json"));

    // Spawn a detached copy directly. The child waits for this listener to
    // release the port through a private environment flag, avoiding shell
    // interpolation of the executable path or arguments.
    #[cfg(target_os = "windows")]
    let mut replacement = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        std::process::Command::new(&exe)
            .arg("--internal-daemon")
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .env("ALTER_RESTART_WAIT_FOR_PORT", "1")
            .env("ALTER_RESTART_HANDOFF_PATH", &handoff_path)
            .env("ALTER_RESTART_HANDOFF_TOKEN", &handoff_token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map_err(|error| ApiError::internal(format!("restart cancelled: {error}")))?
    };
    #[cfg(not(target_os = "windows"))]
    let mut replacement = {
        std::process::Command::new(&exe)
            .arg("--internal-daemon")
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .env("ALTER_RESTART_WAIT_FOR_PORT", "1")
            .env("ALTER_RESTART_HANDOFF_PATH", &handoff_path)
            .env("ALTER_RESTART_HANDOFF_TOKEN", &handoff_token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| ApiError::internal(format!("restart cancelled: {error}")))?
    };
    if let Err(error) =
        wait_for_replacement_handoff(&mut replacement, &handoff_path, &handoff_token).await
    {
        drop(mutation_guard);
        let cleanup = crate::daemon::terminate_failed_replacement(replacement).await;
        if let Err(remove_error) = tokio::fs::remove_file(&handoff_path).await {
            if remove_error.kind() != std::io::ErrorKind::NotFound {
                tracing::error!(path = %handoff_path.display(), %remove_error, "failed to remove rejected restart handoff");
            }
        }
        if let Err(cleanup_error) = cleanup {
            tracing::error!(%error, %cleanup_error, "replacement daemon failed restart handoff and bounded cleanup");
            return Err(ApiError::internal(
                "restart cancelled but replacement cleanup could not be confirmed",
            ));
        }
        tracing::error!(%error, "replacement daemon failed restart handoff");
        return Err(ApiError::internal(
            "restart cancelled because the replacement daemon was not ready",
        ));
    }
    state
        .arm_restart(crate::daemon::state::RestartAttempt {
            child: replacement,
            handoff_path,
            handoff_token,
        })
        .map_err(|error| {
            ApiError::internal(format!("restart cancelled before shutdown: {error}"))
        })?;
    state.request_restart_shutdown();
    Ok(Json(
        json!({ "success": true, "message": "daemon restarting" }),
    ))
}

// @group APIEndpoints > System : POST /system/open-folder — open a path in the OS file explorer
#[derive(serde::Deserialize)]
struct OpenFolderRequest {
    path: String,
}

async fn open_folder(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<OpenFolderRequest>,
) -> Result<Json<Value>, ApiError> {
    let requested = std::path::Path::new(&req.path);
    if !requested.is_absolute() || looks_like_uri(&req.path) || is_network_path(requested) {
        return Err(ApiError::bad_request(
            "folder path must be an absolute local filesystem path",
        ));
    }
    let requested = std::fs::canonicalize(requested)
        .map_err(|error| ApiError::bad_request(format!("invalid folder path: {error}")))?;
    if !requested.is_dir() {
        return Err(ApiError::bad_request("folder path is not a directory"));
    }
    let mut allowed = std::fs::canonicalize(crate::config::paths::data_dir())
        .ok()
        .is_some_and(|root| requested.starts_with(root));
    if !allowed {
        for snapshot in state.manager.snapshot().await {
            let cwd = snapshot.config.cwd.as_deref().unwrap_or(".");
            if std::fs::canonicalize(cwd)
                .ok()
                .is_some_and(|root| requested.starts_with(root))
            {
                allowed = true;
                break;
            }
        }
    }
    if !allowed {
        return Err(ApiError::bad_request(
            "folder access is limited to registered process directories and Alter data",
        ));
    }

    #[cfg(target_os = "windows")]
    let spawn = std::process::Command::new("explorer.exe")
        .arg(&requested)
        .spawn();
    #[cfg(target_os = "macos")]
    let spawn = std::process::Command::new("open").arg(&requested).spawn();
    #[cfg(target_os = "linux")]
    let spawn = std::process::Command::new("xdg-open")
        .arg(&requested)
        .spawn();
    spawn.map_err(|error| ApiError::internal(format!("failed to open folder: {error}")))?;
    Ok(Json(json!({ "success": true })))
}

fn looks_like_uri(path: &str) -> bool {
    path.contains("://") || path.starts_with("file:")
}

#[cfg(windows)]
fn is_network_path(path: &std::path::Path) -> bool {
    use std::path::{Component, Prefix};
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
    )
}

#[cfg(not(windows))]
fn is_network_path(_path: &std::path::Path) -> bool {
    false
}

#[cfg(test)]
mod open_folder_tests {
    use super::*;

    #[test]
    fn uri_like_paths_are_rejected_before_os_launch() {
        assert!(looks_like_uri("https://example.com"));
        assert!(looks_like_uri("file:///tmp/test"));
        assert!(!looks_like_uri("C:\\work\\project"));
    }
}
