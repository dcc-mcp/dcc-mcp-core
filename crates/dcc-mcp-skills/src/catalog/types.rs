//! Data types for the skill catalog: state, entries, summary, and detail.

use dcc_mcp_models::{RegistryEntry, SkillMetadata, SkillScope, ToolDeclaration};
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde::Serializer;

// RTK-inspired: compact serialization for tool_names
fn serialize_tool_names<S>(tool_names: &[String], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let compact = tool_names.join(",");
    serializer.serialize_str(&compact)
}

// ── Skill state ──

/// Load state of a skill in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// Skill discovered but not loaded (tools not registered).
    Discovered,
    /// Skill loaded — tools registered in ActionRegistry.
    Loaded,
    /// Skill failed to load.
    Error(String),
}

impl std::fmt::Display for SkillState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillState::Discovered => write!(f, "discovered"),
            SkillState::Loaded => write!(f, "loaded"),
            SkillState::Error(e) => write!(f, "error: {e}"),
        }
    }
}

// ── Skill entry ──

/// A skill entry in the catalog, tracking its metadata and load state.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    /// Parsed skill metadata from SKILL.md.
    pub metadata: SkillMetadata,
    /// Current load state.
    pub state: SkillState,
    /// Names of tools registered from this skill (populated on load).
    pub registered_tools: Vec<String>,
    /// Trust level / origin of this skill.
    ///
    /// Set at discovery time based on which search path the skill was found in.
    /// Defaults to `SkillScope::Repo` when not explicitly assigned.
    pub scope: SkillScope,
}

// ── Summary / Detail types ──

/// Lightweight summary of a skill for search/list results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillSummary", get_all, from_py_object)
)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    /// Short hint used for keyword search (from SKILL.md `search-hint` field).
    /// Falls back to description if not set in SKILL.md.
    pub search_hint: String,
    pub tags: Vec<String>,
    pub dcc: String,
    pub version: String,
    pub tool_count: usize,
    /// RTK-inspired: compact comma-separated format when serialized to reduce token consumption.
    #[serde(serialize_with = "serialize_tool_names")]
    pub tool_names: Vec<String>,
    pub loaded: bool,
    /// Trust level / origin scope of this skill (e.g. `"repo"`, `"user"`, `"system"`).
    pub scope: String,
    /// `true` when this skill declares `allow_implicit_invocation: false`.
    pub implicit_invocation: bool,
}

/// Detailed information about a skill.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub dcc: String,
    pub version: String,
    pub depends: Vec<String>,
    pub skill_path: String,
    pub scripts: Vec<String>,
    pub tools: Vec<ToolDeclaration>,
    pub state: String,
    pub registered_tools: Vec<String>,
    /// Trust level / origin scope of this skill.
    pub scope: String,
    /// Whether this skill may be invoked implicitly (without explicit `load_skill`).
    pub implicit_invocation: bool,
    /// Number of declared external dependencies (MCP servers, env vars, binaries).
    pub dependency_count: usize,
}

// ── RegistryEntry impl ───────────────────────────────────────────────────────

impl RegistryEntry for SkillEntry {
    /// The stable lookup key is the skill's unique name.
    fn key(&self) -> String {
        self.metadata.name.clone()
    }

    /// Search tokens: name, description, DCC name, and declared tags.
    fn search_tags(&self) -> Vec<String> {
        let mut tags = vec![
            self.metadata.name.clone(),
            self.metadata.description.clone(),
            self.metadata.dcc.clone(),
            self.metadata.search_hint.clone(),
        ];
        tags.extend(self.metadata.tags.iter().cloned());
        tags.retain(|t| !t.is_empty());
        tags
    }
}

// ── Python bindings for summary ──

#[cfg(feature = "python-bindings")]
#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pyo3::pymethods]
impl SkillSummary {
    fn __repr__(&self) -> String {
        format!("SkillSummary(name={:?}, loaded={})", self.name, self.loaded)
    }
}
