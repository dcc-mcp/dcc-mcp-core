//! Persisted lifecycle operation records for `start-instance`.
//!
//! The store backs two contracts: convergence (a second identical request must
//! converge on the already-owned instance instead of launching another host) and
//! the guarded stop operation (only the operation that launched a process may
//! stop it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::domain::start_instance::{
    LIFECYCLE_DIR_NAME, LIFECYCLE_INDEX_FILE, LIFECYCLE_OPERATION_SCHEMA_VERSION,
    LifecycleOperation,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OperationIndex {
    /// `dcc_type|canonical-project` → owning operation id.
    #[serde(default)]
    operations: BTreeMap<String, String>,
}

/// Filesystem-backed store for lifecycle operation records.
#[derive(Debug, Clone)]
pub struct LifecycleStore {
    dir: PathBuf,
}

impl LifecycleStore {
    pub fn new(registry_dir: &Path) -> Self {
        Self {
            dir: registry_dir.join(LIFECYCLE_DIR_NAME),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Persist `operation` and bind it to its `dcc_type|project` key.
    pub fn save(&self, operation: &LifecycleOperation) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating lifecycle dir {}", self.dir.display()))?;
        let path = self.operation_path(&operation.operation_id);
        let encoded =
            serde_json::to_string_pretty(operation).context("encoding lifecycle operation")?;
        std::fs::write(&path, encoded)
            .with_context(|| format!("writing lifecycle operation {}", path.display()))?;

        let mut index = self.read_index()?;
        index.operations.insert(
            binding_key(&operation.dcc_type, &operation.project),
            operation.operation_id.clone(),
        );
        self.write_index(&index)?;
        Ok(())
    }

    pub fn load(&self, operation_id: &str) -> anyhow::Result<Option<LifecycleOperation>> {
        let path = self.operation_path(operation_id);
        if !path.is_file() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading lifecycle operation {}", path.display()))?;
        let operation: LifecycleOperation = serde_json::from_str(&raw)
            .with_context(|| format!("parsing lifecycle operation {}", path.display()))?;
        if operation.schema_version > LIFECYCLE_OPERATION_SCHEMA_VERSION {
            anyhow::bail!(
                "lifecycle operation {} uses schema_version {} newer than the supported {}",
                operation_id,
                operation.schema_version,
                LIFECYCLE_OPERATION_SCHEMA_VERSION
            );
        }
        Ok(Some(operation))
    }

    /// Owning operation recorded for `dcc_type` + `project`, when it still
    /// points at a stored record.
    pub fn find_owned(
        &self,
        dcc_type: &str,
        project: &Path,
    ) -> anyhow::Result<Option<LifecycleOperation>> {
        let index = self.read_index()?;
        if let Some(operation_id) = index.operations.get(&binding_key(dcc_type, project)) {
            return self.load(operation_id);
        }
        // Records store the canonical project path, so a caller that passes a
        // non-canonical (or differently-prefixed) path still has to find its
        // operation: fall back to a path comparison instead of only key lookup.
        for operation_id in index.operations.values() {
            let Some(operation) = self.load(operation_id)? else {
                continue;
            };
            if operation.dcc_type.eq_ignore_ascii_case(dcc_type)
                && crate::application::instance_launch::same_path(&operation.project, project)
            {
                return Ok(Some(operation));
            }
        }
        Ok(None)
    }

    fn operation_path(&self, operation_id: &str) -> PathBuf {
        self.dir.join(format!("{operation_id}.json"))
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join(LIFECYCLE_INDEX_FILE)
    }

    fn read_index(&self) -> anyhow::Result<OperationIndex> {
        let path = self.index_path();
        if !path.is_file() {
            return Ok(OperationIndex::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading lifecycle index {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing lifecycle index {}", path.display()))
    }

    fn write_index(&self, index: &OperationIndex) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating lifecycle dir {}", self.dir.display()))?;
        let encoded = serde_json::to_string_pretty(index).context("encoding lifecycle index")?;
        std::fs::write(self.index_path(), encoded)
            .with_context(|| format!("writing lifecycle index {}", self.index_path().display()))?;
        Ok(())
    }
}

/// Stable key binding one DCC type + project to one owning operation.
pub fn binding_key(dcc_type: &str, project: &Path) -> String {
    format!(
        "{}|{}",
        dcc_type.trim().to_ascii_lowercase(),
        project.display().to_string().to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation(id: &str, dcc_type: &str, project: &Path) -> LifecycleOperation {
        LifecycleOperation::new(
            id,
            dcc_type,
            project,
            Path::new("/opt/unity/Editor/Unity"),
            None,
        )
    }

    #[test]
    fn save_then_find_owned_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = LifecycleStore::new(dir.path());
        let project = dir.path().join("MyProject");

        store.save(&operation("op-1", "unity", &project)).unwrap();

        let found = store.find_owned("unity", &project).unwrap().unwrap();
        assert_eq!(found.operation_id, "op-1");
        assert_eq!(found.dcc_type, "unity");
    }

    #[test]
    fn binding_is_case_insensitive_on_dcc_type() {
        let dir = tempfile::tempdir().unwrap();
        let store = LifecycleStore::new(dir.path());
        let project = dir.path().join("MyProject");
        store.save(&operation("op-1", "Unity", &project)).unwrap();

        assert!(store.find_owned("unity", &project).unwrap().is_some());
    }

    #[test]
    fn different_projects_get_different_owners() {
        let dir = tempfile::tempdir().unwrap();
        let store = LifecycleStore::new(dir.path());
        let first = dir.path().join("One");
        let second = dir.path().join("Two");
        store.save(&operation("op-1", "unity", &first)).unwrap();
        store.save(&operation("op-2", "unity", &second)).unwrap();

        assert_eq!(
            store
                .find_owned("unity", &first)
                .unwrap()
                .unwrap()
                .operation_id,
            "op-1"
        );
        assert_eq!(
            store
                .find_owned("unity", &second)
                .unwrap()
                .unwrap()
                .operation_id,
            "op-2"
        );
    }

    #[test]
    fn load_returns_none_for_unknown_operations() {
        let dir = tempfile::tempdir().unwrap();
        let store = LifecycleStore::new(dir.path());

        assert!(store.load("missing").unwrap().is_none());
    }
}
