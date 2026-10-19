//! Shared desktop access preference. The daemon and desktop shell use the same
//! file and data directory; no project or UI settings are copied between them.

use crate::config::{atomic_file, paths};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopAccess {
    #[serde(default)]
    pub desktop_only: bool,
    #[serde(default)]
    desktop_token: Option<String>,
}

impl DesktopAccess {
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::data_dir().join("desktop-access.json"))
    }

    fn load_from(path: &Path) -> Result<Self> {
        match atomic_file::read_bounded(path) {
            Ok(bytes) => {
                let config: Self = serde_json::from_slice(&bytes)
                    .context("RunDock desktop access configuration is invalid")?;
                config.validate()?;
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("cannot read RunDock desktop access configuration"),
        }
    }

    fn validate(&self) -> Result<()> {
        if let Some(token) = &self.desktop_token {
            anyhow::ensure!(
                token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "RunDock desktop access credential is invalid"
            );
        }
        anyhow::ensure!(
            !self.desktop_only || self.desktop_token.is_some(),
            "RunDock desktop-only mode requires a desktop credential"
        );
        Ok(())
    }

    /// Only the single-instance desktop shell initializes the credential.
    /// A daemon started through the CLI keeps the existing web default.
    pub fn initialize_desktop() -> Result<Self> {
        let path = paths::data_dir().join("desktop-access.json");
        let mut config = Self::load_from(&path)?;
        if config.desktop_token.is_none() {
            config.desktop_token = Some(format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(paths::data_dir())?;
            atomic_file::write_with_backup(&path, &serde_json::to_vec_pretty(&config)?, None)?;
        }
        Ok(config)
    }

    pub fn user_agent(&self) -> Option<String> {
        self.desktop_token
            .as_ref()
            .map(|token| format!("RunDockDesktop/{token}"))
    }

    pub fn accepts_user_agent(&self, agent: Option<&str>) -> bool {
        self.user_agent()
            .as_deref()
            .is_some_and(|expected| agent == Some(expected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_configuration_preserves_web_default() {
        let path = std::env::temp_dir().join(format!("desktop-access-{}", uuid::Uuid::new_v4()));
        assert!(!DesktopAccess::load_from(&path).unwrap().desktop_only);
    }

    #[test]
    fn invalid_configuration_does_not_reopen_the_web_dashboard() {
        let path =
            std::env::temp_dir().join(format!("desktop-access-{}.json", uuid::Uuid::new_v4()));
        for bytes in [
            br#"{"desktop_only":true}"#.as_slice(),
            br#"{"desktop_only":true,"desktop_token":"bad"}"#.as_slice(),
            b"broken".as_slice(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(DesktopAccess::load_from(&path).is_err());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn desktop_credential_requires_an_exact_match() {
        let config = DesktopAccess {
            desktop_only: true,
            desktop_token: Some("a".repeat(64)),
        };
        let agent = config.user_agent().unwrap();
        assert!(config.accepts_user_agent(Some(&agent)));
        assert!(!config.accepts_user_agent(None));
        assert!(!config.accepts_user_agent(Some("RunDockDesktop/")));
        assert!(!config.accepts_user_agent(Some(&format!("Mozilla/5.0 {agent}"))));
    }
}
