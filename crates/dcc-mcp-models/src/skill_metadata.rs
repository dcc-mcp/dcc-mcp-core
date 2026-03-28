//! SkillMetadata — parsed from SKILL.md frontmatter.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use serde::{Deserialize, Serialize};

/// Metadata parsed from a SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillMetadata"))]
pub struct SkillMetadata {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default)]
    pub description: String,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default)]
    pub tools: Vec<String>,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default = "default_dcc")]
    pub dcc: String,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default)]
    pub tags: Vec<String>,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default)]
    pub scripts: Vec<String>,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default)]
    pub skill_path: String,

    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_dcc() -> String {
    "python".to_string()
}

fn default_version() -> String {
    "1.0.0".to_string()
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillMetadata {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), tools=vec![], dcc="python".to_string(), tags=vec![], scripts=vec![], skill_path="".to_string(), version="1.0.0".to_string()))]
    fn new(
        name: String,
        description: String,
        tools: Vec<String>,
        dcc: String,
        tags: Vec<String>,
        scripts: Vec<String>,
        skill_path: String,
        version: String,
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
        }
    }

    fn __repr__(&self) -> String {
        format!("SkillMetadata(name={:?}, dcc={:?})", self.name, self.dcc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_metadata_deserialize() {
        let yaml = r#"
name: test-skill
description: A test skill
dcc: maya
tags:
  - geometry
  - creation
"#;
        let meta: SkillMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.tags, vec!["geometry", "creation"]);
        assert_eq!(meta.version, "1.0.0"); // default
    }
}
