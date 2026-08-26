// @group Configuration : Platform-aware path resolution

use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    // A full override is primarily useful for integration tests and portable
    // deployments. It is checked before platform defaults so a test process
    // cannot accidentally read or create the operator's real data files.
    if let Ok(custom) = std::env::var("ALTER_DATA_DIR") {
        return PathBuf::from(custom);
    }

    // ALTER_DATA_DIR_SUFFIX lets alternate builds (e.g. alter-dev) use an isolated data directory.
    #[cfg(target_os = "windows")]
    let default_suffix = "alter-pm2";
    #[cfg(not(target_os = "windows"))]
    let default_suffix = ".alter-pm2";

    let suffix =
        std::env::var("ALTER_DATA_DIR_SUFFIX").unwrap_or_else(|_| default_suffix.to_string());

    #[cfg(target_os = "windows")]
    {
        let base =
            dirs::data_dir().expect("cannot resolve the current user's application data directory");
        base.join(suffix)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let base = dirs::home_dir().expect("cannot resolve the current user's home directory");
        base.join(suffix)
    }
}

pub fn log_dir() -> PathBuf {
    // ALTER_LOG_DIR fully overrides the log directory path.
    if let Ok(custom) = std::env::var("ALTER_LOG_DIR") {
        return PathBuf::from(custom);
    }
    data_dir().join("logs")
}

pub fn state_file() -> PathBuf {
    data_dir().join("state.json")
}

pub fn projects_file() -> PathBuf {
    data_dir().join("projects.json")
}

pub fn pid_file() -> PathBuf {
    data_dir().join("daemon.pid")
}

pub fn daemon_log_file() -> PathBuf {
    data_dir().join(format!(
        "daemon.{}.log",
        chrono::Utc::now().format("%Y-%m-%d")
    ))
}

pub fn scripts_dir() -> PathBuf {
    data_dir().join("scripts")
}

pub fn terminal_history_file() -> PathBuf {
    data_dir().join("terminal-history.json")
}

pub fn process_log_dir(name: &str) -> PathBuf {
    log_dir().join(sanitize_name(name))
}

pub fn sanitize_name(name: &str) -> String {
    let unchanged = !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '-' || character == '_');
    if unchanged {
        return name.to_string();
    }
    let sanitized: String = name
        .chars()
        .take(80)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let prefix = if sanitized.is_empty() {
        "process".to_string()
    } else {
        sanitized
    };
    let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
    format!("{prefix}-{}", &digest[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_log_names_cannot_collide_with_safe_names() {
        assert_eq!(sanitize_name("api_server"), "api_server");
        assert_ne!(sanitize_name("api/server"), sanitize_name("api?server"));
        assert_ne!(sanitize_name("api/server"), sanitize_name("api_server"));
        assert!(!sanitize_name("").is_empty());
    }
}
