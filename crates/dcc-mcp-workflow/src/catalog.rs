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

use dcc_mcp_models::SkillMetadata;
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
#[derive(Debug, Default, Clone)]
pub struct WorkflowCatalog {
    entries: Vec<WorkflowSummary>,
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
        let mut cat = Self::new();
        cat.extend_from_skill(meta, skill_root)?;
        Ok(cat)
    }

    /// Merge workflow summaries from another skill into this catalog.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Io`] on glob-pattern failure.
    pub fn extend_from_skill(
        &mut self,
        meta: &SkillMetadata,
        skill_root: &Path,
    ) -> Result<(), WorkflowError> {
        for path in resolve_workflow_paths(meta, skill_root)? {
            match read_summary(&path, &meta.name) {
                Ok(summary) => self.entries.push(summary),
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

    /// All recorded summaries.
    #[must_use]
    pub fn entries(&self) -> &[WorkflowSummary] {
        &self.entries
    }

    /// Look up a single summary by `(skill, name)`.
    #[must_use]
    pub fn get(&self, skill: &str, name: &str) -> Option<&WorkflowSummary> {
        self.entries
            .iter()
            .find(|s| s.skill == skill && s.name == name)
    }

    /// Search summaries by a free-text query (substring match, case-insensitive)
    /// against `name` + `description`. Used by `workflows.lookup`.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&WorkflowSummary> {
        let q = query.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|s| {
                s.name.to_ascii_lowercase().contains(&q)
                    || s.description.to_ascii_lowercase().contains(&q)
            })
            .collect()
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
