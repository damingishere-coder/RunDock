// @group Configuration > Persistence : Crash-safe file replacement helpers

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

pub const MAX_JSON_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
static FILE_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_file_operations() -> std::sync::MutexGuard<'static, ()> {
    FILE_OPERATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let path_metadata = std::fs::symlink_metadata(path)?;
    if !path_metadata.file_type().is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("persistence path is not a regular file: {}", path.display()),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "opened persistence object is not a regular file: {}",
                path.display()
            ),
        ));
    }
    #[cfg(unix)]
    {
        harden_open_unix_permissions(path, &file, &metadata)?;
        validate_unix_owner_and_mode(path, &file.metadata()?)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "persistence reparse points are not allowed: {}",
                    path.display()
                ),
            ));
        }
    }
    if metadata.len() > MAX_JSON_DOCUMENT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "JSON document exceeds the {} byte limit",
                MAX_JSON_DOCUMENT_BYTES
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_JSON_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON_DOCUMENT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "JSON document exceeds the {} byte limit",
                MAX_JSON_DOCUMENT_BYTES
            ),
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn harden_open_unix_permissions(
    path: &Path,
    file: &std::fs::File,
    metadata: &std::fs::Metadata,
) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "persistence file is not owned by the current user: {}",
                path.display()
            ),
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_owner_and_mode(path: &Path, metadata: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "persistence file owner or permissions are unsafe: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Persist bytes through a unique temporary file and an atomic replacement.
///
/// When `backup` is supplied it is committed first, so callers can retain a
/// previously validated last-known-good representation before replacing the
/// primary file. The primary file is read back byte-for-byte before success is
/// reported. If verification fails, the validated backup is restored.
pub fn write_with_backup(
    path: &Path,
    content: &[u8],
    backup: Option<(&Path, &[u8])>,
) -> Result<()> {
    let _file_guard = lock_file_operations();
    write_with_backup_unlocked(path, content, backup)
}

fn write_with_backup_unlocked(
    path: &Path,
    content: &[u8],
    backup: Option<(&Path, &[u8])>,
) -> Result<()> {
    anyhow::ensure!(
        content.len() as u64 <= MAX_JSON_DOCUMENT_BYTES,
        "persistence document exceeds the {} byte limit",
        MAX_JSON_DOCUMENT_BYTES
    );
    if let Some((_, backup_content)) = backup {
        anyhow::ensure!(
            backup_content.len() as u64 <= MAX_JSON_DOCUMENT_BYTES,
            "persistence recovery document exceeds the {} byte limit",
            MAX_JSON_DOCUMENT_BYTES
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create persistence directory: {}",
                parent.display()
            )
        })?;
    }

    if let Some((backup_path, backup_content)) = backup {
        replace_bytes(backup_path, backup_content).with_context(|| {
            format!(
                "failed to update last-known-good file: {}",
                backup_path.display()
            )
        })?;
    }

    replace_bytes(path, content)
        .with_context(|| format!("failed to replace persistence file: {}", path.display()))?;

    match read_bounded(path) {
        Ok(persisted) if persisted == content => Ok(()),
        verification => {
            let verification_message = match &verification {
                Ok(_) => format!("write verification mismatch: {}", path.display()),
                Err(error) => format!(
                    "failed to verify persistence file {}: {error}",
                    path.display()
                ),
            };
            if let Some((backup_path, _)) = backup {
                let recovery = read_bounded(backup_path)
                    .with_context(|| {
                        format!("failed to read recovery copy: {}", backup_path.display())
                    })
                    .and_then(|backup_content| {
                        replace_bytes(path, &backup_content).with_context(|| {
                            format!("failed to restore recovery copy to: {}", path.display())
                        })
                    });
                if let Err(recovery_error) = recovery {
                    anyhow::bail!("{verification_message}; recovery failed: {recovery_error}");
                }
            }
            anyhow::bail!(verification_message)
        }
    }
}

/// After a compensated multi-file transaction, make the recovery copy match
/// the restored primary instead of retaining bytes from the failed commit.
pub fn refresh_backup_from_primary(path: &Path) -> Result<()> {
    refresh_backup_from_primary_validated::<serde_json::Value, _>(path, |_| Ok(()))
}

/// Refresh a recovery copy only after the primary passes both JSON parsing and
/// the caller's type-specific semantic validation.
pub fn refresh_backup_from_primary_validated<T, F>(path: &Path, validate: F) -> Result<()>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let _file_guard = lock_file_operations();
    let content = read_bounded(path)
        .with_context(|| format!("failed to read restored primary: {}", path.display()))?;
    let value = serde_json::from_slice::<T>(&content)
        .with_context(|| format!("restored primary is not valid JSON: {}", path.display()))?;
    validate(&value).with_context(|| {
        format!(
            "restored primary failed semantic validation: {}",
            path.display()
        )
    })?;
    let backup_path = path.with_extension("json.bak");
    write_with_backup_unlocked(&backup_path, &content, None)
}

/// Load a JSON document without treating corruption as a first-run default.
/// A validated backup is accepted; only the absence of both files yields T::default().
pub fn load_json_with_backup<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    load_json_with_backup_validated(path, |_| Ok(()))
}

/// Load the first candidate that passes both deserialization and semantic
/// validation. A parseable-but-invalid primary must not hide a valid LKG copy.
pub fn load_json_with_backup_validated<T, F>(path: &Path, validate: F) -> Result<T>
where
    T: DeserializeOwned + Default,
    F: Fn(&T) -> Result<()>,
{
    let _file_guard = lock_file_operations();
    let backup_path = path.with_extension("json.bak");
    match read_bounded(path) {
        Ok(content) => match parse_and_validate(&content, &validate) {
            Ok(value) => {
                repair_missing_or_invalid_backup::<T, _>(&backup_path, &content, &validate);
                Ok(value)
            }
            Err(primary_error) => {
                load_json_backup_validated(&backup_path, &primary_error, &validate)
            }
        },
        Err(primary_error) if primary_error.kind() == std::io::ErrorKind::NotFound => {
            match read_bounded(&backup_path) {
                Ok(backup) => parse_and_validate(&backup, &validate).with_context(|| {
                    format!(
                        "primary JSON is absent; backup is invalid: {}",
                        backup_path.display()
                    )
                }),
                Err(backup_error) if backup_error.kind() == std::io::ErrorKind::NotFound => {
                    let value = T::default();
                    validate(&value).context("default JSON document failed semantic validation")?;
                    Ok(value)
                }
                Err(backup_error) => Err(backup_error).with_context(|| {
                    format!(
                        "primary JSON is absent; backup is unreadable: {}",
                        backup_path.display()
                    )
                }),
            }
        }
        Err(primary_error) => load_json_backup_validated(&backup_path, &primary_error, &validate),
    }
}

fn repair_missing_or_invalid_backup<T, F>(backup_path: &Path, primary: &[u8], validate: &F)
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let backup_valid = read_bounded(backup_path)
        .ok()
        .and_then(|bytes| parse_and_validate::<T, _>(&bytes, validate).ok())
        .is_some();
    if !backup_valid {
        if let Err(error) = write_with_backup_unlocked(backup_path, primary, None) {
            tracing::warn!(path = %backup_path.display(), %error, "failed to repair last-known-good persistence copy");
        }
    }
}

fn parse_and_validate<T, F>(content: &[u8], validate: &F) -> Result<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let value: T = serde_json::from_slice(content)?;
    validate(&value)?;
    Ok(value)
}

fn load_json_backup_validated<T, F>(
    backup_path: &Path,
    primary_error: &dyn std::fmt::Display,
    validate: &F,
) -> Result<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let backup = read_bounded(backup_path).with_context(|| {
        format!(
            "primary JSON failed ({primary_error}); no readable backup at {}",
            backup_path.display()
        )
    })?;
    parse_and_validate(&backup, validate).with_context(|| {
        format!(
            "primary JSON failed ({primary_error}); backup is invalid: {}",
            backup_path.display()
        )
    })
}

pub fn write_json_with_backup<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize + DeserializeOwned,
{
    write_json_with_backup_validated(path, value, |_| Ok(()))
}

/// Persist a document while allowing only semantically valid candidates to
/// become or replace the last-known-good recovery copy.
pub fn write_json_with_backup_validated<T, F>(path: &Path, value: &T, validate: F) -> Result<()>
where
    T: Serialize + DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let _file_guard = lock_file_operations();
    validate(value).context("refusing to persist a semantically invalid JSON document")?;
    let content = serde_json::to_vec_pretty(value)?;
    if content.len() as u64 > MAX_JSON_DOCUMENT_BYTES {
        anyhow::bail!(
            "JSON document exceeds the {} byte limit",
            MAX_JSON_DOCUMENT_BYTES
        );
    }
    let backup_path = path.with_extension("json.bak");
    let primary_previous = read_validated_candidate::<T, _>(path, &validate);
    let backup_previous = read_validated_candidate::<T, _>(&backup_path, &validate);
    let primary_is_valid = matches!(primary_previous, Ok(Some(_)));
    let recovered_from_backup = !primary_is_valid && matches!(backup_previous, Ok(Some(_)));
    let validated_previous = match (primary_previous, backup_previous) {
        (Ok(Some(primary)), _) => Some(primary),
        (_, Ok(Some(backup))) => Some(backup),
        (Ok(None), Ok(None)) => None,
        (Err(primary_error), Err(backup_error)) => anyhow::bail!(
            "refusing to overwrite invalid primary ({primary_error}) and backup ({backup_error})"
        ),
        (Err(error), Ok(None)) | (Ok(None), Err(error)) => {
            anyhow::bail!("refusing to overwrite an existing invalid persistence file: {error}")
        }
    };
    write_with_backup_unlocked(
        path,
        &content,
        validated_previous
            .as_deref()
            .map(|previous| (backup_path.as_path(), previous)),
    )?;
    if recovered_from_backup {
        write_with_backup_unlocked(&backup_path, &content, None)?;
    }
    Ok(())
}

fn read_validated_candidate<T, F>(path: &Path, validate: &F) -> Result<Option<Vec<u8>>>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    match read_bounded(path) {
        Ok(bytes) => {
            parse_and_validate::<T, _>(&bytes, validate).with_context(|| {
                format!("existing persistence file is invalid: {}", path.display())
            })?;
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!(
                "existing persistence file is unreadable: {}",
                path.display()
            )
        }),
    }
}

fn replace_bytes(path: &Path, content: &[u8]) -> Result<()> {
    let temp_path = unique_temp_path(path)?;
    let write_result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("failed to create temporary file: {}", temp_path.display()))?;
        file.write_all(content)?;
        file.sync_all()?;
        replace_file_atomically(&temp_path, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result
}

fn unique_temp_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("persistence path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("persistence filename is not valid UTF-8"))?;
    Ok(parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4())))
}

#[cfg(target_os = "windows")]
fn replace_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

pub(crate) fn move_file_durably(source: &Path, destination: &Path) -> Result<()> {
    replace_file_atomically(source, destination)
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    std::fs::rename(source, destination)?;
    if let Some(parent) = destination.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestDocument {
        version: u32,
    }

    #[cfg(unix)]
    #[test]
    fn bounded_reader_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = std::env::temp_dir().join(format!("alter-json-fifo-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let fifo = directory.join("state.json");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);

        let error = read_bounded(&fifo).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_overwrites_primary_and_preserves_validated_backup() {
        let directory = std::env::temp_dir().join(format!("alter-atomic-file-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("state.json");
        let backup = directory.join("state.json.bak");
        std::fs::write(&primary, br#"{"version":1}"#).unwrap();

        write_with_backup(
            &primary,
            br#"{"version":2}"#,
            Some((&backup, br#"{"version":1}"#)),
        )
        .unwrap();

        assert_eq!(std::fs::read(&primary).unwrap(), br#"{"version":2}"#);
        assert_eq!(std::fs::read(&backup).unwrap(), br#"{"version":1}"#);
        assert!(std::fs::read_dir(&directory).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_loader_distinguishes_first_run_from_corruption() {
        let directory = std::env::temp_dir().join(format!("alter-json-load-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");

        assert_eq!(
            load_json_with_backup::<TestDocument>(&primary).unwrap(),
            TestDocument::default()
        );
        std::fs::write(&primary, "{broken").unwrap();
        assert!(load_json_with_backup::<TestDocument>(&primary).is_err());
        std::fs::write(primary.with_extension("json.bak"), r#"{"version":1}"#).unwrap();
        assert_eq!(
            load_json_with_backup::<TestDocument>(&primary).unwrap(),
            TestDocument { version: 1 }
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn valid_primary_repairs_a_corrupt_recovery_copy() {
        let directory =
            std::env::temp_dir().join(format!("alter-json-backup-repair-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");
        let backup = primary.with_extension("json.bak");
        std::fs::write(&primary, br#"{"version":7}"#).unwrap();
        std::fs::write(&backup, b"{broken").unwrap();

        assert_eq!(
            load_json_with_backup::<TestDocument>(&primary).unwrap(),
            TestDocument { version: 7 }
        );
        assert_eq!(
            load_json_with_backup::<TestDocument>(&backup).unwrap(),
            TestDocument { version: 7 }
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn semantic_validation_uses_backup_without_preserving_invalid_primary() {
        let directory =
            std::env::temp_dir().join(format!("alter-json-semantic-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");
        let backup = primary.with_extension("json.bak");
        std::fs::write(&primary, br#"{"version":0}"#).unwrap();
        std::fs::write(&backup, br#"{"version":1}"#).unwrap();
        let validate = |document: &TestDocument| {
            anyhow::ensure!(document.version > 0, "version must be positive");
            Ok(())
        };

        assert_eq!(
            load_json_with_backup_validated(&primary, validate).unwrap(),
            TestDocument { version: 1 }
        );
        write_json_with_backup_validated(&primary, &TestDocument { version: 2 }, validate).unwrap();
        let backup_value: TestDocument =
            serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
        assert_eq!(backup_value, TestDocument { version: 2 });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_writer_keeps_previous_valid_document_as_backup() {
        let directory = std::env::temp_dir().join(format!("alter-json-write-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");

        write_json_with_backup(&primary, &TestDocument { version: 1 }).unwrap();
        write_json_with_backup(&primary, &TestDocument { version: 2 }).unwrap();
        assert_eq!(
            load_json_with_backup::<TestDocument>(&primary).unwrap(),
            TestDocument { version: 2 }
        );
        let backup: TestDocument =
            serde_json::from_slice(&std::fs::read(primary.with_extension("json.bak")).unwrap())
                .unwrap();
        assert_eq!(backup, TestDocument { version: 1 });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovered_document_refreshes_backup_after_verified_save() {
        let directory = std::env::temp_dir().join(format!("alter-json-lkg-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");
        let backup = primary.with_extension("json.bak");
        std::fs::write(&primary, b"{broken").unwrap();
        std::fs::write(&backup, br#"{"version":1}"#).unwrap();

        write_json_with_backup(&primary, &TestDocument { version: 2 }).unwrap();
        std::fs::write(&primary, b"{broken-again").unwrap();

        assert_eq!(
            load_json_with_backup::<TestDocument>(&primary).unwrap(),
            TestDocument { version: 2 }
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_writer_refuses_to_hide_corrupt_primary_and_backup() {
        let directory =
            std::env::temp_dir().join(format!("alter-json-corrupt-write-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");
        let backup = primary.with_extension("json.bak");
        std::fs::write(&primary, b"{broken-primary").unwrap();
        std::fs::write(&backup, b"{broken-backup").unwrap();

        let error = write_json_with_backup(&primary, &TestDocument { version: 2 }).unwrap_err();

        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(std::fs::read(&primary).unwrap(), b"{broken-primary");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{broken-backup");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_loader_rejects_oversized_primary_and_backup() {
        let directory = std::env::temp_dir().join(format!("alter-json-limit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("settings.json");
        let backup = primary.with_extension("json.bak");
        std::fs::File::create(&primary)
            .unwrap()
            .set_len(MAX_JSON_DOCUMENT_BYTES + 1)
            .unwrap();
        std::fs::File::create(&backup)
            .unwrap()
            .set_len(MAX_JSON_DOCUMENT_BYTES + 1)
            .unwrap();

        assert!(load_json_with_backup::<TestDocument>(&primary).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn raw_writer_rejects_oversized_content_without_touching_primary() {
        let directory =
            std::env::temp_dir().join(format!("alter-raw-write-limit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("handoff.json");
        std::fs::write(&primary, b"previous").unwrap();
        let oversized = vec![b'x'; (MAX_JSON_DOCUMENT_BYTES + 1) as usize];

        assert!(write_with_backup(&primary, &oversized, None).is_err());
        assert_eq!(std::fs::read(&primary).unwrap(), b"previous");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compensated_transaction_refreshes_recovery_copy() {
        let directory =
            std::env::temp_dir().join(format!("alter-json-compensate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let primary = directory.join("state.json");
        let backup = primary.with_extension("json.bak");
        std::fs::write(&primary, br#"{"version":1}"#).unwrap();
        std::fs::write(&backup, br#"{"version":2}"#).unwrap();

        refresh_backup_from_primary(&primary).unwrap();

        assert_eq!(std::fs::read(&backup).unwrap(), br#"{"version":1}"#);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
