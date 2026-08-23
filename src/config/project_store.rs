// @group Configuration : Persistent logical-project display metadata

use crate::config::paths;
use crate::models::project::{ProjectKind, ProjectRecord, DEFAULT_PROJECT_CATEGORY};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectStore {
    #[serde(default)]
    pub projects: HashMap<Uuid, ProjectRecord>,
}

impl ProjectStore {
    pub fn ensure(&mut self, id: Uuid, fallback_name: &str) -> &mut ProjectRecord {
        self.projects.entry(id).or_insert_with(|| ProjectRecord {
            id,
            kind: ProjectKind::Managed,
            display_name: fallback_name.to_string(),
            note: String::new(),
            category: DEFAULT_PROJECT_CATEGORY.to_string(),
            web_port: None,
            launch_uri: None,
        })
    }
}

pub fn load() -> ProjectStore {
    let path = paths::projects_file();
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save(store: &ProjectStore) -> Result<()> {
    let path = paths::projects_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(store)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &content)?;
    if let Err(error) = replace_file_atomically(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
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

#[cfg(not(target_os = "windows"))]
fn replace_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    std::fs::rename(source, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_common_project_without_changing_existing_metadata() {
        let id = Uuid::new_v4();
        let mut store = ProjectStore::default();
        let record = store.ensure(id, "Backend");
        assert_eq!(record.display_name, "Backend");
        assert_eq!(record.category, DEFAULT_PROJECT_CATEGORY);
        assert_eq!(record.kind, ProjectKind::Managed);
        assert_eq!(record.web_port, None);
        assert_eq!(record.launch_uri, None);
        record.note = "keep".to_string();
        assert_eq!(store.ensure(id, "Other").note, "keep");
        assert_eq!(store.ensure(id, "Other").display_name, "Backend");
    }

    #[test]
    fn legacy_project_metadata_loads_as_managed_without_desktop_fields() {
        let id = Uuid::new_v4();
        let record: ProjectRecord = serde_json::from_value(serde_json::json!({
            "id": id,
            "display_name": "Legacy",
            "note": "keep",
            "category": "常用"
        }))
        .unwrap();

        assert_eq!(record.kind, ProjectKind::Managed);
        assert_eq!(record.web_port, None);
        assert_eq!(record.launch_uri, None);
    }

    #[test]
    fn atomic_replace_overwrites_existing_metadata_file() {
        let directory =
            std::env::temp_dir().join(format!("alter-project-store-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("projects.json");
        let source = directory.join("projects.json.tmp");
        std::fs::write(&destination, "old").unwrap();
        std::fs::write(&source, "new").unwrap();

        replace_file_atomically(&source, &destination).unwrap();

        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "new");
        assert!(!source.exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
