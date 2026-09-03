// @group Configuration : Tunnel settings — stored at %APPDATA%\alter-pm2\tunnel.json

use crate::models::tunnel::TunnelSettings;
use anyhow::Result;

// @group Configuration : Load tunnel settings from disk (returns default if missing or corrupt)
pub fn load() -> Result<TunnelSettings> {
    let path = crate::config::paths::data_dir().join("tunnel.json");
    let mut settings: TunnelSettings = crate::config::atomic_file::load_json_with_backup_validated(
        &path,
        |candidate: &TunnelSettings| {
            let mut normalized = candidate.clone();
            normalized.normalize();
            normalized.validate()
        },
    )?;
    settings.normalize();
    settings.validate()?;
    Ok(settings)
}

// @group Configuration : Persist tunnel settings to disk (atomic write)
pub fn save(settings: &TunnelSettings) -> Result<()> {
    let path = crate::config::paths::data_dir().join("tunnel.json");
    let mut normalized = settings.clone();
    normalized.normalize();
    normalized.validate()?;
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        &normalized,
        TunnelSettings::validate,
    )
}
