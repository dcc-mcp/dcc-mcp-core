//! SkillMetadata — parsed from SKILL.md frontmatter.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};
use serde::{Deserialize, Serialize};

/// Metadata parsed from a SKILL.md frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillMetadata", eq, get_all, set_all, from_py_object)
)]
pub struct SkillMetadata {
    pub name: String,

    #[serde(default)]
    pub description: String,

    #[serde(default)]
    pub tools: Vec<String>,

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
        tools: Vec<String>,
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
            tools: vec!["create_mesh".to_string(), "delete_mesh".to_string()],
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
        assert!(meta.tools.contains(&"mesh_bevel".to_string()));
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
