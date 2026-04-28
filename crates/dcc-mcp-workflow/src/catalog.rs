//! [`WorkflowCatalog`] — in-memory directory of workflow specs declared by
//! skills via the `metadata.dcc-mcp.workflows` glob key.
//!
//! See the amendment comment on issue #348: workflows are sibling YAML files
//! referenced from the skill's `metadata:` block, not a top-level SKILL.md
//! field. The catalog reads the glob, records the matched paths, and (in
//! this skeleton) parses only the `name` + `description` + `inputs` header
//! of each file. Full-body parse is deferred until `workflows.lookup` or
//! `workflows.run` actually touches the entry.

use std::path::{Path, PathBuf};

use dcc_mcp_models::registry::{Registry, SearchQuery};
use dcc_mcp_models::{RegistryEntry, SkillMetadata};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;

/// Metadata-key name under `SkillMetadata.metadata` that holds the workflow
/// glob. Namespaced under `dcc-mcp.*` so it cannot collide with other
/// agentskills.io clients.
pub const METADATA_KEY_WORKFLOWS: &str = "dcc-mcp.workflows";

/// Lightweight summary of a workflow declared by a skill.
///
/// Populated from the YAML header only (name + description + inputs) so the
/// catalog can list many workflows cheaply. The `path` field records the
/// absolute path to the YAML file for full parse on demand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSummary {
    /// Workflow name (YAML top-level `name` field, falls back to the file
    /// stem).
    pub name: String,
    /// Skill this workflow belongs to (the owning skill's `name`).
    pub skill: String,
    /// One-line description from the YAML header, if present.
    #[serde(default)]
    pub description: String,
    /// Input schema declaration (opaque JSON in the skeleton).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub inputs: serde_json::Value,
    /// Absolute path to the source `*.workflow.yaml` file.
    pub path: String,
}

/// In-memory catalog of workflow summaries.
///
/// Populated by [`Self::from_skill`] per skill; callers may merge multiple
/// skills into one catalog with [`Self::extend_from_skill`].
///
/// Interior mutability via `parking_lot::RwLock` allows the catalog to
/// implement [`Registry<WorkflowSummary>`] (which requires `&self`) while
/// still supporting mutation through `extend_from_skill`.
#[derive(Debug, Default)]
pub struct WorkflowCatalog {
    entries: RwLock<Vec<WorkflowSummary>>,
}

impl Clone for WorkflowCatalog {
    fn clone(&self) -> Self {
        Self {
            entries: RwLock::new(self.entries.read().clone()),
        }
    }
}

impl WorkflowCatalog {
    /// Create an empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a catalog from a single skill. Reads the
    /// `metadata["dcc-mcp.workflows"]` glob (see [`METADATA_KEY_WORKFLOWS`])
    /// relative to `skill_root`.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Io`] on glob pattern failure. Individual
    /// YAML parse failures are logged and skipped — a malformed workflow
    /// must not kill the catalog for the rest of the skill.
    pub fn from_skill(meta: &SkillMetadata, skill_root: &Path) -> Result<Self, WorkflowError> {
        let cat = Self::new();
        cat.extend_from_skill(meta, skill_root)?;
        Ok(cat)
    }

    /// Merge workflow summaries from another skill into this catalog.
    ///
    /// Takes `&self` (interior mutability via `RwLock`) so the catalog can
    /// implement [`Registry<WorkflowSummary>`] which requires `&self`.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Io`] on glob-pattern failure.
    pub fn extend_from_skill(
        &self,
        meta: &SkillMetadata,
        skill_root: &Path,
    ) -> Result<(), WorkflowError> {
        for path in resolve_workflow_paths(meta, skill_root)? {
            match read_summary(&path, &meta.name) {
                Ok(summary) => self.entries.write().push(summary),
                Err(e) => {
                    tracing::warn!(
                        "workflow catalog: failed to summarise {}: {e}",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }

    /// All recorded summaries (cloned — interior mutability prevents returning
    /// a reference into the locked `Vec`).
    #[must_use]
    pub fn entries(&self) -> Vec<WorkflowSummary> {
        self.entries.read().clone()
    }

    /// Look up a single summary by `(skill, name)`.
    #[must_use]
    pub fn get(&self, skill: &str, name: &str) -> Option<WorkflowSummary> {
        self.entries
            .read()
            .iter()
            .find(|s| s.skill == skill && s.name == name)
            .cloned()
    }

    /// Search summaries by a free-text query (substring match, case-insensitive)
    /// against `name` + `description`. Used by `workflows.lookup`.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<WorkflowSummary> {
        let q = query.to_ascii_lowercase();
        self.entries
            .read()
            .iter()
            .filter(|s| {
                s.name.to_ascii_lowercase().contains(&q)
                    || s.description.to_ascii_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }
}

// ── RegistryEntry impl ────────────────────────────────────────────────────────

impl RegistryEntry for WorkflowSummary {
    /// Composite key: `"<skill>::<name>"` to avoid name collisions across
    /// skills that may declare workflows with the same local name.
    fn key(&self) -> String {
        format!("{}::{}", self.skill, self.name)
    }

    /// Search tokens: name, skill, and description.
    fn search_tags(&self) -> Vec<String> {
        [
            self.name.clone(),
            self.skill.clone(),
            self.description.clone(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
    }
}

// ── impl Registry<WorkflowSummary> ───────────────────────────────────────────

/// Satisfy the shared [`Registry<WorkflowSummary>`] contract.
///
/// Preserves insertion order (unlike `HashMap`-based `DefaultRegistry`) by
/// using the `Vec` as the backing store. Upserts overwrite in place to keep
/// ordering stable for existing entries.
impl Registry<WorkflowSummary> for WorkflowCatalog {
    fn register(&self, entry: WorkflowSummary) {
        let mut guard = self.entries.write();
        let key = entry.key();
        match guard.iter().position(|e| e.key() == key) {
            Some(pos) => guard[pos] = entry,
            None => guard.push(entry),
        }
    }

    fn get(&self, key: &str) -> Option<WorkflowSummary> {
        self.entries.read().iter().find(|e| e.key() == key).cloned()
    }

    fn list(&self) -> Vec<WorkflowSummary> {
        self.entries.read().clone()
    }

    fn remove(&self, key: &str) -> bool {
        let mut guard = self.entries.write();
        let before = guard.len();
        guard.retain(|e| e.key() != key);
        guard.len() < before
    }

    fn count(&self) -> usize {
        self.entries.read().len()
    }

    fn search(&self, query: &SearchQuery) -> Vec<WorkflowSummary> {
        let q = query.query.to_ascii_lowercase();
        let mut results: Vec<WorkflowSummary> = self
            .entries
            .read()
            .iter()
            .filter(|v| {
                v.search_tags()
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        results
    }
}

/// Resolve the glob(s) declared under `metadata["dcc-mcp.workflows"]`
/// relative to `skill_root`. Returns absolute paths.
///
/// Accepts a comma-separated list of globs inside a single string.
///
/// # Errors
///
/// Returns [`WorkflowError::Io`] on glob pattern error.
pub fn resolve_workflow_paths(
    meta: &SkillMetadata,
    skill_root: &Path,
) -> Result<Vec<PathBuf>, WorkflowError> {
    let raw = match meta.metadata.get(METADATA_KEY_WORKFLOWS) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => {
            tracing::warn!(
                "skill {:?}: metadata.{METADATA_KEY_WORKFLOWS} must be a string, got {}",
                meta.name,
                other
            );
            return Ok(Vec::new());
        }
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::new();
    for part in raw.split(',') {
        let pattern = part.trim();
        if pattern.is_empty() {
            continue;
        }
        let joined = skill_root.join(pattern);
        let full = joined.to_string_lossy().to_string();
        match glob::glob(&full) {
            Ok(paths) => {
                for entry in paths {
                    match entry {
                        Ok(p) if p.is_file() => out.push(p),
                        Ok(_) => {}
                        Err(e) => tracing::warn!(
                            "workflow glob {pattern:?} entry error in skill {:?}: {e}",
                            meta.name
                        ),
                    }
                }
            }
            Err(e) => {
                return Err(WorkflowError::Io(format!(
                    "bad glob pattern {pattern:?} for skill {:?}: {e}",
                    meta.name
                )));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Parse only the header fields of a workflow YAML.
///
/// TODO(#348-full): lazy full parse on demand when `workflows.run` resolves
/// a `{skill, name}` pair. The summary is cheap; the full [`WorkflowSpec`]
/// parse is deferred until execution is implemented in the follow-up PR.
fn read_summary(path: &Path, skill: &str) -> Result<WorkflowSummary, WorkflowError> {
    let content = std::fs::read_to_string(path)?;

    #[derive(Deserialize, Default)]
    struct Header {
        #[serde(default)]
        name: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        inputs: serde_json::Value,
    }

    // Use `serde_yaml_ng` with extra fields ignored (default behaviour).
    let header: Header = serde_yaml_ng::from_str(&content).unwrap_or_default();

    let name = if header.name.is_empty() {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .trim_end_matches(".workflow")
            .to_string()
    } else {
        header.name
    };

    Ok(WorkflowSummary {
        name,
        skill: skill.to_string(),
        description: header.description,
        inputs: header.inputs,
        path: path.to_string_lossy().to_string(),
    })
}
