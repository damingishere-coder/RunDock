// @group BusinessLogic : Spawn child process and pipe stdout/stderr to log infrastructure

use crate::models::log_stats::LogStatsState;
use crate::process::instance::{LogLine, LogStream};
use crate::process::tree::ProcessTreeGuard;
use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct ManagedChild {
    child: Child,
    process_tree: Option<ProcessTreeGuard>,
    reader_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl ManagedChild {
    pub fn take_process_tree(&mut self) -> Option<ProcessTreeGuard> {
        self.process_tree.take()
    }
}

const MAX_LOG_LINE_BYTES: usize = 64 * 1024;

async fn forward_bounded_output<R>(
    source: R,
    process_id: Uuid,
    stream: LogStream,
    log_tx: broadcast::Sender<LogLine>,
    log_stats: Arc<Mutex<LogStatsState>>,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(source);
    let mut line = Vec::with_capacity(8 * 1024);
    let mut truncated = false;
    loop {
        let available = match reader.fill_buf().await {
            Ok([]) => {
                if !line.is_empty() || truncated {
                    emit_log_line(
                        process_id,
                        stream.clone(),
                        &mut line,
                        truncated,
                        &log_tx,
                        &log_stats,
                    )
                    .await;
                }
                break;
            }
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, %process_id, "managed process output could not be read");
                break;
            }
        };
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content_len = newline.unwrap_or(consumed);
        let remaining = MAX_LOG_LINE_BYTES.saturating_sub(line.len());
        let copied = content_len.min(remaining);
        line.extend_from_slice(&available[..copied]);
        truncated |= copied < content_len;
        reader.consume(consumed);

        if newline.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            emit_log_line(
                process_id,
                stream.clone(),
                &mut line,
                truncated,
                &log_tx,
                &log_stats,
            )
            .await;
            truncated = false;
        }
    }
}

async fn emit_log_line(
    process_id: Uuid,
    stream: LogStream,
    line: &mut Vec<u8>,
    truncated: bool,
    log_tx: &broadcast::Sender<LogLine>,
    log_stats: &Arc<Mutex<LogStatsState>>,
) {
    let mut content = String::from_utf8_lossy(line).into_owned();
    if truncated {
        content.push_str(" … [line truncated at 64 KiB]");
    }
    line.clear();
    let stdout = stream == LogStream::Stdout;
    let _ = log_tx.send(LogLine {
        timestamp: Utc::now(),
        process_id,
        stream,
        content,
    });
    log_stats.lock().await.record(stdout);
}

impl std::ops::Deref for ManagedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

// @group BusinessLogic > Windows : Process creation flags
// CREATE_NO_WINDOW  — hides the console window for every spawned child.
// CREATE_BREAKAWAY_FROM_JOB — removes the child from the daemon's Windows Job Object so
//   managed processes survive a daemon restart without being killed by the OS.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Result of a child process run: the exit code (or None on signal/kill)
pub struct RunResult {
    pub exit_code: Option<i32>,
}

fn configure_command(
    cmd: &mut Command,
    cwd: Option<&str>,
    env_vars: &HashMap<String, String>,
) -> Result<()> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(false);

    if let Some(dir) = cwd {
        let path = PathBuf::from(dir);
        anyhow::ensure!(path.exists(), "cwd does not exist: {dir}");
        cmd.current_dir(path);
    }
    for (key, value) in env_vars {
        cmd.env(key, value);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_command(script: &str, args: &[String], creation_flags: u32) -> Command {
    let is_native =
        script.to_lowercase().ends_with(".exe") || script.contains('\\') || script.contains('/');
    let mut command = if is_native {
        Command::new(script)
    } else {
        let mut shell = Command::new("cmd");
        shell.arg("/C");
        shell.arg(script);
        shell
    };
    command.args(args);
    command.creation_flags(creation_flags);
    command
}

/// Spawn a child process and begin streaming its output.
/// Returns the Child handle and a receiver for exit notification.
pub async fn spawn_process(
    process_id: Uuid,
    script: &str,
    args: &[String],
    cwd: Option<&str>,
    env_vars: &HashMap<String, String>,
    log_tx: broadcast::Sender<LogLine>,
    log_stats: Arc<Mutex<LogStatsState>>,
) -> Result<ManagedChild> {
    // @group BusinessLogic > Windows : npm/node/python etc. are .cmd batch scripts on Windows.
    // Wrap with cmd.exe /C so the shell resolves them correctly.
    // If the script is already a full path or ends in .exe, spawn directly.
    #[cfg(target_os = "windows")]
    let mut cmd = windows_command(script, args, CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new(script);
        c.args(args);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.as_std_mut().process_group(0);
        }
        c
    };
    configure_command(&mut cmd, cwd, env_vars)?;

    #[cfg(target_os = "windows")]
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(initial_error) if initial_error.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::warn!(
                "CREATE_BREAKAWAY_FROM_JOB was denied for '{script}'; retrying without breakaway"
            );
            let mut fallback = windows_command(script, args, CREATE_NO_WINDOW);
            configure_command(&mut fallback, cwd, env_vars)?;
            fallback.spawn().with_context(|| {
                format!(
                    "failed to spawn without CREATE_BREAKAWAY_FROM_JOB after Windows denied it: {script}"
                )
            })?
        }
        Err(error) => return Err(error).with_context(|| format!("failed to spawn: {script}")),
    };
    #[cfg(not(target_os = "windows"))]
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn: {script}"))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    // @group BusinessLogic > Logging : Stream stdout to broadcast + disk + log stats counter
    let stdout_tx = log_tx.clone();
    let stats_out = Arc::clone(&log_stats);
    let stdout_task = tokio::spawn(forward_bounded_output(
        stdout,
        process_id,
        LogStream::Stdout,
        stdout_tx,
        stats_out,
    ));

    // @group BusinessLogic > Logging : Stream stderr to broadcast + disk + log stats counter
    let stderr_tx = log_tx.clone();
    let stats_err = Arc::clone(&log_stats);
    let stderr_task = tokio::spawn(forward_bounded_output(
        stderr,
        process_id,
        LogStream::Stderr,
        stderr_tx,
        stats_err,
    ));

    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("spawned process did not expose a PID"))?;
    let process_tree = match ProcessTreeGuard::new(pid, &process_id.to_string()) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error).context("failed to establish process-tree ownership");
        }
    };

    Ok(ManagedChild {
        child,
        process_tree: Some(process_tree),
        reader_tasks: vec![stdout_task, stderr_task],
    })
}

/// Wait for the child to exit and send the result through the channel.
pub async fn wait_for_exit(child: ManagedChild, exit_tx: mpsc::Sender<RunResult>) {
    let ManagedChild {
        mut child,
        process_tree,
        reader_tasks,
    } = child;
    let exit_code = match child.wait().await {
        Ok(status) => status.code(),
        Err(_) => None,
    };
    // Ensure descendants are gone before autorestart observes the root exit.
    drop(process_tree);
    for mut task in reader_tasks {
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
    let _ = exit_tx.send(RunResult { exit_code }).await;
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_spawn_retries_when_breakaway_is_denied() {
        let script = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let args = vec!["/C".to_string(), "exit 0".to_string()];
        let (log_tx, _) = broadcast::channel(8);
        let stats = Arc::new(Mutex::new(LogStatsState::new()));

        let mut child = spawn_process(
            Uuid::new_v4(),
            &script,
            &args,
            None,
            &HashMap::new(),
            log_tx,
            stats,
        )
        .await
        .unwrap();
        let status = child.wait().await.unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    async fn dropping_managed_child_terminates_windows_descendants() {
        let directory = std::env::temp_dir().join(format!("alter-process-tree-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let pid_file = directory.join("descendant.pid");
        let pid_path = pid_file.to_string_lossy().replace("'", "''");
        let command = format!(
            "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-Command', 'Start-Sleep -Seconds 30') -PassThru; $child.Id | Set-Content -Encoding ascii '{pid_path}'; Start-Sleep -Seconds 30"
        );
        let args = vec!["-NoProfile".to_string(), "-Command".to_string(), command];
        let (log_tx, _) = broadcast::channel(8);
        let stats = Arc::new(Mutex::new(LogStatsState::new()));
        let mut child = spawn_process(
            Uuid::new_v4(),
            "powershell.exe",
            &args,
            None,
            &HashMap::new(),
            log_tx,
            stats,
        )
        .await
        .unwrap();

        for _ in 0..50 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let descendant_pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("descendant did not publish its PID")
            .trim()
            .parse()
            .unwrap();

        let _ = child.kill().await;
        let _ = child.wait().await;
        drop(child);
        for _ in 0..30 {
            if !crate::process::identity::is_pid_alive(descendant_pid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(!crate::process::identity::is_pid_alive(descendant_pid));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn named_job_handoff_preserves_process_until_final_owner_closes() {
        let process_id = Uuid::new_v4();
        let args = vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 30".to_string(),
        ];
        let (log_tx, _) = broadcast::channel(8);
        let stats = Arc::new(Mutex::new(LogStatsState::new()));
        let child = spawn_process(
            process_id,
            "powershell.exe",
            &args,
            None,
            &HashMap::new(),
            log_tx,
            stats,
        )
        .await
        .unwrap();
        let pid = child.id().unwrap();
        let replacement_owner = ProcessTreeGuard::new(pid, &process_id.to_string()).unwrap();

        drop(child);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(crate::process::identity::is_pid_alive(pid));

        drop(replacement_owner);
        for _ in 0..30 {
            if !crate::process::identity::is_pid_alive(pid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(!crate::process::identity::is_pid_alive(pid));
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn oversized_output_line_is_truncated_before_broadcast() {
        let (mut writer, reader) = tokio::io::duplex(8 * 1024);
        let (log_tx, mut log_rx) = broadcast::channel(8);
        let stats = Arc::new(Mutex::new(LogStatsState::new()));
        let payload = vec![b'x'; MAX_LOG_LINE_BYTES * 2];
        let write_task = tokio::spawn(async move {
            writer.write_all(&payload).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        });

        forward_bounded_output(reader, Uuid::new_v4(), LogStream::Stdout, log_tx, stats).await;
        write_task.await.unwrap();
        let line = log_rx.recv().await.unwrap();
        assert!(line.content.contains("line truncated at 64 KiB"));
        assert!(line.content.len() < MAX_LOG_LINE_BYTES + 64);
    }
}
