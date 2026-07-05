use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        icon: options.icon,
    };
    dcc_mcp_catalog::validate_entry(&entry)?;

    let mut catalog = load_catalog_doc(&options.catalog_path)?;
    let action = upsert_entry(&mut catalog.entries, entry.clone());
    save_catalog_doc(&options.catalog_path, &catalog)?;
    Ok(MarketplacePublishResult {
        catalog_path: options.catalog_path.display().to_string(),
        entry,
        action,
        count: catalog.entries.len(),
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

fn load_catalog_doc(path: &Path) -> Result<CatalogDoc, MarketplaceError> {
    if !path.exists() {
        return Ok(CatalogDoc {
            version: default_catalog_version(),
            entries: Vec::new(),
        });
    }
    let text = fs::read_to_string(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
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
    for existing in entries.iter_mut() {
        if existing.name == entry.name {
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
            "---\nname: my-skill\ndescription: Test skill\nmetadata:\n  dcc-mcp:\n    dcc: maya, blender\n    version: 0.1.0\n    tags: modeling, test\n---\n",
        )
        .unwrap();
        let catalog_path = tmp.path().join("marketplace.json");

        let result = publish_marketplace_package(MarketplacePublishOptions {
            package_dir: src,
            catalog_path: catalog_path.clone(),
            install_url: "https://example.com/my-skill.zip".into(),
            install_type: "zip".into(),
            install_ref: None,
            sha256: Some("sha256:abc".into()),
            name: None,
            description: None,
            dcc: Vec::new(),
            version: None,
            maintainer: Some("dcc-mcp".into()),
            tags: vec!["extra".into()],
            min_core_version: None,
            homepage_url: None,
            icon: None,
        })
        .unwrap();

        assert_eq!(result.action, "created");
        assert_eq!(result.entry.name, "my-skill");
        assert_eq!(result.entry.dcc, vec!["maya", "blender"]);
        assert_eq!(result.entry.version.as_deref(), Some("0.1.0"));
        let text = fs::read_to_string(catalog_path).unwrap();
        assert!(text.contains("\"sha256\": \"sha256:abc\""));
    }
}
