//! Persistence types for `SkillCatalog.loaded` (issue #1405).
//!
//! When an MCP / REST client loads a skill into a DCC adapter, that
//! decision is purely in-memory by default — restarting the host process
//! (Maya, Blender, the standalone `dcc-mcp-server`…) loses the entire
//! `loaded` set and the user has to re-run `load_skill` for every skill
//! they want active.
//!
//! Track B of the admin-UI overhaul adds opt-in persistence so the host
//! can checkpoint the `loaded` set (and the active tool-group selection)
//! to disk and replay it on the next start. The on-disk format and the
//! sqlite mirror that backs the admin UI both live outside this crate —
//! this module defines the value types that flow through the
//! [`crate::catalog::SkillCatalog::replay_loaded`] entry point and the
//! after-load / after-unload / after-group hooks.

use serde::{Deserialize, Serialize};

/// One persisted skill load entry.
///
/// Identified by `name`. `version` and `skill_path` are recorded at the
/// time of load so the replay policy can detect drift (the SKILL.md on
/// disk now declares a different version, or the skill directory has
/// moved or been removed entirely).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LoadedSkillRecord {
    /// Skill name (catalog key).
    pub name: String,
    /// Skill version recorded at load time. `None` for skills loaded by
    /// older code that didn't capture the version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Absolute path to the skill directory at load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// UNIX-ms timestamp captured when the skill was loaded.
    #[serde(default)]
    pub loaded_at_ms: i64,
}

impl LoadedSkillRecord {
    /// Minimal constructor used by the catalog when emitting events to a
    /// host-side store.
    #[must_use]
    pub fn from_metadata(name: &str, version: &str, skill_path: &str, loaded_at_ms: i64) -> Self {
        let version = if version.trim().is_empty() {
            None
        } else {
            Some(version.to_string())
        };
        let skill_path = if skill_path.trim().is_empty() {
            None
        } else {
            Some(skill_path.to_string())
        };
        Self {
            name: name.to_string(),
            version,
            skill_path,
            loaded_at_ms,
        }
    }
}

/// Full persisted catalog snapshot.
///
/// Written by [`crate::catalog::SkillCatalog`] consumers (Python
/// `LoadedStateStore`) on every state change and read back on startup to
/// drive [`crate::catalog::SkillCatalog::replay_loaded`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCatalogState {
    /// Skills that were loaded at checkpoint time.
    #[serde(default)]
    pub skills: Vec<LoadedSkillRecord>,
    /// Tool-group names that were active at checkpoint time. Catalog-wide
    /// because [`crate::catalog::SkillCatalog::active_groups`] is a flat
    /// set, not per-skill.
    #[serde(default)]
    pub active_groups: Vec<String>,
    /// UNIX-ms timestamp the snapshot was written.
    #[serde(default)]
    pub saved_at_ms: i64,
    /// Schema version. Bump if the on-disk shape changes incompatibly.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 {
    1
}

/// How [`crate::catalog::SkillCatalog::replay_loaded`] handles a
/// persisted record whose on-disk skill no longer matches the recorded
/// version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadReplayPolicy {
    /// Skip the load with a warning and record the entry under
    /// [`ReplayReport::skipped_drift`]. Default — the user can always
    /// re-load manually if they accept the new version.
    #[default]
    SkipOnDrift,
    /// Refuse to replay any record whose version differs. Used by
    /// embedders that pin skill versions and treat drift as a fatal
    /// startup error.
    RequireExactVersion,
    /// Ignore the recorded version entirely and load whatever is on disk.
    /// Used by embedders that always trust the local checkout.
    IgnoreVersion,
}

/// Outcome of a [`crate::catalog::SkillCatalog::replay_loaded`] call.
///
/// Each persisted record falls into exactly one of the four buckets so
/// hosts can surface a per-record breakdown in their admin UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayReport {
    /// Skills that were successfully re-loaded.
    #[serde(default)]
    pub loaded: Vec<String>,
    /// Records whose skill could not be found in the catalog (the
    /// directory was deleted / moved out of the scan roots).
    #[serde(default)]
    pub missing: Vec<String>,
    /// Records whose on-disk skill version no longer matches the
    /// persisted one and the policy refused / skipped the load.
    #[serde(default)]
    pub skipped_drift: Vec<DriftRecord>,
    /// Records whose `load_skill` call returned an error string.
    #[serde(default)]
    pub failed: Vec<FailedRecord>,
    /// Active groups that were successfully re-activated after the
    /// skills above were loaded.
    #[serde(default)]
    pub activated_groups: Vec<String>,
}

/// Detail for a skill that was skipped due to version drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftRecord {
    pub name: String,
    pub persisted_version: Option<String>,
    pub current_version: String,
}

/// Detail for a skill whose load attempt returned an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedRecord {
    pub name: String,
    pub error: String,
}
