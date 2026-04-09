//! SkillMetadata — parsed from SKILL.md frontmatter.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};
use serde::{Deserialize, Serialize};

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
    /// Useful when a single skill has multiple tools backed by different scripts.
    ///
    /// Example in SKILL.md:
    /// ```yaml
    /// tools:
    ///   - name: create_mesh
    ///     source_file: scripts/create.py
    ///   - name: delete_mesh
    ///     source_file: scripts/delete.py
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

/// Metadata parsed from a SKILL.md frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillMetadata", get_all, set_all, from_py_object)
)]
pub struct SkillMetadata {
    pub name: String,

    #[serde(default)]
    pub description: String,

    /// Tool declarations from SKILL.md frontmatter.
    ///
    /// In SKILL.md, tools can be declared as either:
    /// - Simple string names: `tools: ["bevel", "extrude"]`  (becomes ToolDeclaration with name only)
    /// - Full declarations: `tools: [{name: "bevel", description: "...", ...}]`
    #[serde(default, deserialize_with = "deserialize_tool_declarations")]
    pub tools: Vec<ToolDeclaration>,

    #[serde(default = "default_dcc")]
    pub dcc: String,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub scripts: Vec<String>,

    #[serde(default)]
    pub skill_path: String,

    #[serde(default = "default_version")]
    pub version: String,

    /// Skill dependencies — names of other skills this skill requires.
    #[serde(default)]
    pub depends: Vec<String>,

    /// Files discovered under the metadata/ directory (e.g. help.md, install.md).
    #[serde(default)]
    pub metadata_files: Vec<String>,
}

/// Custom deserializer that accepts both `tools: ["name1", "name2"]` (simple strings)
/// and `tools: [{name: "bevel", description: "..."}]` (full declarations).
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

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillMetadata {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), tools=vec![], dcc=DEFAULT_DCC.to_string(), tags=vec![], scripts=vec![], skill_path="".to_string(), version=DEFAULT_VERSION.to_string(), depends=vec![], metadata_files=vec![]))]
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
        }
    }

    fn __repr__(&self) -> String {
        format!("SkillMetadata(name={:?}, dcc={:?})", self.name, self.dcc)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

impl std::fmt::Display for SkillMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{} ({})", self.name, self.version, self.dcc)
    }
}

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
        assert_eq!(meta.version, DEFAULT_VERSION); // default
        assert!(meta.depends.is_empty());
        assert!(meta.metadata_files.is_empty());
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
        // Default dcc and version come from serde defaults, but Rust Default uses ""
        assert_eq!(meta.name, "minimal");
        assert!(meta.tools.is_empty());
        assert!(meta.scripts.is_empty());
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn test_skill_metadata_serde_round_trip() {
        let meta = SkillMetadata {
            name: "full-skill".to_string(),
            description: "A full skill".to_string(),
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
    fn test_skill_metadata_clone_eq() {
        let meta = SkillMetadata {
            name: "cloned-skill".to_string(),
            dcc: "houdini".to_string(),
            version: "0.1.0".to_string(),
            ..Default::default()
        };
        let cloned = meta.clone();
        assert_eq!(meta, cloned);
    }

    #[test]
    fn test_skill_metadata_inequality() {
        let a = SkillMetadata {
            name: "skill-a".to_string(),
            ..Default::default()
        };
        let b = SkillMetadata {
            name: "skill-b".to_string(),
            ..Default::default()
        };
        assert_ne!(a, b);
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
    fn test_skill_metadata_scripts_list() {
        let json = r#"{"name": "scripted", "scripts": ["setup.py", "cleanup.py"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.scripts, vec!["setup.py", "cleanup.py"]);
    }

    #[test]
    fn test_skill_metadata_metadata_files() {
        let json = r#"{"name": "documented", "metadata_files": ["help.md", "install.md", "changelog.md"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.metadata_files.len(), 3);
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
