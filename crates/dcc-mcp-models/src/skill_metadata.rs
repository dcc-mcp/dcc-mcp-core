//! SkillMetadata — parsed from SKILL.md frontmatter.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use serde::{Deserialize, Serialize};

/// Metadata parsed from a SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillMetadata"))]
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

    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }
    #[getter]
    fn get_tools(&self) -> Vec<String> {
        self.tools.clone()
    }
    #[setter]
    fn set_tools(&mut self, v: Vec<String>) {
        self.tools = v;
    }
    #[getter]
    fn get_dcc(&self) -> &str {
        &self.dcc
    }
    #[setter]
    fn set_dcc(&mut self, v: String) {
        self.dcc = v;
    }
    #[getter]
    fn get_tags(&self) -> Vec<String> {
        self.tags.clone()
    }
    #[setter]
    fn set_tags(&mut self, v: Vec<String>) {
        self.tags = v;
    }
    #[getter]
    fn get_scripts(&self) -> Vec<String> {
        self.scripts.clone()
    }
    #[setter]
    fn set_scripts(&mut self, v: Vec<String>) {
        self.scripts = v;
    }
    #[getter]
    fn get_skill_path(&self) -> &str {
        &self.skill_path
    }
    #[setter]
    fn set_skill_path(&mut self, v: String) {
        self.skill_path = v;
    }
    #[getter]
    fn get_version(&self) -> &str {
        &self.version
    }
    #[setter]
    fn set_version(&mut self, v: String) {
        self.version = v;
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
