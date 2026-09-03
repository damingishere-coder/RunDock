// @group Logging : Size-bounded daemon tracing writer

use std::io::{self, Write};

#[derive(Clone)]
pub struct DaemonLogWriter {
    max_size_bytes: u64,
    max_files: usize,
    state: std::sync::Arc<std::sync::Mutex<Option<chrono::NaiveDate>>>,
}

impl DaemonLogWriter {
    pub fn new(max_size_mb: u64, max_files: usize) -> Self {
        Self {
            max_size_bytes: max_size_mb.clamp(1, 1024).saturating_mul(1024 * 1024),
            max_files: max_files.clamp(1, 100),
            state: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl Write for DaemonLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut last_purge = self
            .state
            .lock()
            .map_err(|_| io::Error::other("daemon log lock is poisoned"))?;
        let path = crate::config::paths::daemon_log_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            let today = chrono::Local::now().date_naive();
            if *last_purge != Some(today) {
                purge_old_daemon_logs(parent, today, 14)?;
                *last_purge = Some(today);
            }
        }
        let current_size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if current_size > 0 && current_size.saturating_add(bytes.len() as u64) > self.max_size_bytes
        {
            crate::logging::rotation::rotate_if_needed(&path, current_size, self.max_files)
                .map_err(io::Error::other)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        // A single pathological event cannot exceed the configured file cap.
        // Return the full consumed length so the tracing worker does not retry
        // an intentionally truncated suffix indefinitely.
        let allowed = usize::try_from(self.max_size_bytes)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        file.write_all(&bytes[..allowed])?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn purge_old_daemon_logs(
    directory: &std::path::Path,
    today: chrono::NaiveDate,
    keep_days: i64,
) -> io::Result<()> {
    let cutoff = today - chrono::Duration::days(keep_days);
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(suffix) = name.strip_prefix("daemon.") else {
            continue;
        };
        let Some(date_text) = suffix.get(..10) else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(date_text, "%Y-%m-%d").is_ok_and(|date| date < cutoff)
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_enforces_nonzero_bounded_rotation_settings() {
        let writer = DaemonLogWriter::new(0, 0);
        assert_eq!(writer.max_size_bytes, 1024 * 1024);
        assert_eq!(writer.max_files, 1);
    }

    #[test]
    fn purge_removes_only_expired_daemon_logs() {
        let directory =
            std::env::temp_dir().join(format!("alter-daemon-log-purge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("daemon.2024-01-01.log"), b"old").unwrap();
        std::fs::write(directory.join("daemon.2024-01-01.log.1"), b"old rotated").unwrap();
        std::fs::write(directory.join("daemon.2026-08-26.log"), b"current").unwrap();
        std::fs::write(directory.join("unrelated.log"), b"keep").unwrap();

        purge_old_daemon_logs(
            &directory,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
            14,
        )
        .unwrap();

        assert!(!directory.join("daemon.2024-01-01.log").exists());
        assert!(!directory.join("daemon.2024-01-01.log.1").exists());
        assert!(directory.join("daemon.2026-08-26.log").exists());
        assert!(directory.join("unrelated.log").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
