//! SkillMetadata — parsed from SKILL.md frontmatter.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};
use serde::{Deserialize, Serialize};

/// Metadata parsed from a SKILL.md frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillMetadata", eq, get_all, set_all)
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
}
