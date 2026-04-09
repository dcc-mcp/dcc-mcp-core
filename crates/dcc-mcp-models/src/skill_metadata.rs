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

// ── ToolDeclaration ───────────────────────────────────────────────────────

/// Declaration of a tool provided by a skill, parsed from SKILL.md frontmatter.
///
/// Unlike `ActionMeta`, this is a lightweight declaration that can be discovered
/// without loading the skill's scripts. It carries enough information for agents
/// to decide whether to load a skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolDeclaration", eq, from_py_object)
)]
pub struct ToolDeclaration {
    /// Tool name (unique within the skill).
    #[serde(default)]
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// JSON Schema for input parameters (as serde_json::Value).
    #[serde(default)]
    pub input_schema: serde_json::Value,

    /// JSON Schema for output (as serde_json::Value).
    #[serde(default, skip_serializing_if = "is_null_value")]
    pub output_schema: serde_json::Value,

    /// Whether this tool only reads data (no side effects).
    #[serde(default)]
    pub read_only: bool,

    /// Whether this tool may cause destructive changes.
    #[serde(default)]
    pub destructive: bool,

    /// Whether calling this tool with the same args always produces the same result.
    #[serde(default)]
    pub idempotent: bool,

    /// Explicit path to the script that implements this tool.
    ///
    /// If empty, the catalog will try to find a matching script by name.
    ///
    /// Example in SKILL.md:
    /// ```yaml
    /// tools:
    ///   - name: create_mesh
    ///     source_file: scripts/create.py
    /// ```
    #[serde(default)]
    pub source_file: String,
}

fn is_null_value(v: &serde_json::Value) -> bool {
    v.is_null()
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolDeclaration {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), input_schema=None, output_schema=None, read_only=false, destructive=false, idempotent=false, source_file="".to_string()))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        input_schema: Option<String>,
        output_schema: Option<String>,
        read_only: bool,
        destructive: bool,
        idempotent: bool,
        source_file: String,
    ) -> Self {
        let input_schema = input_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({"type": "object"}));
        let output_schema = output_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        Self {
            name,
            description,
            input_schema,
            output_schema,
            read_only,
            destructive,
            idempotent,
            source_file,
        }
    }

    fn __repr__(&self) -> String {
        format!("ToolDeclaration(name={:?})", self.name)
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[setter]
    fn set_name(&mut self, value: String) {
        self.name = value;
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }

    #[setter]
    fn set_description(&mut self, value: String) {
        self.description = value;
    }

    /// Returns input_schema as a JSON string.
    #[getter]
    fn input_schema(&self) -> String {
        self.input_schema.to_string()
    }

    /// Set input_schema from a JSON string.
    #[setter]
    fn set_input_schema(&mut self, value: String) {
        self.input_schema =
            serde_json::from_str(&value).unwrap_or(serde_json::json!({"type": "object"}));
    }

    /// Returns output_schema as a JSON string (empty string if null).
    #[getter]
    fn output_schema(&self) -> String {
        if self.output_schema.is_null() {
            String::new()
        } else {
            self.output_schema.to_string()
        }
    }

    /// Set output_schema from a JSON string.
    #[setter]
    fn set_output_schema(&mut self, value: String) {
        self.output_schema = if value.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&value).unwrap_or(serde_json::Value::Null)
        };
    }

    #[getter]
    fn read_only(&self) -> bool {
        self.read_only
    }

    #[setter]
    fn set_read_only(&mut self, value: bool) {
        self.read_only = value;
    }

    #[getter]
    fn destructive(&self) -> bool {
        self.destructive
    }

    #[setter]
    fn set_destructive(&mut self, value: bool) {
        self.destructive = value;
    }

    #[getter]
    fn idempotent(&self) -> bool {
        self.idempotent
    }

    #[setter]
    fn set_idempotent(&mut self, value: bool) {
        self.idempotent = value;
    }

    #[getter]
    fn source_file(&self) -> &str {
        &self.source_file
    }

    #[setter]
    fn set_source_file(&mut self, value: String) {
        self.source_file = value;
    }
}

impl std::fmt::Display for ToolDeclaration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolDeclaration({})", self.name)
    }
}

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
            scripts,
            skill_path,
            version,
            depends,
            metadata_files,
            license,
            compatibility,
            allowed_tools,
            metadata: serde_json::Value::Null,
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
}

impl std::fmt::Display for SkillMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{} ({})", self.name, self.version, self.dcc)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deserialization / defaults ──────────────────────────────────────────────

    #[test]
    fn test_skill_metadata_deserialize() {
        let json = r#"{
            "name": "test-skill",
            "description": "A test skill",
            "dcc": "maya",
            "tags": ["geometry", "creation"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.tags, vec!["geometry", "creation"]);
        assert_eq!(meta.version, DEFAULT_VERSION);
        assert!(meta.depends.is_empty());
        assert!(meta.metadata_files.is_empty());
        assert!(meta.license.is_empty());
        assert!(meta.compatibility.is_empty());
        assert!(meta.allowed_tools.is_empty());
        assert!(meta.metadata.is_null());
    }

    #[test]
    fn test_agentskills_standard_fields() {
        let json = r#"{
            "name": "pdf-tools",
            "description": "Extract text from PDFs. Use when working with PDF files.",
            "license": "MIT",
            "compatibility": "Requires Python 3.9+",
            "allowed-tools": "Bash Read Write",
            "metadata": {"author": "studio", "category": "documents"}
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.license, "MIT");
        assert_eq!(meta.compatibility, "Requires Python 3.9+");
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Write"]);
        let flat = meta.flat_metadata();
        assert_eq!(flat.get("author"), Some(&"studio"));
        assert_eq!(flat.get("category"), Some(&"documents"));
    }

    #[test]
    fn test_allowed_tools_yaml_list() {
        let json = r#"{
            "name": "test",
            "allowed-tools": ["Bash", "Read", "Edit"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Edit"]);
    }

    #[test]
    fn test_allowed_tools_alias() {
        let json = r#"{"name": "test", "allowed_tools": ["Bash"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.allowed_tools, vec!["Bash"]);
    }

    #[test]
    fn test_clawhub_metadata_openclaw() {
        let yaml_json = r#"{
            "name": "ffmpeg-media",
            "description": "Media conversion via FFmpeg",
            "version": "1.0.0",
            "metadata": {
                "openclaw": {
                    "requires": {
                        "bins": ["ffmpeg", "ffprobe"],
                        "env": ["FFMPEG_PATH"]
                    },
                    "primaryEnv": "FFMPEG_PATH",
                    "emoji": "🎬",
                    "homepage": "https://ffmpeg.org",
                    "os": ["linux", "macos"],
                    "always": false
                }
            }
        }"#;
        let meta: SkillMetadata = serde_json::from_str(yaml_json).unwrap();
        assert_eq!(meta.required_bins(), vec!["ffmpeg", "ffprobe"]);
        assert_eq!(meta.required_env_vars(), vec!["FFMPEG_PATH"]);
        assert_eq!(meta.primary_env(), Some("FFMPEG_PATH"));
        assert_eq!(meta.emoji(), Some("🎬"));
        assert_eq!(meta.homepage(), Some("https://ffmpeg.org"));
        assert_eq!(meta.os_restrictions(), vec!["linux", "macos"]);
        assert!(!meta.always_active());
    }

    #[test]
    fn test_clawhub_metadata_alias_clawdbot() {
        let json = r#"{
            "name": "test",
            "metadata": {
                "clawdbot": {
                    "emoji": "🦀"
                }
            }
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.emoji(), Some("🦀"));
    }

    #[test]
    fn test_all_three_standards_combined() {
        let json = r#"{
            "name": "maya-bevel",
            "description": "Bevel tools for Maya. Use when beveling polygon edges.",
            "license": "MIT",
            "compatibility": "Maya 2022+, Python 3.7+",
            "allowed-tools": "Bash Read",
            "metadata": {
                "author": "studio",
                "openclaw": {
                    "requires": {"bins": ["maya"]},
                    "emoji": "🎨"
                }
            },
            "dcc": "maya",
            "version": "2.0.0",
            "tags": ["modeling", "polygon"],
            "tools": [
                {"name": "bevel", "description": "Apply bevel to edges"}
            ]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        // agentskills.io fields
        assert_eq!(meta.license, "MIT");
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read"]);
        // ClawHub fields
        assert_eq!(meta.required_bins(), vec!["maya"]);
        assert_eq!(meta.emoji(), Some("🎨"));
        // flat metadata
        assert_eq!(meta.flat_metadata().get("author"), Some(&"studio"));
        // dcc-mcp-core extensions
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.tools[0].name, "bevel");
    }

    #[test]
    fn test_validate_name_constraints() {
        let valid = SkillMetadata {
            name: "my-skill-v2".to_string(),
            ..Default::default()
        };
        assert!(valid.validate().is_empty());

        let too_long = SkillMetadata {
            name: "a".repeat(65),
            ..Default::default()
        };
        assert!(!too_long.validate().is_empty());

        let starts_hyphen = SkillMetadata {
            name: "-bad".to_string(),
            ..Default::default()
        };
        assert!(!starts_hyphen.validate().is_empty());

        let uppercase = SkillMetadata {
            name: "MySkill".to_string(),
            ..Default::default()
        };
        assert!(!uppercase.validate().is_empty());
    }

    #[test]
    fn test_skill_metadata_with_depends() {
        let json = r#"{
            "name": "pipeline",
            "depends": ["geometry-tools", "usd-tools"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.depends, vec!["geometry-tools", "usd-tools"]);
    }

    #[test]
    fn test_skill_metadata_display() {
        let meta = SkillMetadata {
            name: "my-skill".to_string(),
            version: "2.0.0".to_string(),
            dcc: "maya".to_string(),
            ..Default::default()
        };
        assert_eq!(meta.to_string(), "my-skill v2.0.0 (maya)");
    }

    #[test]
    fn test_skill_metadata_default_values() {
        let meta = SkillMetadata {
            name: "minimal".to_string(),
            ..Default::default()
        };
        assert_eq!(meta.name, "minimal");
        assert!(meta.tools.is_empty());
        assert!(meta.scripts.is_empty());
        assert!(meta.tags.is_empty());
        assert!(meta.license.is_empty());
        assert!(meta.allowed_tools.is_empty());
    }

    #[test]
    fn test_skill_metadata_serde_round_trip() {
        let meta = SkillMetadata {
            name: "full-skill".to_string(),
            description: "A full skill".to_string(),
            license: "MIT".to_string(),
            compatibility: "Python 3.7+".to_string(),
            allowed_tools: vec!["Bash".to_string(), "Read".to_string()],
            metadata: serde_json::json!({"author": "test"}),
            tools: vec![
                ToolDeclaration {
                    name: "create_mesh".to_string(),
                    ..Default::default()
                },
                ToolDeclaration {
                    name: "delete_mesh".to_string(),
                    ..Default::default()
                },
            ],
            dcc: "blender".to_string(),
            tags: vec!["modeling".to_string()],
            scripts: vec!["init.py".to_string()],
            skill_path: "/skills/full".to_string(),
            version: "1.2.3".to_string(),
            depends: vec!["base-skill".to_string()],
            metadata_files: vec!["help.md".to_string()],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: SkillMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn test_skill_metadata_tools_list() {
        let json =
            r#"{"name": "tools-skill", "tools": ["mesh_bevel", "mesh_extrude", "mesh_inset"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools.len(), 3);
        assert_eq!(meta.tools[0].name, "mesh_bevel");
        assert_eq!(meta.tools[1].name, "mesh_extrude");
        assert_eq!(meta.tools[2].name, "mesh_inset");
    }

    #[test]
    fn test_tool_declaration_full_object() {
        let json = r#"{"name": "tools-skill", "tools": [{"name": "bevel", "description": "Bevel edges", "read_only": false, "destructive": true, "idempotent": true}]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools.len(), 1);
        assert_eq!(meta.tools[0].name, "bevel");
        assert_eq!(meta.tools[0].description, "Bevel edges");
        assert!(!meta.tools[0].read_only);
        assert!(meta.tools[0].destructive);
        assert!(meta.tools[0].idempotent);
    }

    #[test]
    fn test_skill_metadata_deserialize_all_dccs() {
        for dcc in &["maya", "blender", "houdini", "3dsmax", "unreal", "unity"] {
            let json = format!(r#"{{"name": "test", "dcc": "{dcc}"}}"#);
            let meta: SkillMetadata = serde_json::from_str(&json).unwrap();
            assert_eq!(&meta.dcc, dcc);
        }
    }
}
