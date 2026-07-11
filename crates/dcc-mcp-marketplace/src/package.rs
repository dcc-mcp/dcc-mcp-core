use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall, CatalogPolicy, CatalogRequirements};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use crate::error::MarketplaceError;
use crate::path_component;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplacePackOptions {
    pub source_dir: PathBuf,
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplacePackResult {
    pub package_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplacePublishOptions {
    pub package_dir: PathBuf,
    pub catalog_path: PathBuf,
    pub install_url: String,
    pub install_type: String,
    pub install_ref: Option<String>,
    pub skill_roots: Vec<String>,
    pub sha256: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub dcc: Vec<String>,
    pub version: Option<String>,
    pub maintainer: Option<String>,
    pub tags: Vec<String>,
    pub min_core_version: Option<String>,
    pub homepage_url: Option<String>,
    pub icon: Option<String>,
    pub showcase: Option<String>,
    pub requires_env: Vec<String>,
    pub requires_bin: Vec<String>,
    pub requires_python: Vec<String>,
    pub requires_skill: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplacePublishResult {
    pub catalog_path: String,
    pub entry: CatalogEntry,
    pub action: String,
    pub count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct CatalogDoc {
    #[serde(default = "default_catalog_version")]
    version: String,
    #[serde(default)]
    entries: Vec<CatalogEntry>,
}

pub fn pack_marketplace_package(
    options: MarketplacePackOptions,
) -> Result<MarketplacePackResult, MarketplaceError> {
    let source_dir = options.source_dir;
    if !source_dir.is_dir() {
        return Err(MarketplaceError::Read(
            source_dir.display().to_string(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "source directory not found"),
        ));
    }

    let package_name = path_component(
        "package name",
        source_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("marketplace-package"),
    )?;
    let out_path = resolve_pack_out_path(options.out, &source_dir, &package_name);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }

    let bytes = build_zip_bytes(&source_dir, &out_path)?;
    fs::write(&out_path, &bytes)
        .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
    Ok(MarketplacePackResult {
        package_path: out_path.display().to_string(),
        sha256: sha256_hex(&bytes),
        bytes: bytes.len() as u64,
    })
}

pub fn publish_marketplace_package(
    options: MarketplacePublishOptions,
) -> Result<MarketplacePublishResult, MarketplaceError> {
    let skill_meta = read_skill_frontmatter(&options.package_dir)?;
    let requirements = requirements_from_options(&options);
    let name = required_value(
        "name",
        options
            .name
            .or_else(|| string_at(&skill_meta, &["name"]))
            .filter(|value| !value.trim().is_empty()),
    )?;
    let description = required_value(
        "description",
        options
            .description
            .or_else(|| string_at(&skill_meta, &["description"]))
            .filter(|value| !value.trim().is_empty()),
    )?;
    let mut dcc = options.dcc;
    if dcc.is_empty() {
        dcc = list_or_csv_at(&skill_meta, &["metadata", "dcc-mcp", "dcc"]);
    }
    let tags = merge_unique(
        list_or_csv_at(&skill_meta, &["metadata", "dcc-mcp", "tags"]),
        options.tags,
    );

    let entry = CatalogEntry {
        name: path_component("package name", &name)?,
        description,
        dcc,
        url: options.homepage_url,
        tags,
        version: options
            .version
            .or_else(|| string_at(&skill_meta, &["metadata", "dcc-mcp", "version"])),
        min_core_version: options.min_core_version,
        install: Some(CatalogInstall {
            install_type: options.install_type,
            url: Some(options.install_url),
            ref_: options.install_ref,
            sha256: options.sha256,
            skill_roots: (!options.skill_roots.is_empty()).then_some(options.skill_roots),
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        }),
        maintainer: options.maintainer.or_else(|| {
            string_at(&skill_meta, &["metadata", "dcc-mcp", "maintainer"])
                .or_else(|| string_at(&skill_meta, &["maintainer"]))
        }),
        category: Some("Skills".into()),
        policy: Some(CatalogPolicy {
            installation: "available".into(),
        }),
        requires: requirements,
        icon: options.icon,
        showcase: options
            .showcase
            .or_else(|| string_at(&skill_meta, &["metadata", "dcc-mcp", "showcase"])),
    };
    dcc_mcp_catalog::validate_entry(&entry)?;

    let (action, count) = upsert_catalog_entry(&options.catalog_path, entry.clone())?;
    Ok(MarketplacePublishResult {
        catalog_path: options.catalog_path.display().to_string(),
        entry,
        action,
        count,
    })
}

fn resolve_pack_out_path(out: Option<PathBuf>, source_dir: &Path, package_name: &str) -> PathBuf {
    match out {
        Some(path) if path.extension().is_some() => path,
        Some(dir) => dir.join(format!("{package_name}.zip")),
        None => source_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{package_name}.zip")),
    }
}

fn build_zip_bytes(source_dir: &Path, out_path: &Path) -> Result<Vec<u8>, MarketplaceError> {
    let mut files = Vec::new();
    collect_pack_files(source_dir, source_dir, out_path, &mut files)?;
    if files.is_empty() {
        return Err(MarketplaceError::CommandFailed(format!(
            "no package files found under '{}'",
            source_dir.display()
        )));
    }
    files.sort();

    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for rel in files {
        let full_path = source_dir.join(&rel);
        let rel_name = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(rel_name, options)
            .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;
        let bytes = fs::read(&full_path)
            .map_err(|err| MarketplaceError::ConfigIo(full_path.display().to_string(), err))?;
        zip.write_all(&bytes)
            .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;
    }
    zip.finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))
}

fn collect_pack_files(
    root: &Path,
    dir: &Path,
    out_path: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), MarketplaceError> {
    for entry in fs::read_dir(dir)
        .map_err(|err| MarketplaceError::ConfigIo(dir.display().to_string(), err))?
    {
        let entry =
            entry.map_err(|err| MarketplaceError::ConfigIo(dir.display().to_string(), err))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if should_skip_path(&path, &file_name, out_path) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
        if file_type.is_dir() {
            collect_pack_files(root, &path, out_path, files)?;
        } else if file_type.is_file() {
            files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

fn should_skip_path(path: &Path, name: &str, out_path: &Path) -> bool {
    path == out_path
        || matches!(
            name,
            ".git"
                | ".hg"
                | ".svn"
                | "target"
                | "node_modules"
                | "__pycache__"
                | ".pytest_cache"
                | ".mypy_cache"
                | ".ruff_cache"
                | ".venv"
                | "venv"
        )
}

fn upsert_catalog_entry(
    path: &Path,
    entry: CatalogEntry,
) -> Result<(String, usize), MarketplaceError> {
    let mut raw = load_catalog_value(path)?;
    if raw.get("skills").is_some() || !path.exists() {
        let skills = raw
            .get_mut("skills")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                MarketplaceError::ConfigParse(
                    path.display().to_string(),
                    serde_json::Error::io(std::io::Error::other(
                        "marketplace v1 catalog must contain a skills array",
                    )),
                )
            })?;
        let entry_value = marketplace_v1_entry_value(&entry)?;
        let action = upsert_entry_value(skills, entry_value, &entry.name)?;
        let count = skills.len();
        save_catalog_value(path, &raw)?;
        return Ok((action, count));
    }

    let mut catalog: CatalogDoc = serde_json::from_value(raw)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))?;
    let action = upsert_entry(&mut catalog.entries, entry);
    let count = catalog.entries.len();
    save_catalog_doc(path, &catalog)?;
    Ok((action, count))
}

fn load_catalog_value(path: &Path) -> Result<Value, MarketplaceError> {
    if !path.exists() {
        return Ok(json!({
            "name": "dcc-mcp-local",
            "schemaVersion": "1",
            "version": "1.0.0",
            "skills": []
        }));
    }
    let text = fs::read_to_string(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
}

fn save_catalog_value(path: &Path, catalog: &Value) -> Result<(), MarketplaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }
    let text = serde_json::to_string_pretty(catalog)
        .expect("marketplace catalog serialization should not fail");
    fs::write(path, format!("{text}\n"))
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

fn marketplace_v1_entry_value(entry: &CatalogEntry) -> Result<Value, MarketplaceError> {
    let version = required_entry_value(entry, "version", entry.version.as_deref())?;
    if entry.dcc.is_empty() {
        return Err(MarketplaceError::CommandFailed(format!(
            "marketplace v1 entry '{}' requires at least one DCC target",
            entry.name
        )));
    }
    if entry.tags.is_empty() {
        return Err(MarketplaceError::CommandFailed(format!(
            "marketplace v1 entry '{}' requires at least one tag",
            entry.name
        )));
    }
    let install = entry
        .install
        .as_ref()
        .ok_or_else(|| MarketplaceError::MissingInstall(entry.name.clone()))?;
    if install.url.as_deref().is_none_or(str::is_empty) {
        return Err(MarketplaceError::MissingInstall("source.url".into()));
    }
    if install.install_type == "git" && install.ref_.as_deref().is_none_or(str::is_empty) {
        return Err(MarketplaceError::MissingInstall("source.ref".into()));
    }
    if install.install_type == "zip" && install.sha256.as_deref().is_none_or(str::is_empty) {
        return Err(MarketplaceError::MissingInstall("source.sha256".into()));
    }

    let mut value = json!({
        "name": entry.name,
        "description": entry.description,
        "version": version,
        "dcc": entry.dcc,
        "tags": entry.tags,
        "category": entry.category.as_deref().unwrap_or("Skills"),
        "source": install,
        "policy": entry.policy.as_ref().unwrap_or(&CatalogPolicy {
            installation: "available".into()
        })
    });
    let object = value
        .as_object_mut()
        .expect("json object literal should remain an object");
    if let Some(requires) = entry.requires.as_ref() {
        object.insert("requires".into(), serde_json::to_value(requires).unwrap());
    }
    if let Some(min_core_version) = entry.min_core_version.as_ref() {
        object.insert(
            "minCoreVersion".into(),
            Value::String(min_core_version.clone()),
        );
    }
    if let Some(maintainer) = entry.maintainer.as_ref() {
        object.insert("maintainer".into(), Value::String(maintainer.clone()));
    }
    if let Some(docs) = entry.url.as_ref() {
        object.insert("docs".into(), Value::String(docs.clone()));
    }
    if let Some(icon) = entry.icon.as_ref() {
        object.insert("icon".into(), Value::String(icon.clone()));
    }
    if let Some(showcase) = entry.showcase.as_ref() {
        object.insert("showcase".into(), Value::String(showcase.clone()));
    }
    Ok(value)
}

fn required_entry_value<'a>(
    entry: &CatalogEntry,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str, MarketplaceError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            MarketplaceError::CommandFailed(format!(
                "marketplace v1 entry '{}' requires {field}",
                entry.name
            ))
        })
}

fn upsert_entry_value(
    entries: &mut Vec<Value>,
    mut entry: Value,
    name: &str,
) -> Result<String, MarketplaceError> {
    for existing in entries.iter_mut() {
        if existing.get("name").and_then(Value::as_str) == Some(name) {
            preserve_marketplace_v1_metadata(&mut entry, existing);
            validate_marketplace_v1_entry(&entry)?;
            *existing = entry;
            return Ok("updated".to_string());
        }
    }
    validate_marketplace_v1_entry(&entry)?;
    entries.push(entry);
    Ok("created".to_string())
}

fn preserve_marketplace_v1_metadata(entry: &mut Value, existing: &Value) {
    let Some(entry_object) = entry.as_object_mut() else {
        return;
    };
    let Some(existing_object) = existing.as_object() else {
        return;
    };
    preserve_requirement_fields(entry_object, existing_object);
    for key in [
        "minCoreVersion",
        "maintainer",
        "category",
        "policy",
        "requires",
        "docs",
        "icon",
        "showcase",
        "license",
        "lifecycle",
        "replacedBy",
    ] {
        if !entry_object.contains_key(key)
            && let Some(value) = existing_object.get(key)
        {
            entry_object.insert(key.to_string(), value.clone());
        }
    }
}

fn validate_marketplace_v1_entry(entry: &Value) -> Result<(), MarketplaceError> {
    for key in [
        "name",
        "description",
        "version",
        "maintainer",
        "minCoreVersion",
        "source",
        "policy",
    ] {
        if entry.get(key).is_none() {
            return Err(MarketplaceError::CommandFailed(format!(
                "marketplace v1 entry is missing required field '{key}'"
            )));
        }
    }
    if entry
        .get("dcc")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(MarketplaceError::CommandFailed(
            "marketplace v1 entry requires at least one DCC target".into(),
        ));
    }
    if entry
        .get("tags")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(MarketplaceError::CommandFailed(
            "marketplace v1 entry requires at least one tag".into(),
        ));
    }
    Ok(())
}

fn save_catalog_doc(path: &Path, catalog: &CatalogDoc) -> Result<(), MarketplaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }
    let text =
        serde_json::to_string_pretty(catalog).expect("CatalogDoc serialization should not fail");
    fs::write(path, format!("{text}\n"))
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

fn upsert_entry(entries: &mut Vec<CatalogEntry>, entry: CatalogEntry) -> String {
    let mut entry = entry;
    for existing in entries.iter_mut() {
        if existing.name == entry.name {
            preserve_requirements(&mut entry.requires, existing.requires.as_ref());
            if entry.showcase.is_none() {
                entry.showcase.clone_from(&existing.showcase);
            }
            *existing = entry;
            return "updated".to_string();
        }
    }
    entries.push(entry);
    "created".to_string()
}

fn read_skill_frontmatter(dir: &Path) -> Result<Value, MarketplaceError> {
    let path = dir.join("SKILL.md");
    if !path.is_file() {
        return Ok(Value::Object(Default::default()));
    }
    let text = fs::read_to_string(&path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    let Some(rest) = text.strip_prefix("---") else {
        return Ok(Value::Object(Default::default()));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(MarketplaceError::CommandFailed(format!(
            "SKILL.md frontmatter is not closed in '{}'",
            path.display()
        )));
    };
    serde_yaml_ng::from_str(rest[..end].trim()).map_err(|err| {
        MarketplaceError::CommandFailed(format!(
            "failed to parse SKILL.md frontmatter in '{}': {err}",
            path.display()
        ))
    })
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn list_or_csv_at(value: &Value, path: &[&str]) -> Vec<String> {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return Vec::new();
        };
        current = next;
    }
    if let Some(items) = current.as_array() {
        return items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect();
    }
    current
        .as_str()
        .map(|value| split_csv(value).collect())
        .unwrap_or_default()
}

fn split_csv(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn merge_unique(first: Vec<String>, second: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in first.into_iter().chain(second) {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

fn requirements_from_options(options: &MarketplacePublishOptions) -> Option<CatalogRequirements> {
    let requirements = CatalogRequirements {
        env: merge_unique(Vec::new(), options.requires_env.clone()),
        bins: merge_unique(Vec::new(), options.requires_bin.clone()),
        python: merge_unique(Vec::new(), options.requires_python.clone()),
        skills: merge_unique(Vec::new(), options.requires_skill.clone()),
    };
    (!requirements.env.is_empty()
        || !requirements.bins.is_empty()
        || !requirements.python.is_empty()
        || !requirements.skills.is_empty())
    .then_some(requirements)
}

fn preserve_requirements(
    incoming: &mut Option<CatalogRequirements>,
    existing: Option<&CatalogRequirements>,
) {
    let Some(existing) = existing else {
        return;
    };
    let Some(incoming) = incoming.as_mut() else {
        *incoming = Some(existing.clone());
        return;
    };
    if incoming.env.is_empty() {
        incoming.env.clone_from(&existing.env);
    }
    if incoming.bins.is_empty() {
        incoming.bins.clone_from(&existing.bins);
    }
    if incoming.python.is_empty() {
        incoming.python.clone_from(&existing.python);
    }
    if incoming.skills.is_empty() {
        incoming.skills.clone_from(&existing.skills);
    }
}

fn preserve_requirement_fields(
    incoming: &mut serde_json::Map<String, Value>,
    existing: &serde_json::Map<String, Value>,
) {
    let Some(existing_requires) = existing.get("requires") else {
        return;
    };
    let Some(incoming_requires) = incoming.get_mut("requires") else {
        incoming.insert("requires".into(), existing_requires.clone());
        return;
    };
    let (Some(incoming_requires), Some(existing_requires)) = (
        incoming_requires.as_object_mut(),
        existing_requires.as_object(),
    ) else {
        return;
    };
    for key in ["env", "bins", "python", "skills"] {
        if !incoming_requires.contains_key(key)
            && let Some(value) = existing_requires.get(key)
        {
            incoming_requires.insert(key.into(), value.clone());
        }
    }
}

fn required_value(kind: &str, value: Option<String>) -> Result<String, MarketplaceError> {
    value.ok_or_else(|| {
        MarketplaceError::CommandFailed(format!(
            "marketplace publish requires {kind}; pass --{kind} or add it to SKILL.md"
        ))
    })
}

fn default_catalog_version() -> String {
    "1".to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_skips_vcs_and_writes_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("my-skill");
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Test\n---\n",
        )
        .unwrap();
        fs::write(src.join(".git/config"), "secret").unwrap();

        let result = pack_marketplace_package(MarketplacePackOptions {
            source_dir: src.clone(),
            out: Some(tmp.path().join("dist")),
        })
        .unwrap();

        assert!(Path::new(&result.package_path).is_file());
        assert_eq!(result.sha256.len(), 64);
        let bytes = fs::read(&result.package_path).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("SKILL.md").is_ok());
        assert!(archive.by_name(".git/config").is_err());
    }

    #[test]
    fn publish_upserts_catalog_entry_from_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("my-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Test skill\nmetadata:\n  dcc-mcp:\n    dcc: maya, blender\n    version: 0.1.0\n    tags: modeling, test\n    showcase: docs/images/showcase.webp\n---\n",
        )
        .unwrap();
        let catalog_path = tmp.path().join("marketplace.json");

        let result = publish_marketplace_package(MarketplacePublishOptions {
            package_dir: src,
            catalog_path: catalog_path.clone(),
            install_url: "https://example.com/my-skill.zip".into(),
            install_type: "zip".into(),
            install_ref: None,
            skill_roots: vec!["skill/my-skill".into()],
            sha256: Some(format!("sha256:{}", "a".repeat(64))),
            name: None,
            description: None,
            dcc: Vec::new(),
            version: None,
            maintainer: Some("dcc-mcp".into()),
            tags: vec!["extra".into()],
            min_core_version: Some("0.19.0".into()),
            homepage_url: None,
            icon: None,
            showcase: None,
            requires_env: vec!["MY_SKILL_TOKEN".into(), "MY_SKILL_TOKEN".into()],
            requires_bin: vec!["my-skill-cli".into()],
            requires_python: vec!["my_skill".into()],
            requires_skill: vec!["dcc-base".into()],
        })
        .unwrap();

        assert_eq!(result.action, "created");
        assert_eq!(result.entry.name, "my-skill");
        assert_eq!(result.entry.dcc, vec!["maya", "blender"]);
        assert_eq!(result.entry.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            result.entry.showcase.as_deref(),
            Some("docs/images/showcase.webp")
        );
        let requires = result.entry.requires.as_ref().unwrap();
        assert_eq!(requires.env, vec!["MY_SKILL_TOKEN"]);
        assert_eq!(requires.bins, vec!["my-skill-cli"]);
        assert_eq!(requires.python, vec!["my_skill"]);
        assert_eq!(requires.skills, vec!["dcc-base"]);
        let text = fs::read_to_string(catalog_path).unwrap();
        assert!(text.contains("\"schemaVersion\": \"1\""));
        assert!(text.contains("\"skills\": ["));
        assert!(text.contains("\"minCoreVersion\": \"0.19.0\""));
        assert!(text.contains("\"source\": {"));
        assert!(text.contains("\"skillRoots\": ["));
        assert!(text.contains("\"showcase\": \"docs/images/showcase.webp\""));
        assert!(text.contains("\"MY_SKILL_TOKEN\""));
    }

    #[test]
    fn typed_catalog_update_preserves_unset_requirements_and_showcase() {
        let existing = serde_json::from_value(json!({
            "name": "my-skill",
            "description": "Existing",
            "requires": {
                "env": ["OLD_TOKEN"],
                "bins": ["existing-cli"],
                "python": ["existing_module"],
                "skills": ["dcc-base"]
            },
            "showcase": "docs/images/existing.webp"
        }))
        .unwrap();
        let incoming = serde_json::from_value(json!({
            "name": "my-skill",
            "description": "Updated",
            "requires": { "env": ["NEW_TOKEN"] }
        }))
        .unwrap();
        let mut entries = vec![existing];

        assert_eq!(upsert_entry(&mut entries, incoming), "updated");
        let entry = &entries[0];
        let requires = entry.requires.as_ref().unwrap();
        assert_eq!(requires.env, vec!["NEW_TOKEN"]);
        assert_eq!(requires.bins, vec!["existing-cli"]);
        assert_eq!(requires.python, vec!["existing_module"]);
        assert_eq!(requires.skills, vec!["dcc-base"]);
        assert_eq!(entry.showcase.as_deref(), Some("docs/images/existing.webp"));
    }
}
