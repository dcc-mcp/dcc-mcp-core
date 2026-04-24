//! SkillMetadata — parsed from SKILL.md frontmatter.
//!
//! Supports three skill standards simultaneously:
//!
//! - **agentskills.io / Anthropic Skills**: `name`, `description`, `license`,
//!   `compatibility`, `metadata`, `allowed-tools`
//! - **ClawHub / OpenClaw**: `version`, `metadata.openclaw.*` (requires, install,
//!   primaryEnv, emoji, homepage, os, always, skillKey)
//! - **dcc-mcp-core extensions**: `dcc`, `tags`, `tools`, `depends`, `scripts`
//!
//! The same SKILL.md file can satisfy all three formats simultaneously.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod methods;
#[cfg(feature = "python-bindings")]
mod python;
mod serde_impl;

use serde_impl::{
    default_dcc, default_version, deserialize_allowed_tools, deserialize_tool_declarations,
};

// ── SkillMetadata ─────────────────────────────────────────────────────────

/// Metadata parsed from a SKILL.md frontmatter.
///
/// Supports all three skill standards:
///
/// ## Minimal (agentskills.io compatible)
/// ```yaml
/// ---
/// name: my-skill
/// description: What it does and when to use it.
/// ---
/// ```
///
/// ## Full (all standards)
/// ```yaml
/// ---
/// name: maya-bevel
/// description: Bevel tools for Maya polygon modeling.
/// # agentskills.io standard
/// license: MIT
/// compatibility: Maya 2022+, Python 3.7+
/// allowed-tools: Bash Read
/// metadata:
///   author: studio-name
///   category: modeling
/// # ClawHub / OpenClaw
/// version: "1.0.0"
/// metadata:
///   openclaw:
///     requires:
///       bins: [maya]
///     emoji: "🎨"
///     homepage: https://example.com
/// # dcc-mcp-core extensions
/// dcc: maya
/// tags: [modeling, polygon]
/// tools:
///   - name: bevel
///     description: Apply bevel to selected edges
/// ---
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillMetadata", from_py_object)
)]
pub struct SkillMetadata {
    /// Skill identifier — lowercase, hyphens only.
    /// Must match the parent directory name (agentskills.io requirement).
    pub name: String,

    /// Human-readable description of what the skill does and when to use it.
    /// Shown in skill discovery results. Keep under 1024 chars.
    #[serde(default)]
    pub description: String,

    // ── agentskills.io / Anthropic Skills standard fields ─────────────
    /// SPDX license identifier or short license description.
    /// Example: `"MIT"`, `"Apache-2.0"`, `"Proprietary"`
    #[serde(default)]
    pub license: String,

    /// Environment and dependency requirements for this skill.
    /// Example: `"Python 3.7+, Maya 2022+"`, `"Requires docker and git"`
    /// Keep under 500 chars.
    #[serde(default)]
    pub compatibility: String,

    /// Pre-approved tools this skill may use (agentskills.io `allowed-tools`).
    /// Space-delimited in SKILL.md YAML, stored as Vec<String> here.
    ///
    /// This is distinct from `tools` (MCP tool declarations):
    /// - `allowed-tools`: permission whitelist for agent capabilities (e.g. `["Bash", "Read"]`)
    /// - `tools`: MCP tool definitions with schemas
    ///
    /// Supports both space-delimited strings and YAML lists:
    /// ```yaml
    /// allowed-tools: Bash Read Write
    /// # or:
    /// allowed-tools: [Bash, Read, Write]
    /// ```
    #[serde(
        default,
        rename = "allowed-tools",
        alias = "allowed_tools",
        deserialize_with = "deserialize_allowed_tools"
    )]
    pub allowed_tools: Vec<String>,

    /// Arbitrary metadata key-value pairs.
    ///
    /// Used by both agentskills.io (flat KV strings) and ClawHub (`openclaw.*`
    /// nested structure). Stored as a JSON value to support both:
    ///
    /// ```yaml
    /// # agentskills.io flat style
    /// metadata:
    ///   author: studio-name
    ///   category: modeling
    ///
    /// # ClawHub nested style
    /// metadata:
    ///   openclaw:
    ///     requires:
    ///       bins: [ffmpeg]
    ///     emoji: "🎬"
    /// ```
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,

    // ── dcc-mcp-core extension fields ─────────────────────────────────
    /// Target DCC application (e.g. "maya", "blender", "houdini", "python").
    #[serde(default = "default_dcc")]
    pub dcc: String,

    /// Searchable tags for skill discovery.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Short search hint for lightweight skill discovery.
    ///
    /// Used by `search_skills` to match against without loading full tool schemas.
    /// Should be a comma-separated list of keywords or a short phrase, e.g.:
    /// `"polygon modeling, bevel, extrude, mesh editing"`
    ///
    /// Falls back to `description` if not set.
    #[serde(default, rename = "search-hint", alias = "search_hint")]
    pub search_hint: String,

    /// MCP tool declarations — defines the tools this skill exposes.
    ///
    /// Accepts both simple names and full declarations:
    /// ```yaml
    /// tools: ["bevel", "extrude"]
    /// # or with full schema:
    /// tools:
    ///   - name: bevel
    ///     description: Apply bevel to edges
    ///     source_file: scripts/bevel.py
    /// ```
    #[serde(default, deserialize_with = "deserialize_tool_declarations")]
    pub tools: Vec<ToolDeclaration>,

    /// Semantic version string.
    #[serde(default = "default_version")]
    pub version: String,

    /// Skill dependencies — names of other skills this skill requires.
    #[serde(default)]
    pub depends: Vec<String>,

    // ── Runtime-populated fields (not in YAML) ─────────────────────────
    /// Script files discovered in the `scripts/` subdirectory.
    /// Populated at load time, not from SKILL.md frontmatter.
    #[serde(default)]
    pub scripts: Vec<String>,

    /// Absolute path to the skill's root directory.
    /// Populated at load time.
    #[serde(default)]
    pub skill_path: String,

    /// Markdown files discovered in the `metadata/` subdirectory.
    /// Populated at load time.
    #[serde(default)]
    pub metadata_files: Vec<String>,

    // ── dcc-mcp-core: progressive discovery extensions ─────────────────
    /// Invocation policy declared in SKILL.md frontmatter.
    ///
    /// Controls whether the skill may be loaded implicitly and which
    /// DCC products it is available for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<SkillPolicy>,

    /// External dependencies declared in SKILL.md frontmatter.
    ///
    /// Declares required MCP servers, environment variables, or binaries
    /// that must be available for this skill to function correctly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_deps: Option<SkillDependencies>,

    /// Declared tool groups for progressive exposure (see [`SkillGroup`]).
    ///
    /// When a tool declares a group name that is not present in this list,
    /// the catalog auto-inserts an inactive placeholder group at load time.
    #[serde(default)]
    pub groups: Vec<SkillGroup>,

    /// Names of legacy top-level extension fields detected while parsing
    /// this skill's SKILL.md (issue #356).
    ///
    /// Populated by the loader, not by serde. When empty the skill uses the
    /// agentskills.io-compliant `metadata.dcc-mcp.*` form exclusively; when
    /// non-empty the skill still relies on deprecated top-level extension
    /// keys. See [`SkillMetadata::is_spec_compliant`].
    #[serde(default, skip_serializing, skip_deserializing)]
    pub legacy_extension_fields: Vec<String>,

    /// Sibling-file reference for the MCP prompts primitive (issues #351, #355).
    ///
    /// Set from `metadata.dcc-mcp.prompts` in SKILL.md frontmatter. The value
    /// is a path relative to the skill root — either a single YAML file that
    /// contains a top-level `prompts:` (and optional `workflows:`) list, or a
    /// glob (`prompts/*.prompt.yaml`) that enumerates one file per prompt.
    ///
    /// Parsing is deferred until the MCP server handles a `prompts/list` or
    /// `prompts/get` call, so a skill with 50 prompt files pays zero cost at
    /// scan / load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts_file: Option<String>,
}

mod execution;
mod skill_dependency;
mod skill_policy;
mod tests;
mod tool_declaration;

pub use execution::*;
pub use skill_dependency::*;
pub use skill_policy::*;
pub use tool_declaration::*;
