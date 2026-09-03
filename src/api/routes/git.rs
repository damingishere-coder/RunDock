// @group APIEndpoints : Git integration — branch info, pull, dependency reinstall, restart

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use anyhow::Context;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/{id}/git", get(git_info))
        .route("/{id}/git/pull", post(git_pull))
        .with_state(state)
}

// @group Utilities > Git : Resolve a string process ID to Uuid
async fn resolve(state: &DaemonState, id_str: &str) -> Result<Uuid, ApiError> {
    state
        .manager
        .resolve_id(id_str)
        .await
        .map_err(|_| ApiError::not_found(format!("process not found: {id_str}")))
}

// @group Utilities > Git : Run a bounded metadata command and preserve failure semantics
fn git_out(dir: &FsPath, args: &[&str]) -> anyhow::Result<String> {
    const MAX_OUTPUT_BYTES: u64 = 512 * 1024;
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

    let output_path = std::env::temp_dir().join(format!("alter-git-{}.log", Uuid::new_v4()));
    let output_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&output_path)?;
    let mut cmd = std::process::Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_file))
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    };
    let result = (|| -> anyhow::Result<String> {
        let mut child = cmd.spawn()?;
        let started = std::time::Instant::now();
        let status = loop {
            if std::fs::metadata(&output_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                > MAX_OUTPUT_BYTES
                || started.elapsed() >= COMMAND_TIMEOUT
            {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("git metadata command exceeded its time or output limit");
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(error) => return Err(error.into()),
            }
        };
        if !status.success() {
            anyhow::bail!(
                "git metadata command failed with exit code {:?}",
                status.code()
            );
        }
        if std::fs::metadata(&output_path)?.len() > MAX_OUTPUT_BYTES {
            anyhow::bail!("git metadata output exceeded {MAX_OUTPUT_BYTES} bytes");
        }
        Ok(std::fs::read_to_string(&output_path)?.trim().to_string())
    })();
    if let Err(error) = std::fs::remove_file(&output_path) {
        tracing::warn!(path = %output_path.display(), %error, "git metadata output cleanup failed");
    }
    result
}

// @group Utilities > Git : Run a bounded command without buffering untrusted output in memory
async fn cmd_output(program: &str, args: &[&str], dir: &FsPath) -> anyhow::Result<String> {
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(15 * 60);

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        // Dependency lifecycle hooks may print credentials or environment
        // variables. Do not persist or return their raw output from the daemon.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000 | 0x0100_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }
    let mut child = cmd.spawn()?;
    let child_pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("spawned {program} command has no process id"))?;
    let _process_tree = crate::process::tree::ProcessTreeGuard::attach_or_terminate(
        &mut child,
        child_pid,
        &format!("git-{child_pid}"),
    )
    .await
    .with_context(|| format!("failed to contain {program} process tree"))?;
    let started = tokio::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!("{program} timed out after 15 minutes");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    if !status.success() {
        anyhow::bail!("{program} failed with exit code {:?}", status.code());
    }
    Ok(format!("{program} completed successfully"))
}

fn dependency_failure(error: anyhow::Error) -> ApiError {
    ApiError::internal(format!(
        "git pull completed, but dependency installation failed ({error}); the checkout changed and the process was not restarted"
    ))
}

#[cfg(windows)]
fn package_program(name: &'static str) -> &'static str {
    match name {
        "npm" => "npm.cmd",
        "yarn" => "yarn.cmd",
        "pnpm" => "pnpm.cmd",
        other => other,
    }
}

#[cfg(not(windows))]
fn package_program(name: &'static str) -> &'static str {
    name
}

// @group Utilities > Git : Detect package manager from working directory
fn detect_pkg_manager(dir: &FsPath) -> &'static str {
    if dir.join("package.json").exists() {
        if dir.join("pnpm-lock.yaml").exists() {
            return "pnpm";
        }
        if dir.join("yarn.lock").exists() {
            return "yarn";
        }
        return "npm";
    }
    if dir.join("Cargo.toml").exists() {
        return "cargo";
    }
    if dir.join("requirements.txt").exists() {
        return "pip";
    }
    if dir.join("pyproject.toml").exists() {
        return "pip";
    }
    if dir.join("Pipfile").exists() {
        return "pip";
    }
    if dir.join("go.mod").exists() {
        return "go";
    }
    "none"
}

// @group APIEndpoints > Git : GET /processes/:id/git — branch, SHA, dirty state, ahead/behind
async fn git_info(
    Path(id_str): Path<String>,
    State(state): State<Arc<DaemonState>>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.clone().unwrap_or_else(|| ".".to_string());
    let _permit = state
        .blocking_io_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: axum::http::StatusCode::TOO_MANY_REQUESTS,
            message: "git metadata capacity is busy; retry shortly".into(),
        })?;

    let result = tokio::task::spawn_blocking(move || {
        let dir = PathBuf::from(&cwd);

        // Check if it's a git repo
        let is_git =
            dir.join(".git").exists() || git_out(&dir, &["rev-parse", "--git-dir"]).is_ok();

        if !is_git {
            return Ok(json!({
                "is_git_repo": false,
                "dirty": false,
                "ahead": 0,
                "behind": 0,
                "upstream_available": false,
                "ahead_behind_error": null,
                "pkg_manager": detect_pkg_manager(&dir),
            }));
        }

        let branch = git_out(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let sha = git_out(&dir, &["log", "-1", "--format=%H"])?;
        let sha_short = (!sha.is_empty())
            .then_some(sha.as_str())
            .map(|s| &s[..s.len().min(7)])
            .map(str::to_string);
        let message = git_out(&dir, &["log", "-1", "--format=%s"])?;
        let author = git_out(&dir, &["log", "-1", "--format=%an"])?;
        let date = git_out(&dir, &["log", "-1", "--format=%ci"])?;
        let dirty = !git_out(&dir, &["status", "--porcelain"])?.is_empty();

        // Preserve an explicit unavailable state instead of presenting a failed comparison as 0/0.
        let ahead_behind = git_out(
            &dir,
            &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
        );
        let parsed_counts = ahead_behind.as_ref().ok().and_then(|s| {
            let mut parts = s.split_whitespace();
            let a = parts.next()?.parse::<i64>().ok()?;
            let b = parts.next()?.parse::<i64>().ok()?;
            Some((a, b))
        });
        let (ahead, behind, upstream_available, ahead_behind_error) = match parsed_counts {
            Some((ahead, behind)) => (ahead, behind, true, Value::Null),
            None => (
                0,
                0,
                false,
                Value::String(
                    "upstream comparison is unavailable; configure a tracking branch and retry"
                        .to_string(),
                ),
            ),
        };

        let pkg_manager = detect_pkg_manager(&dir);

        Ok(json!({
            "is_git_repo": true,
            "branch": branch,
            "sha": sha,
            "sha_short": sha_short,
            "message": message,
            "author": author,
            "date": date,
            "dirty": dirty,
            "ahead": ahead,
            "behind": behind,
            "upstream_available": upstream_available,
            "ahead_behind_error": ahead_behind_error,
            "pkg_manager": pkg_manager,
        }))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e: anyhow::Error| ApiError::internal(e.to_string()))?;

    Ok(Json(result))
}

// @group APIEndpoints > Git : POST /processes/:id/git/pull — git pull + install deps + restart
async fn git_pull(
    Path(id_str): Path<String>,
    State(state): State<Arc<DaemonState>>,
) -> Result<Json<Value>, ApiError> {
    let _git_guard = state.git_operation_lock.lock().await;
    let id = resolve(&state, &id_str).await?;
    let info = state.manager.get(id).await.map_err(ApiError::from)?;
    let cwd = info.cwd.clone().unwrap_or_else(|| ".".to_string());

    let dir = std::fs::canonicalize(&cwd)
        .map_err(|error| ApiError::bad_request(format!("invalid process cwd: {error}")))?;
    if !dir.is_dir() {
        return Err(ApiError::bad_request("process cwd is not a directory"));
    }

    let pull_output = cmd_output("git", &["pull", "--ff-only"], &dir)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let pkg_manager = detect_pkg_manager(&dir);
    let deps_output = match pkg_manager {
        "npm" if dir.join("package-lock.json").exists() => Some(
            cmd_output(package_program("npm"), &["ci"], &dir)
                .await
                .map_err(dependency_failure)?,
        ),
        "npm" => Some(
            cmd_output(package_program("npm"), &["install"], &dir)
                .await
                .map_err(dependency_failure)?,
        ),
        "yarn" => Some(
            cmd_output(
                package_program("yarn"),
                &["install", "--frozen-lockfile"],
                &dir,
            )
            .await
            .map_err(dependency_failure)?,
        ),
        "pnpm" => Some(
            cmd_output(
                package_program("pnpm"),
                &["install", "--frozen-lockfile"],
                &dir,
            )
            .await
            .map_err(dependency_failure)?,
        ),
        "pip" => {
            let args: Vec<&str> = if dir.join("requirements.txt").exists() {
                vec!["install", "-r", "requirements.txt"]
            } else {
                vec!["install", "-e", "."]
            };
            Some(
                cmd_output("pip", &args, &dir)
                    .await
                    .map_err(dependency_failure)?,
            )
        }
        "cargo" => Some(
            cmd_output("cargo", &["build", "--locked"], &dir)
                .await
                .map_err(dependency_failure)?,
        ),
        "go" => Some(
            cmd_output("go", &["mod", "download"], &dir)
                .await
                .map_err(dependency_failure)?,
        ),
        _ => None,
    };

    // Restart and persistence are part of the same user-visible operation.
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let before = state
        .manager
        .snapshot_one(id)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.manager.restart(id).await {
        state
            .manager
            .restore_snapshot(before.clone())
            .await
            .map_err(|rollback_error| {
                ApiError::internal(format!(
                    "checkout and dependencies were updated, process restart failed ({error}), and runtime restoration also failed ({rollback_error})"
                ))
            })?;
        return Err(ApiError::internal(format!(
            "checkout and dependencies were updated, but process restart failed ({error}); the previous runtime intent was restored against the updated checkout"
        )));
    }
    if let Err(error) = state.save_to_disk().await {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if state.save_to_disk().await.is_ok() {
            tracing::warn!(%error, "git pull state persistence succeeded on retry");
        } else {
            let message = format!(
            "checkout, dependency update and restart completed, but state persistence failed twice ({error}); the running state was left intact to avoid an unsafe partial rollback. Do not repeat the pull; resolve the storage error and save state."
        );
            *state.background_persistence_error.write().await = Some(message.clone());
            return Err(ApiError::internal(format!(
                "{message}; previous status was {:?} with restart_count {}",
                before.info.status, before.info.restart_count
            )));
        }
    }

    Ok(Json(json!({
        "success": true,
        "pull_output": pull_output,
        "deps_output": deps_output,
        "pkg_manager": pkg_manager,
    })))
}
