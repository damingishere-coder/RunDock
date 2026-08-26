// @group Configuration : Persistent logical-project display metadata

use crate::config::paths;
use crate::models::project::{ProjectKind, ProjectRecord, DEFAULT_PROJECT_CATEGORY};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.projects.len() <= 1_000,
            "project store contains too many projects"
        );
        for (id, project) in &self.projects {
            anyhow::ensure!(
                id == &project.id,
                "project store key does not match record ID"
            );
            anyhow::ensure!(
                !project.display_name.trim().is_empty() && project.display_name.len() <= 128,
                "project display name must contain between 1 and 128 bytes"
            );
            anyhow::ensure!(
                project.note.len() <= 4_096,
                "project note cannot exceed 4096 bytes"
            );
            anyhow::ensure!(
                !project.category.trim().is_empty() && project.category.len() <= 128,
                "project category must contain between 1 and 128 bytes"
            );
            anyhow::ensure!(
                project
                    .launch_uri
                    .as_deref()
                    .is_none_or(|uri| uri.len() <= 2_048),
                "project launch URI cannot exceed 2048 bytes"
            );
        }
        Ok(())
    }
}

pub fn load() -> Result<ProjectStore> {
    let path = paths::projects_file();
    crate::config::atomic_file::load_json_with_backup_validated(&path, ProjectStore::validate)
}

pub fn save(store: &ProjectStore) -> Result<()> {
    store.validate()?;
    let path = paths::projects_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::config::atomic_file::write_json_with_backup_validated(
        &path,
        store,
        ProjectStore::validate,
    )
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
        std::fs::write(&destination, "old").unwrap();

        crate::config::atomic_file::write_with_backup(&destination, b"new", None).unwrap();

        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "new");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
