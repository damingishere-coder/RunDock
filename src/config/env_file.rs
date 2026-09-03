// @group Configuration > EnvFile : .env file parser and merge logic

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_ENV_FILE_BYTES: u64 = 1024 * 1024;

/// Returns true only for a single, portable env-style filename component.
/// Path separators, parent components, drive prefixes, NTFS alternate streams,
/// control characters and overlong names are rejected before any join occurs.
pub fn is_safe_env_filename(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return false;
    }
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return false;
    }
    let env_style = name == ".env"
        || (name.starts_with(".env.") && name.len() > ".env.".len())
        || (name.ends_with(".env") && name.len() > ".env".len());
    env_style && Path::new(name).components().count() == 1
}

/// Resolve an env filename beneath a canonical process working directory.
/// Existing symlinks are canonicalised and rejected when they escape the root.
pub fn resolve_process_env_path(cwd: &Path, filename: &str) -> Result<PathBuf> {
    if !is_safe_env_filename(filename) {
        anyhow::bail!("invalid env filename");
    }

    let root = std::fs::canonicalize(cwd)
        .with_context(|| format!("failed to resolve process cwd: {}", cwd.display()))?;
    if !root.is_dir() {
        anyhow::bail!("process cwd is not a directory: {}", root.display());
    }

    let candidate = root.join(filename);
    match std::fs::symlink_metadata(&candidate) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                anyhow::bail!("env file symlinks are not allowed");
            }
            let resolved = std::fs::canonicalize(&candidate)
                .with_context(|| format!("failed to resolve env file: {}", candidate.display()))?;
            if !resolved.starts_with(&root) {
                anyhow::bail!("env file resolves outside process cwd");
            }
            Ok(resolved)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(candidate),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect env file: {}", candidate.display())),
    }
}

/// Open an env file without following a final-component symlink/reparse point.
/// This closes the validation-to-open race for secret-bearing env reads.
fn open_env_file_nofollow(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the link itself so it can be rejected below.
        options.custom_flags(0x0020_0000);
    }

    let file = options
        .open(path)
        .with_context(|| format!("failed to open env file: {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect env file: {}", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("env path is not a regular file: {}", path.display());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            anyhow::bail!("env file reparse points are not allowed");
        }
    }
    Ok(file)
}

pub fn read_env_file_text(path: &Path, max_bytes: u64) -> Result<String> {
    let file = open_env_file_nofollow(path)?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect env file: {}", path.display()))?;
    if metadata.len() > max_bytes {
        anyhow::bail!("env file exceeds the {} byte limit", max_bytes);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read env file: {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        anyhow::bail!("env file exceeds the {} byte limit", max_bytes);
    }
    String::from_utf8(bytes)
        .with_context(|| format!("env file is not valid UTF-8: {}", path.display()))
}

// @group Configuration > EnvFile : Parse a .env file without setting process-level env vars
pub fn load_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();
    let content = read_env_file_text(path, MAX_ENV_FILE_BYTES)?;
    let iter = dotenvy::from_read_iter(content.as_bytes());
    for item in iter {
        let (key, value) = item.with_context(|| "failed to parse .env entry")?;
        result.insert(key, value);
    }
    Ok(result)
}

// @group Configuration > EnvFile : Merge .env file values with explicit env vars (explicit wins)
pub fn merge_env(
    env_file: Option<&str>,
    cwd: Option<&str>,
    explicit_env: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let mut merged = HashMap::new();

    // Load .env file if configured
    if let Some(env_path_str) = env_file {
        let cwd = cwd.ok_or_else(|| anyhow::anyhow!("env_file requires a process cwd"))?;
        let resolved = resolve_process_env_path(Path::new(cwd), env_path_str)?;

        if resolved.exists() {
            let env_vars = load_env_file(&resolved)?;
            merged.extend(env_vars);
        } else {
            tracing::warn!("env_file not found: {}", resolved.display());
        }
    }

    // Explicit env vars override .env file values
    for (key, value) in explicit_env {
        merged.insert(key.clone(), value.clone());
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn env_filename_rejects_cross_platform_traversal_and_streams() {
        for invalid in [
            "../outside.env",
            "..\\outside.env",
            "/tmp/outside.env",
            "C:\\outside.env",
            ".env/../secret.env",
            ".env\\..\\secret.env",
            ".env:secret",
            ".env\0secret",
            ".env.",
        ] {
            assert!(!is_safe_env_filename(invalid), "accepted {invalid:?}");
        }
        for valid in [".env", ".env.local", ".env.production", "service.env"] {
            assert!(is_safe_env_filename(valid), "rejected {valid:?}");
        }
    }

    #[test]
    fn env_path_stays_inside_canonical_working_directory() {
        let directory = std::env::temp_dir().join(format!("alter-env-path-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let resolved = resolve_process_env_path(&directory, ".env.local").unwrap();
        assert_eq!(
            resolved.parent().unwrap(),
            std::fs::canonicalize(&directory).unwrap()
        );
        assert!(resolve_process_env_path(&directory, "..\\outside.env").is_err());
        assert!(resolve_process_env_path(&directory, "../outside.env").is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn env_path_rejects_symlinks_including_dangling_links_when_supported() {
        let directory = std::env::temp_dir().join(format!("alter-env-link-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let link = directory.join("linked.env");
        let missing_target = directory.join("missing-target.env");

        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&missing_target, &link).is_ok();
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_file(&missing_target, &link).is_ok();

        if created {
            assert!(resolve_process_env_path(&directory, "linked.env").is_err());
        }
        let _ = std::fs::remove_file(link);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn merge_env_rejects_absolute_and_traversing_env_paths() {
        let explicit = HashMap::new();
        assert!(merge_env(Some("../secret.env"), Some("."), &explicit).is_err());
        assert!(merge_env(Some("C:\\secret.env"), Some("."), &explicit).is_err());
        assert!(merge_env(Some("/tmp/secret.env"), Some("."), &explicit).is_err());
        assert!(merge_env(Some(".env"), None, &explicit).is_err());
    }
}
