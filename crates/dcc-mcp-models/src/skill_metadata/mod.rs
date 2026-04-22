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

impl SkillMetadata {
    /// Access the `metadata.openclaw` section if present (ClawHub format).
    ///
    /// Returns `None` if this skill doesn't have ClawHub metadata.
    pub fn openclaw_metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_object().and_then(|m| {
            m.get("openclaw")
                .or_else(|| m.get("clawdbot"))
                .or_else(|| m.get("clawdis"))
        })
    }

    /// Union of DCC capabilities required by any tool in this skill (issue #354).
    ///
    /// Computed lazily from each [`ToolDeclaration::required_capabilities`].
    /// The result is deduplicated and sorted, so two calls on the same skill
    /// always produce the same ordering.
    ///
    /// ```
    /// use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
    /// let mut md = SkillMetadata::default();
    /// md.tools = vec![
    ///     ToolDeclaration { name: "a".into(), required_capabilities: vec!["usd".into(), "scene.read".into()], ..Default::default() },
    ///     ToolDeclaration { name: "b".into(), required_capabilities: vec!["usd".into(), "scene.mutate".into()], ..Default::default() },
    /// ];
    /// assert_eq!(md.required_capabilities(), vec!["scene.mutate", "scene.read", "usd"]);
    /// ```
    pub fn required_capabilities(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tool in &self.tools {
            for cap in &tool.required_capabilities {
                if !cap.is_empty() {
                    set.insert(cap.clone());
                }
            }
        }
        set.into_iter().collect()
    }

    /// Get required environment variables declared by this skill (ClawHub).
    pub fn required_env_vars(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|r| r.get("env"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get required binaries declared by this skill (ClawHub).
    pub fn required_bins(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|r| r.get("bins"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get the primary credential environment variable (ClawHub `primaryEnv`).
    pub fn primary_env(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("primaryEnv"))
            .and_then(|v| v.as_str())
    }

    /// Get the emoji display for this skill (ClawHub).
    pub fn emoji(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("emoji"))
            .and_then(|v| v.as_str())
    }

    /// Get the homepage URL for this skill (ClawHub).
    pub fn homepage(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("homepage"))
            .and_then(|v| v.as_str())
    }

    /// Whether this skill is always active (no explicit load needed) (ClawHub `always`).
    pub fn always_active(&self) -> bool {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("always"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Get OS restrictions for this skill (ClawHub `os`).
    pub fn os_restrictions(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("os"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get flat metadata key-value pairs (agentskills.io style).
    ///
    /// Returns only top-level string values, ignoring nested objects (like `openclaw`).
    pub fn flat_metadata(&self) -> HashMap<&str, &str> {
        self.metadata
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns `true` iff no legacy top-level extension fields were used
    /// when this skill's SKILL.md was parsed.
    ///
    /// Spec-compliant skills declare all dcc-mcp-specific keys under the
    /// `metadata.dcc-mcp.*` namespace (agentskills.io v1.0). Legacy skills
    /// declared them as top-level YAML fields (`dcc`, `tags`, `tools`, …).
    /// See issue #356.
    pub fn is_spec_compliant(&self) -> bool {
        self.legacy_extension_fields.is_empty()
    }

    /// Returns true if this skill has any validation warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // name: lowercase + hyphens only, max 64 chars
        if self.name.len() > 64 {
            warnings.push(format!(
                "name '{}' exceeds 64 chars (agentskills.io limit)",
                self.name
            ));
        }
        if self.name.starts_with('-') || self.name.ends_with('-') {
            warnings.push(format!(
                "name '{}' must not start or end with a hyphen",
                self.name
            ));
        }
        if self.name.contains("--") {
            warnings.push(format!(
                "name '{}' must not contain consecutive hyphens",
                self.name
            ));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            warnings.push(format!(
                "name '{}' should be lowercase letters, digits, and hyphens only",
                self.name
            ));
        }

        // description: max 1024 chars (agentskills.io)
        if self.description.len() > 1024 {
            warnings.push(format!(
                "description length {} exceeds 1024 chars (agentskills.io limit)",
                self.description.len()
            ));
        }

        // compatibility: max 500 chars (agentskills.io)
        if self.compatibility.len() > 500 {
            warnings.push(format!(
                "compatibility length {} exceeds 500 chars (agentskills.io limit)",
                self.compatibility.len()
            ));
        }

        warnings
    }
}

// ── Deserializers ─────────────────────────────────────────────────────────

/// Deserialize `allowed-tools` from either a space-delimited string or a YAML list.
///
/// Handles:
/// - `allowed-tools: "Bash Read Write"` → `["Bash", "Read", "Write"]`
/// - `allowed-tools: [Bash, Read, Write]` → `["Bash", "Read", "Write"]`
fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct AllowedToolsVisitor;

    impl<'de> Visitor<'de> for AllowedToolsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a space-delimited string or a sequence of tool names")
        }

        // `allowed-tools: "Bash Read Write"` or `allowed-tools: "Bash(git:*) Read"`
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        fn visit_string<E: de::Error>(self, s: String) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        // `allowed-tools: [Bash, Read, Write]`
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut tools = Vec::new();
            while let Some(v) = seq.next_element::<String>()? {
                tools.push(v);
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_any(AllowedToolsVisitor)
}

/// Custom deserializer for `tools` — accepts both string names and full objects.
fn deserialize_tool_declarations<'de, D>(deserializer: D) -> Result<Vec<ToolDeclaration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct ToolDeclarationsVisitor;

    impl<'de> Visitor<'de> for ToolDeclarationsVisitor {
        type Value = Vec<ToolDeclaration>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "a sequence of tool name strings or tool declaration objects"
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut tools = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match &value {
                    serde_json::Value::String(s) => {
                        tools.push(ToolDeclaration {
                            name: s.clone(),
                            ..Default::default()
                        });
                    }
                    serde_json::Value::Object(_) => {
                        let decl: ToolDeclaration =
                            serde_json::from_value(value).map_err(de::Error::custom)?;
                        tools.push(decl);
                    }
                    _ => {
                        return Err(de::Error::custom(
                            "each tool must be a string name or a declaration object",
                        ));
                    }
                }
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_seq(ToolDeclarationsVisitor)
}

fn default_dcc() -> String {
    DEFAULT_DCC.to_string()
}

fn default_version() -> String {
    DEFAULT_VERSION.to_string()
}

// ── Python bindings ───────────────────────────────────────────────────────

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillMetadata {
    #[new]
    #[pyo3(signature = (
        name,
        description = "".to_string(),
        tools = vec![],
        dcc = DEFAULT_DCC.to_string(),
        tags = vec![],
        search_hint = "".to_string(),
        scripts = vec![],
        skill_path = "".to_string(),
        version = DEFAULT_VERSION.to_string(),
        depends = vec![],
        metadata_files = vec![],
        license = "".to_string(),
        compatibility = "".to_string(),
        allowed_tools = vec![],
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        tools: Vec<ToolDeclaration>,
        dcc: String,
        tags: Vec<String>,
        search_hint: String,
        scripts: Vec<String>,
        skill_path: String,
        version: String,
        depends: Vec<String>,
        metadata_files: Vec<String>,
        license: String,
        compatibility: String,
        allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            name,
            description,
            tools,
            dcc,
            tags,
            search_hint,
            scripts,
            skill_path,
            version,
            depends,
            metadata_files,
            license,
            compatibility,
            allowed_tools,
            metadata: serde_json::Value::Null,
            policy: None,
            external_deps: None,
            groups: Vec::new(),
            legacy_extension_fields: Vec::new(),
            prompts_file: None,
        }
    }

    fn __repr__(&self) -> String {
        format!("SkillMetadata(name={:?}, dcc={:?})", self.name, self.dcc)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __eq__(&self, other: &SkillMetadata) -> bool {
        self == other
    }

    // ── Simple field getters/setters ───────────────────────────────────

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }

    #[getter]
    fn dcc(&self) -> &str {
        &self.dcc
    }
    #[setter]
    fn set_dcc(&mut self, v: String) {
        self.dcc = v;
    }

    #[getter]
    fn version(&self) -> &str {
        &self.version
    }
    #[setter]
    fn set_version(&mut self, v: String) {
        self.version = v;
    }

    #[getter]
    fn license(&self) -> &str {
        &self.license
    }
    #[setter]
    fn set_license(&mut self, v: String) {
        self.license = v;
    }

    #[getter]
    fn compatibility(&self) -> &str {
        &self.compatibility
    }
    #[setter]
    fn set_compatibility(&mut self, v: String) {
        self.compatibility = v;
    }

    #[getter]
    fn skill_path(&self) -> &str {
        &self.skill_path
    }
    #[setter]
    fn set_skill_path(&mut self, v: String) {
        self.skill_path = v;
    }

    #[getter]
    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }
    #[setter]
    fn set_tags(&mut self, v: Vec<String>) {
        self.tags = v;
    }

    #[getter]
    fn search_hint(&self) -> &str {
        &self.search_hint
    }
    #[setter]
    fn set_search_hint(&mut self, v: String) {
        self.search_hint = v;
    }

    #[getter]
    fn scripts(&self) -> Vec<String> {
        self.scripts.clone()
    }
    #[setter]
    fn set_scripts(&mut self, v: Vec<String>) {
        self.scripts = v;
    }

    #[getter]
    fn depends(&self) -> Vec<String> {
        self.depends.clone()
    }
    #[setter]
    fn set_depends(&mut self, v: Vec<String>) {
        self.depends = v;
    }

    #[getter]
    fn metadata_files(&self) -> Vec<String> {
        self.metadata_files.clone()
    }
    #[setter]
    fn set_metadata_files(&mut self, v: Vec<String>) {
        self.metadata_files = v;
    }

    #[getter]
    fn allowed_tools(&self) -> Vec<String> {
        self.allowed_tools.clone()
    }
    #[setter]
    fn set_allowed_tools(&mut self, v: Vec<String>) {
        self.allowed_tools = v;
    }

    #[getter]
    fn tools(&self) -> Vec<ToolDeclaration> {
        self.tools.clone()
    }
    #[setter]
    fn set_tools(&mut self, v: Vec<ToolDeclaration>) {
        self.tools = v;
    }

    #[getter]
    fn groups(&self) -> Vec<SkillGroup> {
        self.groups.clone()
    }
    #[setter]
    fn set_groups(&mut self, v: Vec<SkillGroup>) {
        self.groups = v;
    }

    // ── metadata field: JSON value exposed as Python dict ──────────────

    /// Returns metadata as a Python dict.
    #[getter]
    fn metadata(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        let val = if self.metadata.is_null() {
            serde_json::json!({})
        } else {
            self.metadata.clone()
        };
        json_value_to_pyobject(py, &val)
    }

    /// Set metadata from a Python dict (serialized to JSON internally).
    #[setter]
    fn set_metadata(&mut self, py: pyo3::Python<'_>, value: Py<PyAny>) -> pyo3::PyResult<()> {
        use dcc_mcp_utils::py_json::py_any_to_json_value;
        self.metadata = py_any_to_json_value(value.bind(py))
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    // ── policy / external_deps ────────────────────────────────────────

    /// Returns the invocation policy serialised as a JSON string, or `None`.
    ///
    /// Parse with `json.loads(skill.policy)` in Python.
    #[getter]
    fn policy(&self) -> Option<String> {
        self.policy
            .as_ref()
            .and_then(|p| serde_json::to_string(p).ok())
    }

    /// Set the invocation policy from a JSON string (or `None` to clear).
    #[setter]
    fn set_policy(&mut self, value: Option<String>) {
        self.policy = value.and_then(|s| serde_json::from_str::<SkillPolicy>(&s).ok());
    }

    /// Returns `true` if implicit invocation is allowed for this skill.
    #[pyo3(name = "is_implicit_invocation_allowed")]
    fn py_is_implicit_invocation_allowed(&self) -> bool {
        self.policy
            .as_ref()
            .map(|p| p.is_implicit_invocation_allowed())
            .unwrap_or(true)
    }

    /// Returns `true` if this skill is available for the given DCC product.
    #[pyo3(name = "matches_product")]
    fn py_matches_product(&self, product: String) -> bool {
        self.policy
            .as_ref()
            .map(|p| p.matches_product(&product))
            .unwrap_or(true)
    }

    /// Returns the external dependencies serialised as a JSON string, or `None`.
    ///
    /// Parse with `json.loads(skill.external_deps)` in Python.
    #[getter]
    fn external_deps(&self) -> Option<String> {
        self.external_deps
            .as_ref()
            .and_then(|d| serde_json::to_string(d).ok())
    }

    /// Set external dependencies from a JSON string (or `None` to clear).
    #[setter]
    fn set_external_deps(&mut self, value: Option<String>) {
        self.external_deps = value.and_then(|s| serde_json::from_str::<SkillDependencies>(&s).ok());
    }

    // ── ClawHub convenience methods ────────────────────────────────────

    /// Required environment variables (ClawHub `metadata.openclaw.requires.env`).
    #[pyo3(name = "required_env_vars")]
    fn py_required_env_vars(&self) -> Vec<String> {
        SkillMetadata::required_env_vars(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Required binaries (ClawHub `metadata.openclaw.requires.bins`).
    #[pyo3(name = "required_bins")]
    fn py_required_bins(&self) -> Vec<String> {
        SkillMetadata::required_bins(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Primary credential env var (ClawHub `primaryEnv`).
    #[pyo3(name = "primary_env")]
    fn py_primary_env(&self) -> Option<String> {
        SkillMetadata::primary_env(self).map(String::from)
    }

    /// Emoji display (ClawHub).
    #[pyo3(name = "emoji")]
    fn py_emoji(&self) -> Option<String> {
        SkillMetadata::emoji(self).map(String::from)
    }

    /// Homepage URL (ClawHub).
    #[pyo3(name = "homepage")]
    fn py_homepage(&self) -> Option<String> {
        SkillMetadata::homepage(self).map(String::from)
    }

    /// Validate spec constraints. Returns a list of warning strings.
    #[pyo3(name = "validate")]
    fn py_validate(&self) -> Vec<String> {
        SkillMetadata::validate(self)
    }

    /// Returns ``True`` iff this skill uses the agentskills.io-compliant
    /// ``metadata.dcc-mcp.*`` form exclusively (no legacy top-level
    /// extension keys).  See issue #356.
    #[pyo3(name = "is_spec_compliant")]
    fn py_is_spec_compliant(&self) -> bool {
        SkillMetadata::is_spec_compliant(self)
    }

    /// Names of legacy top-level extension fields that were observed when
    /// parsing this skill's SKILL.md.  Empty list ⇒ spec-compliant.
    #[getter]
    fn legacy_extension_fields(&self) -> Vec<String> {
        self.legacy_extension_fields.clone()
    }

    /// Union of DCC capabilities required by any tool in this skill (issue #354).
    ///
    /// Returns a deduplicated, sorted list of capability tags aggregated
    /// from every `ToolDeclaration.required_capabilities` on this skill.
    #[pyo3(name = "required_capabilities")]
    fn py_required_capabilities(&self) -> Vec<String> {
        SkillMetadata::required_capabilities(self)
    }
}

impl std::fmt::Display for SkillMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{} ({})", self.name, self.version, self.dcc)
    }
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
