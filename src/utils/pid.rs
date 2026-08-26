// @group Utilities : PID file management — prevents duplicate daemon instances

use crate::process::instance::ProcessIdentity;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Serialize, Deserialize)]
struct DaemonPidRecord {
    pid: u32,
    start_time_secs: u64,
    executable: String,
    #[serde(default)]
    owner_token: String,
}

pub fn write_pid_file() -> Result<String> {
    let path = crate::config::paths::pid_file();
    let pid = std::process::id();
    let identity = crate::process::identity::capture_process_identity(pid)
        .ok_or_else(|| anyhow!("could not capture daemon process identity"))?;
    let executable = identity
        .executable
        .ok_or_else(|| anyhow!("daemon executable identity is unavailable"))?;
    let owner_token = uuid::Uuid::new_v4().to_string();
    let record = DaemonPidRecord {
        pid,
        start_time_secs: identity.start_time_secs,
        executable: canonical_path_string(std::path::Path::new(&executable)),
        owner_token: owner_token.clone(),
    };
    let record_bytes = serde_json::to_vec(&record)?;
    for _ in 0..3 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(&record_bytes)?;
                file.sync_all()?;
                return Ok(owner_token);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if is_daemon_running_result()? {
                    return Err(anyhow!(
                        "another Alter daemon owns PID file {}",
                        path.display()
                    ));
                }
                let before = std::fs::read_to_string(&path).with_context(|| {
                    format!("failed to inspect existing PID file {}", path.display())
                })?;
                std::thread::sleep(std::time::Duration::from_millis(30));
                let after = std::fs::read_to_string(&path).with_context(|| {
                    format!("failed to re-check existing PID file {}", path.display())
                })?;
                if before != after || is_daemon_running_result()? {
                    continue;
                }
                remove_unchanged_pid_file(&path, &before)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(anyhow!(
        "could not acquire exclusive PID file {}",
        path.display()
    ))
}

pub fn remove_pid_file(expected_pid: u32, expected_owner_token: &str) -> Result<bool> {
    let path = crate::config::paths::pid_file();
    let before = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read daemon PID file {}", path.display()))
        }
    };
    let record: DaemonPidRecord = serde_json::from_str(before.trim())
        .with_context(|| format!("invalid daemon PID file {}", path.display()))?;
    if record.pid != expected_pid || record.owner_token != expected_owner_token {
        anyhow::bail!(
            "refusing to remove PID file not owned by this daemon instance (expected PID {expected_pid})"
        );
    }
    remove_unchanged_pid_file(&path, &before)?;
    Ok(true)
}

fn remove_unchanged_pid_file(path: &std::path::Path, expected_content: &str) -> Result<()> {
    let quarantine = path.with_extension(format!("pid-release-{}", uuid::Uuid::new_v4()));
    std::fs::rename(path, &quarantine).with_context(|| {
        format!(
            "failed to atomically claim PID file {} for release",
            path.display()
        )
    })?;
    let moved_content = std::fs::read_to_string(&quarantine)
        .with_context(|| format!("failed to verify claimed PID file {}", quarantine.display()))?;
    if moved_content != expected_content {
        if !path.exists() {
            let _ = std::fs::rename(&quarantine, path);
        }
        anyhow::bail!(
            "PID file changed before release; the claimed file was preserved at {}",
            quarantine.display()
        );
    }
    std::fs::remove_file(&quarantine)
        .with_context(|| format!("failed to release PID file {}", quarantine.display()))
}

pub fn read_pid() -> Option<u32> {
    read_pid_result().ok().flatten()
}

pub fn read_pid_result() -> Result<Option<u32>> {
    Ok(read_pid_record_result()?.map(|record| record.pid))
}

pub(crate) fn read_pid_owner_result() -> Result<Option<(u32, String)>> {
    Ok(read_pid_record_result()?.map(|record| (record.pid, record.owner_token)))
}

fn read_pid_record_result() -> Result<Option<DaemonPidRecord>> {
    let path = crate::config::paths::pid_file();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let content = content.trim();
            if content.starts_with('{') {
                serde_json::from_str(content)
                    .map(Some)
                    .with_context(|| format!("invalid daemon PID file {}", path.display()))
            } else {
                let pid = content
                    .parse()
                    .with_context(|| format!("invalid daemon PID file {}", path.display()))?;
                Ok(Some(DaemonPidRecord {
                    pid,
                    start_time_secs: 0,
                    executable: String::new(),
                    owner_token: String::new(),
                }))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read daemon PID file {}", path.display()))
        }
    }
}

pub fn is_daemon_running() -> bool {
    match is_daemon_running_result() {
        Ok(running) => running,
        Err(error) => {
            tracing::warn!(%error, "could not determine daemon PID ownership");
            false
        }
    }
}

fn is_daemon_running_result() -> Result<bool> {
    Ok(match read_pid_record_result()? {
        Some(record) => daemon_identity_matches(&record),
        None => false,
    })
}

fn daemon_identity_matches(expected: &DaemonPidRecord) -> bool {
    crate::process::identity::capture_process_identity(expected.pid)
        .is_some_and(|identity| daemon_record_matches_identity(expected, &identity))
}

fn daemon_record_matches_identity(expected: &DaemonPidRecord, identity: &ProcessIdentity) -> bool {
    // Command-line metadata is not a stable ownership signal and may be empty on
    // Windows GNU builds. The PID record already pins the immutable process start
    // time and canonical executable path, which together reject PID reuse.
    if expected.start_time_secs != 0 && identity.start_time_secs != expected.start_time_secs {
        return false;
    }
    let Some(executable) = identity.executable.as_deref() else {
        return false;
    };
    let actual = canonical_path_string(std::path::Path::new(executable));
    let expected_executable = if expected.executable.is_empty() {
        std::env::current_exe()
            .map(|path| canonical_path_string(&path))
            .unwrap_or_default()
    } else {
        expected.executable.clone()
    };
    path_strings_equal(&actual, &expected_executable)
}

fn canonical_path_string(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
fn path_strings_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
fn path_strings_equal(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matching_record_and_identity() -> (DaemonPidRecord, ProcessIdentity) {
        let executable = canonical_path_string(&std::env::current_exe().unwrap());
        (
            DaemonPidRecord {
                pid: std::process::id(),
                start_time_secs: 42,
                executable: executable.clone(),
                owner_token: "owner".to_string(),
            },
            ProcessIdentity {
                executable: Some(executable),
                command_line: Vec::new(),
                cwd: None,
                start_time_secs: 42,
            },
        )
    }

    #[test]
    fn daemon_identity_does_not_require_command_line_metadata() {
        let (record, identity) = matching_record_and_identity();
        assert!(daemon_record_matches_identity(&record, &identity));
    }

    #[test]
    fn daemon_identity_rejects_pid_reuse_and_executable_changes() {
        let (record, mut identity) = matching_record_and_identity();
        identity.start_time_secs += 1;
        assert!(!daemon_record_matches_identity(&record, &identity));

        identity.start_time_secs = record.start_time_secs;
        identity.executable = Some("different-alter.exe".to_string());
        assert!(!daemon_record_matches_identity(&record, &identity));
    }
}
