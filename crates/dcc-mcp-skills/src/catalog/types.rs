//! Data types for the skill catalog: state, entries, summary, and detail.

use dcc_mcp_models::{SkillMetadata, ToolDeclaration};

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    /// Parsed skill metadata from SKILL.md.
    pub metadata: SkillMetadata,
    /// Current load state.
    pub state: SkillState,
    /// Names of actions registered from this skill (populated on load).
    pub registered_actions: Vec<String>,
}

// ── Summary / Detail types ──

/// Lightweight summary of a skill for search/list results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillSummary", get_all)
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
    pub tool_names: Vec<String>,
    pub loaded: bool,
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
    pub registered_actions: Vec<String>,
}

// ── Python bindings for summary ──

#[cfg(feature = "python-bindings")]
#[pyo3::pymethods]
impl SkillSummary {
    fn __repr__(&self) -> String {
        format!("SkillSummary(name={:?}, loaded={})", self.name, self.loaded)
    }
}
