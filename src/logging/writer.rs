// @group BusinessLogic : Rolling file writer — persists log lines to disk and subscribes to broadcast
// @group BusinessLogic > DailyRotation : Midnight timer that rotates logs by date while processes run

use crate::logging::rotation::{rotate_by_date, rotate_if_needed, seconds_until_midnight};
use crate::process::instance::{LogLine, LogStream};
use anyhow::Result;
use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

const MAX_LOG_FILES: usize = 5;
const DAILY_KEEP_DAYS: u32 = 30;

// @group BusinessLogic : File handle with size tracking and re-open after rotation

struct FileHandle {
    path: PathBuf,
    file: File,
    bytes_written: u64,
    max_size_bytes: u64,
}

impl FileHandle {
    fn open(path: &Path, max_size_bytes: u64) -> Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes_written = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            bytes_written,
            max_size_bytes,
        })
    }

    fn write_line(&mut self, line: &str) -> Result<()> {
        // Check size-based rotation before writing
        if self.bytes_written >= self.max_size_bytes
            && rotate_if_needed(&self.path, self.max_size_bytes, MAX_LOG_FILES)?
        {
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.bytes_written = 0;
        }
        writeln!(self.file, "{}", line)?;
        self.bytes_written += (line.len() + 1) as u64;
        Ok(())
    }

    /// Re-open the file (called after a daily rotation renames the current file away).
    fn reopen(&mut self) -> Result<()> {
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.bytes_written = self.file.metadata()?.len();
        Ok(())
    }

    fn truncate(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.set_len(0)?;
        self.file.sync_data()?;
        self.bytes_written = 0;
        Ok(())
    }
}

/// Subscribes to the process's broadcast channel and writes every log line to disk.
/// Also spawns a background task that rotates logs at local midnight every day.
pub struct LogWriter {
    out_handle: Arc<Mutex<FileHandle>>,
    err_handle: Arc<Mutex<FileHandle>>,
    log_dir: PathBuf,
    write_handle: tokio::task::JoinHandle<()>,
    rotate_handle: tokio::task::JoinHandle<()>,
}

impl LogWriter {
    pub fn new(
        log_dir: &Path,
        log_tx: broadcast::Sender<LogLine>,
        max_log_size_mb: u64,
    ) -> Result<Self> {
        let max_size_bytes = max_log_size_mb.clamp(1, 1024).saturating_mul(1024 * 1024);
        let out_path = log_dir.join("out.log");
        let err_path = log_dir.join("err.log");

        let out_handle = Arc::new(Mutex::new(FileHandle::open(&out_path, max_size_bytes)?));
        let err_handle = Arc::new(Mutex::new(FileHandle::open(&err_path, max_size_bytes)?));

        // @group BusinessLogic : Log-line writer task
        let out_clone = Arc::clone(&out_handle);
        let err_clone = Arc::clone(&err_handle);
        let mut rx = log_tx.subscribe();

        let write_handle = tokio::spawn(async move {
            loop {
                let line = match rx.recv().await {
                    Ok(line) => line,
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "process log writer lagged; continuing from newest buffered line"
                        );
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let formatted = format!(
                    "[{}] {}",
                    line.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                    line.content
                );
                let target = match line.stream {
                    LogStream::Stdout => Arc::clone(&out_clone),
                    LogStream::Stderr => Arc::clone(&err_clone),
                };
                let write_result = tokio::task::spawn_blocking(move || match target.lock() {
                    Ok(mut file) => {
                        let path = file.path.clone();
                        file.write_line(&formatted).map_err(|error| (path, error))
                    }
                    Err(error) => Err((PathBuf::new(), anyhow::anyhow!(error.to_string()))),
                })
                .await;
                match write_result {
                    Ok(Ok(())) => {}
                    Ok(Err((path, error))) => {
                        if path.as_os_str().is_empty() {
                            tracing::error!(%error, "process log writer lock is poisoned");
                        } else {
                            tracing::error!(path = %path.display(), %error, "failed to persist process log line");
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "process log writer blocking task failed");
                    }
                }
            }
        });

        // @group BusinessLogic > DailyRotation : Midnight rotation task — runs independently of process state
        let out_rot = Arc::clone(&out_handle);
        let err_rot = Arc::clone(&err_handle);

        let rotate_handle = tokio::spawn(async move {
            loop {
                // Sleep until the next local midnight
                let secs = seconds_until_midnight();
                sleep(Duration::from_secs(secs + 1)).await; // +1 to land just past midnight

                let yesterday = (Local::now() - chrono::Duration::days(1)).date_naive();

                let out_for_rotation = Arc::clone(&out_rot);
                let err_for_rotation = Arc::clone(&err_rot);
                let rotation = tokio::task::spawn_blocking(move || {
                    // Rotate stdout log
                    if let Ok(mut file) = out_for_rotation.lock() {
                        if let Err(error) = rotate_by_date(&file.path, yesterday, DAILY_KEEP_DAYS) {
                            tracing::error!(path = %file.path.display(), %error, "failed to rotate stdout log");
                        }
                        if let Err(error) = file.reopen() {
                            tracing::error!(path = %file.path.display(), %error, "failed to reopen stdout log");
                        }
                    } else {
                        tracing::error!("stdout log rotation lock is poisoned");
                    }
                    // Rotate stderr log
                    if let Ok(mut file) = err_for_rotation.lock() {
                        if let Err(error) = rotate_by_date(&file.path, yesterday, DAILY_KEEP_DAYS) {
                            tracing::error!(path = %file.path.display(), %error, "failed to rotate stderr log");
                        }
                        if let Err(error) = file.reopen() {
                            tracing::error!(path = %file.path.display(), %error, "failed to reopen stderr log");
                        }
                    } else {
                        tracing::error!("stderr log rotation lock is poisoned");
                    }
                })
                .await;
                if let Err(error) = rotation {
                    tracing::error!(%error, "daily log rotation task failed");
                }
            }
        });

        Ok(Self {
            out_handle,
            err_handle,
            log_dir: log_dir.to_path_buf(),
            write_handle,
            rotate_handle,
        })
    }

    /// Clear active logs while keeping the writer's open handles valid, then
    /// remove every size/date rotation for the same two streams.
    pub fn clear(&self) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        let out_handle = Arc::clone(&self.out_handle);
        let err_handle = Arc::clone(&self.err_handle);
        let log_dir = self.log_dir.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                out_handle
                    .lock()
                    .map_err(|error| anyhow::anyhow!("stdout log lock is poisoned: {error}"))?
                    .truncate()?;
                err_handle
                    .lock()
                    .map_err(|error| anyhow::anyhow!("stderr log lock is poisoned: {error}"))?
                    .truncate()?;
                remove_log_files(&log_dir, false)
            })
            .await
            .map_err(|error| anyhow::anyhow!("log clear task failed: {error}"))?
        }
    }

    /// Clear logs for a process without an active writer.
    pub async fn clear_inactive(log_dir: PathBuf) -> Result<()> {
        tokio::task::spawn_blocking(move || remove_log_files(&log_dir, true))
            .await
            .map_err(|error| anyhow::anyhow!("inactive log clear task failed: {error}"))?
    }
}

fn remove_log_files(log_dir: &Path, include_current: bool) -> Result<()> {
    if !log_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_current = matches!(name.as_ref(), "out.log" | "err.log");
        let is_rotation = name.starts_with("out.log.") || name.starts_with("err.log.");
        if is_rotation || (include_current && is_current) {
            let file_type = entry.file_type()?;
            if file_type.is_file() || file_type.is_symlink() {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        self.write_handle.abort();
        self.rotate_handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inactive_clear_removes_current_and_rotated_logs_only() {
        let temp =
            std::env::temp_dir().join(format!("alter-log-clear-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temp).unwrap();
        for name in ["out.log", "err.log", "out.log.1", "err.log.2026-08-25"] {
            std::fs::write(temp.join(name), b"content").unwrap();
        }
        std::fs::write(temp.join("keep.txt"), b"keep").unwrap();

        LogWriter::clear_inactive(temp.clone()).await.unwrap();

        assert!(temp.join("keep.txt").exists());
        assert!(!temp.join("out.log").exists());
        assert!(!temp.join("err.log.2026-08-25").exists());
        std::fs::remove_dir_all(&temp).unwrap();
    }
}
