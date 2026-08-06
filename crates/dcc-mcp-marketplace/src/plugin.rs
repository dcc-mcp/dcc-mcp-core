use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::add_repo::extract_skill_frontmatter;
use crate::error::MarketplaceError;

pub(crate) const AGENT_PLUGIN_SCHEMA_V1: &str =
    "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AgentPluginManifest {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<AgentPluginAuthor>,
    pub homepage: Option<String>,
    #[serde(rename = "repository")]
    pub _repository: Option<String>,
    #[serde(rename = "license")]
    pub _license: Option<String>,
    pub keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentPluginAuthor {
    pub name: Option<String>,
    #[serde(rename = "email")]
    pub _email: Option<String>,
    #[serde(rename = "url")]
    pub _url: Option<String>,
}

#[derive(Debug)]
pub(crate) struct AgentPluginPackage {
    pub manifest: AgentPluginManifest,
    pub skill_dirs: Vec<PathBuf>,
}

pub(crate) fn load_agent_plugin(
    package_root: &Path,
) -> Result<Option<AgentPluginPackage>, MarketplaceError> {
    let Some(plugin_root) = find_plugin_root(package_root)? else {
        return Ok(None);
    };
    let manifest_path = plugin_root.join("plugin.json");
    ensure_contained(&plugin_root, &manifest_path)?;
    let text = fs::read_to_string(&manifest_path)
        .map_err(|err| MarketplaceError::ConfigIo(manifest_path.display().to_string(), err))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(manifest_path.display().to_string(), err))?;
    report_ignored_manifest_fields(&value);
    let manifest: AgentPluginManifest = serde_json::from_value(value)
        .map_err(|err| MarketplaceError::ConfigParse(manifest_path.display().to_string(), err))?;
    validate_manifest(&manifest)?;

    let skills_root = plugin_root.join("skills");
    let skill_dirs = if !skills_root.exists() {
        Vec::new()
    } else if !skills_root.is_dir() {
        eprintln!(
            "warning: Agent Plugin '{}' has a non-directory skills component; ignoring it",
            manifest.name
        );
        Vec::new()
    } else {
        match ensure_contained(&plugin_root, &skills_root) {
            Ok(()) => discover_skills(&plugin_root, &skills_root)?,
            Err(err) => {
                eprintln!("warning: {err}; ignoring Agent Plugin skills component");
                Vec::new()
            }
        }
    };

    Ok(Some(AgentPluginPackage {
        manifest,
        skill_dirs,
    }))
}

fn find_plugin_root(package_root: &Path) -> Result<Option<PathBuf>, MarketplaceError> {
    if package_root.join("plugin.json").is_file() {
        return Ok(Some(package_root.to_path_buf()));
    }
    let children = fs::read_dir(package_root)
        .map_err(|err| MarketplaceError::ConfigIo(package_root.display().to_string(), err))?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return Ok(None);
    };
    Ok(child.join("plugin.json").is_file().then(|| child.clone()))
}

fn validate_manifest(manifest: &AgentPluginManifest) -> Result<(), MarketplaceError> {
    if manifest.schema != AGENT_PLUGIN_SCHEMA_V1 {
        return Err(MarketplaceError::CommandFailed(format!(
            "Agent Plugin '{}' targets unsupported schema '{}'",
            manifest.name, manifest.schema
        )));
    }
    if !valid_plugin_name(&manifest.name) {
        return Err(MarketplaceError::CommandFailed(format!(
            "Agent Plugin name '{}' must be 1-64 lowercase alphanumeric, '-' or '.' characters without repeated separators",
            manifest.name
        )));
    }
    Ok(())
}

fn report_ignored_manifest_fields(value: &Value) {
    const KNOWN: &[&str] = &[
        "$schema",
        "name",
        "version",
        "description",
        "author",
        "homepage",
        "repository",
        "license",
        "keywords",
        "extensions",
    ];
    let Some(object) = value.as_object() else {
        return;
    };
    for key in object.keys().filter(|key| !KNOWN.contains(&key.as_str())) {
        eprintln!("warning: ignoring unknown Agent Plugin manifest field '{key}'");
    }
    if object
        .get("extensions")
        .is_some_and(|value| !value.is_object())
    {
        eprintln!("warning: ignoring non-object Agent Plugin manifest field 'extensions'");
    }
}

fn valid_plugin_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'-' | b'.')
        })
        && !name.contains("--")
        && !name.contains("..")
}

fn discover_skills(
    plugin_root: &Path,
    skills_root: &Path,
) -> Result<Vec<PathBuf>, MarketplaceError> {
    let mut skills = Vec::new();
    for entry in fs::read_dir(skills_root)
        .map_err(|err| MarketplaceError::ConfigIo(skills_root.display().to_string(), err))?
        .flatten()
    {
        let skill_dir = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        if let Err(err) = ensure_contained(plugin_root, &skill_md) {
            eprintln!("warning: {err}; skipping Agent Plugin skill");
            continue;
        }
        let Some(skill) = extract_skill_frontmatter(&skill_dir) else {
            eprintln!(
                "warning: skipping invalid Agent Plugin skill '{}'",
                skill_dir.display()
            );
            continue;
        };
        if skill.description.as_deref().is_none_or(str::is_empty) {
            eprintln!(
                "warning: skipping Agent Plugin skill '{}' without a description",
                skill.name
            );
            continue;
        }
        skills.push(skill_dir);
    }
    skills.sort();
    Ok(skills)
}

fn ensure_contained(root: &Path, path: &Path) -> Result<(), MarketplaceError> {
    let resolved_root = fs::canonicalize(root)
        .map_err(|err| MarketplaceError::ConfigIo(root.display().to_string(), err))?;
    let resolved_path = fs::canonicalize(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    if resolved_path.starts_with(&resolved_root) {
        return Ok(());
    }
    Err(MarketplaceError::CommandFailed(format!(
        "Agent Plugin path '{}' resolves outside plugin root '{}'",
        path.display(),
        root.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(root: &Path, name: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("plugin.json"),
            format!(
                r#"{{"$schema":"{AGENT_PLUGIN_SCHEMA_V1}","name":"{name}","version":"1.0.0"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn discovers_only_immediate_valid_agent_skills() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "test-plugin");
        let valid = tmp.path().join("skills/valid");
        fs::create_dir_all(&valid).unwrap();
        fs::write(
            valid.join("SKILL.md"),
            "---\nname: valid\ndescription: Valid skill\n---\n",
        )
        .unwrap();
        let nested = tmp.path().join("skills/group/nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: nested\ndescription: Nested skill\n---\n",
        )
        .unwrap();

        let plugin = load_agent_plugin(tmp.path()).unwrap().unwrap();
        assert_eq!(plugin.manifest.name, "test-plugin");
        assert_eq!(plugin.skill_dirs, vec![valid]);
    }

    #[test]
    fn rejects_unsupported_schema() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"future"}"#,
        )
        .unwrap();
        assert!(load_agent_plugin(tmp.path()).is_err());
    }
}
