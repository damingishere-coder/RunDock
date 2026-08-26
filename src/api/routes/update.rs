// @group APIEndpoints : Verified self-update check and apply endpoints

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const RELEASE_OWNER: &str = "damingishere-coder";
const RELEASE_REPO: &str = "RunDock";
const MAX_UPDATE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RELEASE_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_RELEASE_NOTES_BYTES: usize = 64 * 1024;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/check", get(check_update))
        .route("/apply", post(apply_update))
        .with_state(state)
}

#[derive(Debug, Clone)]
struct UpdateCandidate {
    version: String,
    asset_name: String,
    download_url: String,
    sha256: String,
    release_notes: Option<String>,
    published_at: Option<String>,
}

fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let core = version.strip_prefix('v').unwrap_or(version);
    let core = core.split_once('-').map(|(value, _)| value).unwrap_or(core);
    let mut parts = core.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

fn semver_gt(current: &str, candidate: &str) -> bool {
    matches!(
        (parse_semver(current), parse_semver(candidate)),
        (Some(current), Some(candidate)) if candidate > current
    )
}

/// Automatic installation is supported only for package formats the daemon
/// can hand to an OS installer without unpacking or executing a shell string.
fn platform_asset_name(version: &str) -> Option<String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some(format!("RunDock-{version}-windows-x64-setup.exe")),
        _ => None,
    }
}

fn trusted_release_url(url: &str, tag: &str, asset_name: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }
    parsed.path() == format!("/{RELEASE_OWNER}/{RELEASE_REPO}/releases/download/{tag}/{asset_name}")
}

fn valid_sha256(value: &str) -> Option<String> {
    let value = value.strip_prefix("sha256:").unwrap_or(value).trim();
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn release_sha256(release: &Value, asset: &Value) -> Option<String> {
    if let Some(digest) = asset["digest"].as_str().and_then(valid_sha256) {
        return Some(digest);
    }

    // Older GitHub releases in this repository put the Windows installer hash
    // in the trusted release body. New releases expose the asset digest.
    let body = release["body"].as_str()?;
    body.lines().find_map(|line| {
        let (_, candidate) = line.split_once("SHA256:")?;
        valid_sha256(candidate)
    })
}

fn update_client(timeout: std::time::Duration) -> anyhow::Result<reqwest::Client> {
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many update redirects");
        }
        let trusted = matches!(
            attempt.url().host_str(),
            Some(
                "github.com"
                    | "objects.githubusercontent.com"
                    | "release-assets.githubusercontent.com"
            )
        ) && attempt.url().scheme() == "https";
        if trusted {
            attempt.follow()
        } else {
            attempt.error("update redirect left the GitHub asset boundary")
        }
    });
    Ok(reqwest::Client::builder()
        .user_agent("RunDock/1.1")
        .timeout(timeout)
        .redirect(redirect)
        .build()?)
}

async fn latest_release() -> anyhow::Result<Value> {
    let client = update_client(std::time::Duration::from_secs(10))?;
    let response = client
        .get(format!(
            "https://api.github.com/repos/{RELEASE_OWNER}/{RELEASE_REPO}/releases/latest"
        ))
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("GitHub release API returned HTTP {}", response.status());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_METADATA_BYTES as u64)
    {
        anyhow::bail!("GitHub release metadata exceeds the response limit");
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > MAX_RELEASE_METADATA_BYTES {
            anyhow::bail!("GitHub release metadata exceeds the response limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&body)?)
}

fn bounded_release_notes(release: &Value) -> Option<String> {
    let notes = release["body"].as_str()?;
    if notes.len() <= MAX_RELEASE_NOTES_BYTES {
        return Some(notes.to_string());
    }
    let mut end = MAX_RELEASE_NOTES_BYTES;
    while !notes.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}\n\n[release notes truncated]", &notes[..end]))
}

fn candidate_from_release(release: &Value) -> anyhow::Result<Option<UpdateCandidate>> {
    let current = env!("CARGO_PKG_VERSION");
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("release is missing tag_name"))?;
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| anyhow::anyhow!("release tag must start with v"))?;
    if parse_semver(version).is_none() {
        anyhow::bail!("release tag is not a supported semantic version");
    }
    if !semver_gt(current, version) {
        return Ok(None);
    }

    let asset_name = platform_asset_name(version)
        .ok_or_else(|| anyhow::anyhow!("automatic update is unsupported on this platform"))?;
    let asset = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset["name"].as_str() == Some(asset_name.as_str()))
        })
        .ok_or_else(|| anyhow::anyhow!("release is missing the expected platform asset"))?;
    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("release asset is missing browser_download_url"))?;
    if !trusted_release_url(download_url, tag, &asset_name) {
        anyhow::bail!("release asset URL is outside the trusted repository boundary");
    }
    let sha256 = release_sha256(release, asset)
        .ok_or_else(|| anyhow::anyhow!("release asset has no trusted SHA-256 digest"))?;

    Ok(Some(UpdateCandidate {
        version: version.to_string(),
        asset_name,
        download_url: download_url.to_string(),
        sha256,
        release_notes: bounded_release_notes(release),
        published_at: release["published_at"].as_str().map(ToOwned::to_owned),
    }))
}

async fn check_update(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);
    let mut cache = state.update_check_cache.lock().await;
    if let Some((checked_at, value)) = cache.as_ref() {
        if checked_at.elapsed() < CACHE_TTL {
            return Json(value.clone());
        }
    }

    let value = check_update_uncached().await;
    *cache = Some((std::time::Instant::now(), value.clone()));
    Json(value)
}

async fn check_update_uncached() -> Value {
    let current = env!("CARGO_PKG_VERSION");
    let release = match latest_release().await {
        Ok(release) => release,
        Err(error) => {
            return json!({
                "current": current,
                "latest": current,
                "up_to_date": false,
                "download_url": null,
                "asset_name": null,
                "sha256": null,
                "integrity_verified": false,
                "is_installer": true,
                "release_notes": null,
                "published_at": null,
                "error": format!("update check failed: {error}"),
            });
        }
    };

    match candidate_from_release(&release) {
        Ok(Some(candidate)) => json!({
            "current": current,
            "latest": candidate.version,
            "up_to_date": false,
            "download_url": candidate.download_url,
            "asset_name": candidate.asset_name,
            "sha256": candidate.sha256,
            "integrity_verified": true,
            "is_installer": true,
            "release_notes": candidate.release_notes,
            "published_at": candidate.published_at,
        }),
        Ok(None) => json!({
            "current": current,
            "latest": release["tag_name"].as_str().unwrap_or(current).trim_start_matches('v'),
            "up_to_date": true,
            "download_url": null,
            "asset_name": null,
            "sha256": null,
            "integrity_verified": false,
            "is_installer": true,
            "release_notes": bounded_release_notes(&release),
            "published_at": release["published_at"].as_str(),
        }),
        Err(error) => json!({
            "current": current,
            "latest": release["tag_name"].as_str().unwrap_or(current).trim_start_matches('v'),
            "up_to_date": false,
            "download_url": null,
            "asset_name": null,
            "sha256": null,
            "integrity_verified": false,
            "is_installer": true,
            "release_notes": bounded_release_notes(&release),
            "published_at": release["published_at"].as_str(),
            "error": format!("automatic update disabled: {error}"),
        }),
    }
}

/// Re-fetches release metadata server-side and ignores client-supplied URLs or
/// hashes. This closes the trust gap between the check and apply endpoints.
async fn apply_update(
    State(state): State<Arc<DaemonState>>,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let _update_guard = state
        .update_lock
        .try_lock()
        .map_err(|_| ApiError::conflict("another update is already in progress"))?;
    let release = latest_release()
        .await
        .map_err(|error| ApiError::internal(format!("update metadata failed: {error}")))?;
    let candidate = candidate_from_release(&release)
        .map_err(|error| ApiError::bad_request(format!("update refused: {error}")))?
        .ok_or_else(|| ApiError::bad_request("no newer verified release is available"))?;

    let current_exe = std::env::current_exe()
        .map_err(|error| ApiError::internal(format!("cannot determine exe path: {error}")))?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| ApiError::internal("exe has no parent directory"))?;
    let extension = if cfg!(windows) { "exe" } else { "deb" };
    let temp_path = exe_dir.join(format!(".alter-update-{}.{}", Uuid::new_v4(), extension));

    if let Err(error) = download_verified(&candidate, &temp_path).await {
        let _ = std::fs::remove_file(&temp_path);
        return Err(ApiError::internal(format!(
            "verified download failed: {error}"
        )));
    }

    let save_result = {
        // Freeze runtime mutations while taking the pre-install snapshot so a
        // transient Starting/Stopping entry cannot become durable state.
        let _mutation_guard = state.state_mutation_lock.lock().await;
        state.save_to_disk().await
    };
    if let Err(error) = save_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(ApiError::internal(format!(
            "state save failed; update not launched: {error}"
        )));
    }

    let install_path = temp_path.clone();
    let target_exe = current_exe.clone();
    let expected_version = candidate.version.clone();
    let install_task = tokio::task::spawn_blocking(move || {
        install_and_verify(&install_path, &target_exe, &expected_version)
    })
    .await;
    let _ = std::fs::remove_file(&temp_path);
    let install_result = install_task
        .map_err(|error| ApiError::internal(format!("installer task failed: {error}")))?;
    install_result.map_err(|error| ApiError::internal(format!("update failed: {error}")))?;

    Ok(Json(json!({
        "success": true,
        "message": "installer completed and the on-disk version was verified",
        "version": candidate.version,
        "asset_name": candidate.asset_name,
    })))
}

async fn download_verified(candidate: &UpdateCandidate, destination: &Path) -> anyhow::Result<()> {
    if !trusted_release_url(
        &candidate.download_url,
        &format!("v{}", candidate.version),
        &candidate.asset_name,
    ) {
        anyhow::bail!("download URL failed repository validation");
    }
    let expected = valid_sha256(&candidate.sha256)
        .ok_or_else(|| anyhow::anyhow!("expected SHA-256 is invalid"))?;
    let client = update_client(std::time::Duration::from_secs(120))?;
    let response = client.get(&candidate.download_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_UPDATE_BYTES)
    {
        anyhow::bail!("update asset exceeds {} bytes", MAX_UPDATE_BYTES);
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .await?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("update size overflow"))?;
        if total > MAX_UPDATE_BYTES {
            anyhow::bail!("update asset exceeds {} bytes", MAX_UPDATE_BYTES);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        anyhow::bail!("SHA-256 mismatch");
    }
    Ok(())
}

#[cfg(windows)]
fn install_and_verify(path: &Path, current_exe: &Path, expected: &str) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    verify_authenticode(path)?;
    let backup = current_exe.with_file_name(format!(".alter-update-backup-{}.exe", Uuid::new_v4()));
    std::fs::copy(current_exe, &backup)?;
    std::fs::File::open(&backup)?.sync_all()?;

    let mut command = std::process::Command::new(path);
    command.arg("/S").creation_flags(0x0900_0000);
    let mut installer = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            command.creation_flags(0x0800_0000);
            command.spawn()?
        }
        Err(error) => return Err(error.into()),
    };
    let installer_pid = installer.id();
    let installer_tree = match crate::process::tree::ProcessTreeGuard::new(
        installer_pid,
        &format!("update-installer-{}", Uuid::new_v4()),
    ) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = installer.kill();
            let _ = installer.wait();
            restore_update_backup(current_exe, &backup)?;
            anyhow::bail!("could not establish installer process-tree ownership: {error}");
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
    let status = loop {
        if let Some(status) = installer.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = installer.kill();
            let _ = installer.wait();
            drop(installer_tree);
            restore_update_backup(current_exe, &backup)?;
            anyhow::bail!("installer timed out after 15 minutes");
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    if !status.success() {
        drop(installer_tree);
        restore_update_backup(current_exe, &backup)?;
        anyhow::bail!("installer exited with status {status}");
    }
    // A successful installer must not leave helpers running while version and
    // signature verification begins. Closing the owned job terminates any
    // lingering descendants before the update transaction is committed.
    drop(installer_tree);

    let (version_status, reported) = match probe_installed_version(current_exe) {
        Ok(result) => result,
        Err(error) => {
            restore_update_backup(current_exe, &backup)?;
            anyhow::bail!("installed executable version probe failed: {error}");
        }
    };
    if !version_status.success() || !reported.split_whitespace().any(|part| part == expected) {
        restore_update_backup(current_exe, &backup)?;
        anyhow::bail!("installed executable did not report expected version {expected}");
    }
    if let Err(error) = verify_authenticode(current_exe) {
        restore_update_backup(current_exe, &backup)?;
        anyhow::bail!("installed executable signature verification failed: {error}");
    }
    if let Err(error) = std::fs::remove_file(&backup) {
        tracing::warn!(path = %backup.display(), %error, "update succeeded but backup cleanup failed");
    }
    Ok(())
}

#[cfg(windows)]
fn probe_installed_version(
    executable: &Path,
) -> anyhow::Result<(std::process::ExitStatus, String)> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    const MAX_VERSION_OUTPUT_BYTES: u64 = 8 * 1024;
    const VERSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    let output_path = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("installed executable has no parent directory"))?
        .join(format!(".alter-version-probe-{}.txt", Uuid::new_v4()));
    let output_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&output_path)?;

    // A file avoids waiting for EOF on a pipe that a descendant could inherit.
    // We can read a bounded snapshot as soon as the direct probe process exits.
    let result = (|| -> anyhow::Result<(std::process::ExitStatus, String)> {
        let mut child = std::process::Command::new(executable)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output_file))
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000 | 0x0100_0000)
            .spawn()?;
        let probe_tree =
            match crate::process::tree::ProcessTreeGuard::new(child.id(), "update-version-probe") {
                Ok(tree) => tree,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("failed to contain version probe process tree: {error}");
                }
            };
        let deadline = std::time::Instant::now() + VERSION_PROBE_TIMEOUT;
        let status = loop {
            if std::fs::metadata(&output_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                > MAX_VERSION_OUTPUT_BYTES
            {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("version probe output exceeded {MAX_VERSION_OUTPUT_BYTES} bytes");
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("version probe timed out after 10 seconds");
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.into());
                }
            }
        };
        drop(probe_tree);

        let size = std::fs::metadata(&output_path)?.len();
        if size > MAX_VERSION_OUTPUT_BYTES {
            anyhow::bail!("version probe output exceeded {MAX_VERSION_OUTPUT_BYTES} bytes");
        }
        let mut output = Vec::new();
        std::fs::File::open(&output_path)?
            .take(MAX_VERSION_OUTPUT_BYTES + 1)
            .read_to_end(&mut output)?;
        if output.len() as u64 > MAX_VERSION_OUTPUT_BYTES {
            anyhow::bail!("version probe output exceeded {MAX_VERSION_OUTPUT_BYTES} bytes");
        }
        Ok((status, String::from_utf8_lossy(&output).into_owned()))
    })();
    if let Err(error) = std::fs::remove_file(&output_path) {
        tracing::warn!(path = %output_path.display(), %error, "version probe output cleanup failed");
    }
    result
}

#[cfg(windows)]
fn verify_authenticode(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
        WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_IGNORE,
        WTD_UICONTEXT_INSTALL, WTD_UI_NONE,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR::from_raw(wide_path.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut trust_data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_IGNORE,
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | WTD_DISABLE_MD2_MD4,
        dwUIContext: WTD_UICONTEXT_INSTALL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            (&mut trust_data as *mut WINTRUST_DATA).cast(),
        )
    };
    if status != 0 {
        anyhow::bail!("Authenticode trust check failed with status 0x{status:08x}");
    }
    verify_update_publisher(path)
}

#[cfg(windows)]
fn verify_update_publisher(path: &Path) -> anyhow::Result<()> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    const MAX_THUMBPRINT_OUTPUT: u64 = 256;
    const PUBLISHER_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    let expected = option_env!("ALTER_UPDATE_PUBLISHER_SHA256")
        .map(normalize_thumbprint)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "automatic update is disabled because this build has no pinned publisher certificate SHA-256"
            )
        })?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("embedded update publisher certificate SHA-256 is invalid");
    }

    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| anyhow::anyhow!("SystemRoot is unavailable"))?;
    let powershell = Path::new(&system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let script = "$s=Get-AuthenticodeSignature -LiteralPath $args[0]; if($null -eq $s.SignerCertificate){exit 2}; [Console]::Out.Write($s.SignerCertificate.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256))";
    let output_path = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("update executable has no parent directory"))?
        .join(format!(".alter-publisher-probe-{}.txt", Uuid::new_v4()));
    let output_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&output_path)?;

    // A file avoids an inherited pipe keeping EOF open after PowerShell exits.
    let result = (|| -> anyhow::Result<()> {
        let mut child = std::process::Command::new(powershell)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output_file))
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000 | 0x0100_0000)
            .spawn()?;
        let probe_tree =
            match crate::process::tree::ProcessTreeGuard::new(child.id(), "update-publisher-probe")
            {
                Ok(tree) => tree,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("failed to contain publisher probe process tree: {error}");
                }
            };
        let deadline = std::time::Instant::now() + PUBLISHER_PROBE_TIMEOUT;
        let status = loop {
            if std::fs::metadata(&output_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                > MAX_THUMBPRINT_OUTPUT
            {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("publisher certificate probe output exceeded the limit");
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("publisher certificate probe timed out after 10 seconds");
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.into());
                }
            }
        };
        drop(probe_tree);
        let mut bytes = Vec::new();
        std::fs::File::open(&output_path)?
            .take(MAX_THUMBPRINT_OUTPUT + 1)
            .read_to_end(&mut bytes)?;
        if !status.success() || bytes.len() as u64 > MAX_THUMBPRINT_OUTPUT {
            anyhow::bail!("publisher certificate probe failed");
        }
        let actual = normalize_thumbprint(&String::from_utf8_lossy(&bytes));
        if actual != expected {
            anyhow::bail!("update publisher certificate does not match the pinned SHA-256");
        }
        Ok(())
    })();
    if let Err(error) = std::fs::remove_file(&output_path) {
        tracing::warn!(path = %output_path.display(), %error, "publisher probe output cleanup failed");
    }
    result
}

fn normalize_thumbprint(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| byte.is_ascii_hexdigit())
        .map(|byte| (byte as char).to_ascii_uppercase())
        .collect()
}

#[cfg(windows)]
fn restore_update_backup(current_exe: &Path, backup: &Path) -> anyhow::Result<()> {
    let failed = current_exe.with_file_name(format!(".alter-update-failed-{}.exe", Uuid::new_v4()));
    if current_exe.exists() {
        std::fs::rename(current_exe, &failed)?;
    }
    if let Err(error) = std::fs::rename(backup, current_exe) {
        let _ = std::fs::rename(&failed, current_exe);
        anyhow::bail!(
            "rollback failed: {error}; verified backup remains at {}",
            backup.display()
        );
    }
    let _ = std::fs::remove_file(failed);
    Ok(())
}

#[cfg(not(windows))]
fn install_and_verify(_path: &Path, _current_exe: &Path, _expected: &str) -> anyhow::Result<()> {
    anyhow::bail!(
        "automatic update is disabled because this platform has no verified rollback protocol"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_url_is_pinned_to_owner_repo_tag_and_asset() {
        let asset = "RunDock-1.2.3-windows-x64-setup.exe";
        assert!(trusted_release_url(
            "https://github.com/damingishere-coder/RunDock/releases/download/v1.2.3/RunDock-1.2.3-windows-x64-setup.exe",
            "v1.2.3",
            asset,
        ));
        for untrusted in [
            "https://github.com/attacker/RunDock/releases/download/v1.2.3/RunDock-1.2.3-windows-x64-setup.exe",
            "https://github.com/damingishere-coder/RunDock/releases/download/v1.2.4/RunDock-1.2.3-windows-x64-setup.exe",
            "https://example.com/damingishere-coder/RunDock/releases/download/v1.2.3/RunDock-1.2.3-windows-x64-setup.exe",
        ] {
            assert!(!trusted_release_url(untrusted, "v1.2.3", asset));
        }
    }

    #[test]
    fn sha256_and_semver_validation_fail_closed() {
        assert_eq!(valid_sha256(&"a".repeat(64)), Some("a".repeat(64)));
        assert!(valid_sha256(&"g".repeat(64)).is_none());
        assert!(valid_sha256("abcd").is_none());
        assert!(semver_gt("1.1.0", "1.2.0"));
        assert!(!semver_gt("1.1.0", "not-a-version"));
    }

    #[test]
    fn publisher_certificate_hash_normalization_is_case_and_separator_insensitive() {
        assert_eq!(normalize_thumbprint("aa bb-01"), "AABB01");
    }

    #[test]
    fn release_notes_are_bounded_without_splitting_utf8() {
        let notes = "界".repeat(MAX_RELEASE_NOTES_BYTES);
        let bounded = bounded_release_notes(&json!({ "body": notes })).unwrap();
        assert!(bounded.is_char_boundary(bounded.len()));
        assert!(bounded.len() <= MAX_RELEASE_NOTES_BYTES + 32);
        assert!(bounded.ends_with("[release notes truncated]"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn candidate_requires_expected_asset_and_digest() {
        let version = "99.0.0";
        let asset_name = platform_asset_name(version).unwrap();
        let release = json!({
            "tag_name": format!("v{version}"),
            "body": "notes",
            "published_at": "2026-08-25T00:00:00Z",
            "assets": [{
                "name": asset_name,
                "browser_download_url": format!(
                    "https://github.com/{RELEASE_OWNER}/{RELEASE_REPO}/releases/download/v{version}/{}",
                    platform_asset_name(version).unwrap()
                ),
                "digest": format!("sha256:{}", "b".repeat(64)),
            }],
        });
        let candidate = candidate_from_release(&release).unwrap().unwrap();
        assert_eq!(candidate.sha256, "b".repeat(64));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn automatic_update_is_disabled_without_a_supported_installer() {
        assert!(platform_asset_name("99.0.0").is_none());
    }
}
