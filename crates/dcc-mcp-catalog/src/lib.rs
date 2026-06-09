//! Public DCC-MCP catalog for ecosystem discovery.
//!
//! Provides [`CatalogEntry`] (a typed YAML/JSON record), and two discovery
//! functions — [`search`] and [`describe`] — that can be wired up as
//! gateway MCP tools (`dcc_catalog__search` / `dcc_catalog__describe`).
//!
//! # YAML format (`dcc-mcp-catalog.yml`)
//!
//! ```yaml
//! version: "1"
//! entries:
//!   - name: "dcc-mcp-maya-skills"
//!     description: "Maya skill pack for DCC-MCP"
//!     dcc: ["maya"]
//!     url: "https://github.com/loonghao/dcc-mcp-maya-skills"
//!     tags: ["skills", "maya", "official"]
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

mod error;
pub use error::{CatalogError, CatalogValidationError};

// ── types ─────────────────────────────────────────────────────────────────────

/// A single entry in the public DCC-MCP catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogEntry {
    /// Unique package / adapter name (e.g. `"dcc-mcp-maya-skills"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// DCC application(s) this entry targets (e.g. `["maya", "blender"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dcc: Vec<String>,
    /// Canonical URL (GitHub repo, docs site, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Searchable tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Package version advertised by a marketplace catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Minimum dcc-mcp-core version required by this package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_core_version: Option<String>,
    /// Installation metadata for CLI-driven marketplace installs.
    ///
    /// Also deserializes from `source` (marketplace.json format) so that
    /// the same `CatalogEntry` struct can ingest both catalog.yml (`install`)
    /// and marketplace.json (`source`) install metadata.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "source")]
    pub install: Option<CatalogInstall>,
    /// Maintainer or publishing organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,
    /// Icon path or URL (e.g. `"icon.png"` for repo-relative, or an absolute URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Installation metadata for a marketplace catalog entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogInstall {
    /// Install source type (`git`, `zip`, `path`, `pip`, ...).
    #[serde(rename = "type")]
    pub install_type: String,
    /// Source URL or local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Git ref, tag, branch, or revision where applicable.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    /// Optional content hash for archive installs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Pip package name (required when type is `pip`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pip_package: Option<String>,
    /// Optional pip extras (e.g. `["maya", "blender"]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pip_extras: Option<Vec<String>>,
    /// Maya Python interpreter path for per-DCC installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mayapy_path: Option<String>,
    /// Python entry point for running the adapter (e.g. `dcc_mcp_maya.cli:main`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
}

/// Top-level catalog document.
#[derive(Debug, Deserialize)]
struct CatalogDoc {
    #[allow(dead_code)]
    #[serde(default)]
    version: Option<String>,
    #[serde(default, alias = "items", alias = "skills")]
    entries: Vec<CatalogEntry>,
}

// ── loading ───────────────────────────────────────────────────────────────────

/// Load catalog entries from a YAML file on disk.
///
/// Returns an empty `Vec` if the file does not exist (so callers that embed a
/// bundled catalog path don't hard-fail when the file is absent in tests or
/// minimal installs).
pub fn load_from_file(path: impl AsRef<Path>) -> Result<Vec<CatalogEntry>, CatalogError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| CatalogError::Io(path.display().to_string(), e))?;
    load_from_str(&text)
}

/// Parse catalog entries from a JSON or YAML string.
///
/// Tries JSON first (marketplace repo uses `marketplace.json`), then falls
/// back to YAML for backward compatibility with `dcc-mcp-catalog.yml`.
pub fn load_from_str(text: &str) -> Result<Vec<CatalogEntry>, CatalogError> {
    let trimmed = text.trim();
    // JSON detection: starts with `{` or `[` and doesn't start with `---`.
    let looks_like_json = trimmed.starts_with('{') || trimmed.starts_with('[');
    if looks_like_json && let Ok(doc) = serde_json::from_str::<CatalogDoc>(trimmed) {
        return Ok(doc.entries);
    }
    let doc: CatalogDoc =
        serde_yaml_ng::from_str(text).map_err(|e| CatalogError::Parse(e.to_string()))?;
    Ok(doc.entries)
}

// ── search / describe ─────────────────────────────────────────────────────────

/// Search catalog entries.
///
/// `query` is matched case-insensitively against `name`, `description`,
/// `dcc`, `tags`, version/maintainer metadata, and install URL.  An empty query
/// returns all entries.
pub fn search(entries: &[CatalogEntry], query: &str) -> Vec<CatalogEntry> {
    if query.is_empty() {
        return entries.to_vec();
    }
    let q = query.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.description.to_lowercase().contains(&q)
                || e.dcc.iter().any(|d| d.to_lowercase().contains(&q))
                || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || e.version
                    .as_deref()
                    .is_some_and(|version| version.to_lowercase().contains(&q))
                || e.maintainer
                    .as_deref()
                    .is_some_and(|maintainer| maintainer.to_lowercase().contains(&q))
                || e.install
                    .as_ref()
                    .and_then(|install| install.url.as_deref())
                    .is_some_and(|url| url.to_lowercase().contains(&q))
        })
        .cloned()
        .collect()
}

/// Look up a single entry by exact name.
pub fn describe(entries: &[CatalogEntry], name: &str) -> Option<CatalogEntry> {
    entries.iter().find(|e| e.name == name).cloned()
}

// ── schema validation ─────────────────────────────────────────────────────────

/// JSON Schema (Draft 2020-12) for marketplace-v1 catalog entries.
///
/// Each entry must declare at least `name` and `description`; all other
/// fields are optional.  `additionalProperties: false` on both the top-level
/// document and each entry catches typos early.
const MARKETPLACE_V1_SCHEMA_JSON: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://dcc-mcp.github.io/schemas/marketplace-v1.schema.json",
  "title": "DCC-MCP Marketplace Catalog",
  "description": "Schema for marketplace.json catalog entries",
  "type": "object",
  "required": ["entries"],
  "properties": {
    "version": { "type": "string" },
    "entries": {
      "type": "array",
      "items": { "$ref": "#/$defs/entry" }
    }
  },
  "additionalProperties": false,
  "$defs": {
    "entry": {
      "type": "object",
      "required": ["name", "description"],
      "properties": {
        "name":        { "type": "string", "minLength": 1 },
        "description": { "type": "string", "minLength": 1 },
        "dcc":         { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
        "url":         { "type": "string" },
        "tags":        { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
        "version":          { "type": "string" },
        "min_core_version": { "type": "string" },
        "maintainer":       { "type": "string" },
        "icon":        { "type": "string" },
        "install": {
          "type": "object",
          "required": ["type"],
          "properties": {
            "type":        { "type": "string", "enum": ["git", "zip", "path", "pip"] },
            "url":         { "type": "string" },
            "ref":         { "type": "string" },
            "sha256":      { "type": "string" },
            "pip_package": { "type": "string" },
            "pip_extras":  { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "mayapy_path": { "type": "string" },
            "entry_point": { "type": "string" }
          },
          "additionalProperties": false
        }
      },
      "additionalProperties": false
    }
  }
}"##;

/// Validate a single [`CatalogEntry`] against the marketplace-v1 JSON Schema.
///
/// Returns `Ok(())` if the entry is valid, or a
/// [`CatalogValidationError::ValidationFailed`] with a human-readable message
/// describing what failed.
pub fn validate_entry(entry: &CatalogEntry) -> Result<(), CatalogValidationError> {
    let value = serde_json::to_value(entry).map_err(|e| {
        CatalogValidationError::SchemaError(format!(
            "failed to serialize entry '{}' for validation: {e}",
            entry.name
        ))
    })?;

    let schema = entry_schema()?;
    let validation = schema.validate(&value);
    if let Err(err) = validation {
        return Err(CatalogValidationError::ValidationFailed {
            name: entry.name.clone(),
            message: format!("  - {}: {}", err.instance_path, err),
        });
    }
    Ok(())
}

/// Validate a slice of [`CatalogEntry`] against the marketplace-v1 JSON Schema.
///
/// Returns `Ok(())` if all entries pass, or
/// [`CatalogValidationError::MultipleFailures`] aggregating each failed entry.
pub fn validate_catalog_entries(entries: &[CatalogEntry]) -> Result<(), CatalogValidationError> {
    let mut failures = Vec::new();
    for entry in entries {
        if let Err(err) = validate_entry(entry) {
            failures.push(err);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        let count = failures.len();
        Err(CatalogValidationError::MultipleFailures { count, failures })
    }
}

/// Compile the entry sub-schema once from `$defs/entry`.
fn entry_schema() -> Result<jsonschema::Validator, CatalogValidationError> {
    let schema_value: serde_json::Value = serde_json::from_str(MARKETPLACE_V1_SCHEMA_JSON)
        .map_err(|e| {
            CatalogValidationError::SchemaError(format!("invalid embedded schema: {e}"))
        })?;
    let entry_schema_value = schema_value
        .pointer("/$defs/entry")
        .cloned()
        .ok_or_else(|| {
            CatalogValidationError::SchemaError("missing $defs/entry in schema".into())
        })?;
    jsonschema::validator_for(&entry_schema_value)
        .map_err(|e| CatalogValidationError::SchemaError(format!("failed to compile schema: {e}")))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"
version: "1"
entries:
  - name: "dcc-mcp-maya-skills"
    description: "Maya skill pack for DCC-MCP"
    dcc: ["maya"]
    url: "https://github.com/loonghao/dcc-mcp-maya-skills"
    tags: ["skills", "maya", "official"]
  - name: "dcc-mcp-blender-skills"
    description: "Blender skill pack for DCC-MCP"
    dcc: ["blender"]
    url: "https://github.com/loonghao/dcc-mcp-blender-skills"
    tags: ["skills", "blender", "official"]
  - name: "dcc-mcp-houdini-vfx"
    description: "Houdini VFX tools"
    dcc: ["houdini"]
    tags: ["vfx", "houdini"]
"#;

    fn sample_entries() -> Vec<CatalogEntry> {
        load_from_str(SAMPLE_YAML).unwrap()
    }

    #[test]
    fn test_load_from_str() {
        let entries = sample_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "dcc-mcp-maya-skills");
    }

    #[test]
    fn test_search_by_dcc_type() {
        let entries = sample_entries();
        let results = search(&entries, "maya");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "dcc-mcp-maya-skills");
    }

    #[test]
    fn test_search_by_tag() {
        let entries = sample_entries();
        let results = search(&entries, "official");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_empty_returns_all() {
        let entries = sample_entries();
        let results = search(&entries, "");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_case_insensitive() {
        let entries = sample_entries();
        let results = search(&entries, "MAYA");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_describe_found() {
        let entries = sample_entries();
        let entry = describe(&entries, "dcc-mcp-blender-skills").unwrap();
        assert_eq!(entry.dcc, vec!["blender"]);
    }

    #[test]
    fn test_describe_not_found() {
        let entries = sample_entries();
        assert!(describe(&entries, "does-not-exist").is_none());
    }

    #[test]
    fn test_load_from_file_missing() {
        let entries = load_from_file("/nonexistent/path/catalog.yml").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_load_from_file_exists() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(SAMPLE_YAML.as_bytes()).unwrap();
        let entries = load_from_file(f.path()).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_load_marketplace_json_with_install_metadata() {
        let json = r#"
{
  "version": "1",
  "entries": [{
    "name": "dcc-asset-hunyuan-download",
    "description": "Search and download Hunyuan 3D models",
    "dcc": ["maya", "blender"],
    "tags": ["asset", "hunyuan", "download"],
    "version": "0.1.0",
    "min_core_version": "0.17.0",
    "maintainer": "dcc-mcp",
    "install": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/dcc-asset-hunyuan-download",
      "ref": "v0.1.0"
    }
  }]
}
"#;

        let entries = load_from_str(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version.as_deref(), Some("0.1.0"));
        let install = entries[0].install.as_ref().unwrap();
        assert_eq!(install.install_type, "git");
        assert_eq!(install.ref_.as_deref(), Some("v0.1.0"));
    }

    // -- schema validation tests ------------------------------------------------

    fn make_entry(name: &str, description: &str) -> CatalogEntry {
        CatalogEntry {
            name: name.into(),
            description: description.into(),
            dcc: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
            icon: None,
        }
    }

    #[test]
    fn test_validate_entry_minimal_valid() {
        let entry = make_entry("my-skill", "A useful skill");
        assert!(validate_entry(&entry).is_ok());
    }

    #[test]
    fn test_validate_entry_empty_name_fails() {
        let entry = make_entry("", "A useful skill");
        let err = validate_entry(&entry).unwrap_err();
        assert!(
            matches!(err, CatalogValidationError::ValidationFailed { .. }),
            "expected ValidationFailed, got {err}"
        );
    }

    #[test]
    fn test_validate_entry_empty_description_fails() {
        let entry = make_entry("my-skill", "");
        let err = validate_entry(&entry).unwrap_err();
        assert!(
            matches!(err, CatalogValidationError::ValidationFailed { .. }),
            "expected ValidationFailed, got {err}"
        );
    }

    #[test]
    fn test_validate_entry_with_full_install_metadata_passes() {
        let entry = CatalogEntry {
            name: "zip-skill".into(),
            description: "A zip-installed skill".into(),
            dcc: vec!["maya".into()],
            url: Some("https://example.com/skill".into()),
            tags: vec!["test".into()],
            version: Some("0.1.0".into()),
            min_core_version: Some("0.17.0".into()),
            maintainer: Some("dcc-mcp".into()),
            install: Some(CatalogInstall {
                install_type: "zip".into(),
                url: Some("https://example.com/skill.zip".into()),
                ref_: Some("v0.1.0".into()),
                sha256: Some("abc123".into()),
                pip_package: None,
                pip_extras: None,
                mayapy_path: None,
                entry_point: None,
            }),
            icon: None,
        };
        assert!(validate_entry(&entry).is_ok());
    }

    #[test]
    fn test_validate_entry_with_pip_install_passes() {
        let entry = CatalogEntry {
            name: "pip-skill".into(),
            description: "A pip-installed skill".into(),
            dcc: vec!["maya".into()],
            url: Some("https://pypi.org/project/dcc-mcp-maya".into()),
            tags: vec!["pip".into(), "maya".into()],
            version: Some("0.3.0".into()),
            min_core_version: Some("0.18.0".into()),
            maintainer: Some("dcc-mcp".into()),
            install: Some(CatalogInstall {
                install_type: "pip".into(),
                url: None,
                ref_: None,
                sha256: None,
                pip_package: Some("dcc-mcp-maya".into()),
                pip_extras: Some(vec!["maya".into()]),
                mayapy_path: Some("/usr/autodesk/maya2026/bin/mayapy".into()),
                entry_point: Some("dcc_mcp_maya.cli:main".into()),
            }),
            icon: None,
        };
        assert!(validate_entry(&entry).is_ok());
    }

    #[test]
    fn test_load_marketplace_json_with_pip_install() {
        let json = r#"
{
  "version": "1",
  "entries": [{
    "name": "dcc-mcp-maya-pip",
    "description": "Maya adapter installed via pip",
    "dcc": ["maya"],
    "tags": ["adapter", "maya", "pip"],
    "version": "0.3.0",
    "min_core_version": "0.18.0",
    "maintainer": "dcc-mcp",
    "install": {
      "type": "pip",
      "pip_package": "dcc-mcp-maya",
      "pip_extras": ["maya"],
      "mayapy_path": "/usr/autodesk/maya2026/bin/mayapy",
      "entry_point": "dcc_mcp_maya.cli:main"
    }
  }]
}
"#;
        let entries = load_from_str(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version.as_deref(), Some("0.3.0"));
        let install = entries[0].install.as_ref().unwrap();
        assert_eq!(install.install_type, "pip");
        assert_eq!(install.pip_package.as_deref(), Some("dcc-mcp-maya"));
        assert_eq!(
            install.pip_extras.as_deref(),
            Some(vec!["maya".to_string()].as_slice())
        );
        assert_eq!(
            install.mayapy_path.as_deref(),
            Some("/usr/autodesk/maya2026/bin/mayapy")
        );
        assert_eq!(
            install.entry_point.as_deref(),
            Some("dcc_mcp_maya.cli:main")
        );
    }

    #[test]
    fn test_validate_entry_with_minimal_pip_install_passes() {
        let entry = CatalogEntry {
            name: "minimal-pip".into(),
            description: "A minimal pip-installed adapter".into(),
            dcc: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            maintainer: None,
            install: Some(CatalogInstall {
                install_type: "pip".into(),
                url: None,
                ref_: None,
                sha256: None,
                pip_package: Some("dcc-mcp-core".into()),
                pip_extras: None,
                mayapy_path: None,
                entry_point: None,
            }),
            icon: None,
        };
        assert!(validate_entry(&entry).is_ok());
    }

    #[test]
    fn test_validate_entry_with_icon_passes() {
        let entry = CatalogEntry {
            name: "icon-skill".into(),
            description: "A skill with an icon".into(),
            dcc: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
            icon: Some("icon.png".into()),
        };
        assert!(validate_entry(&entry).is_ok());
    }

    #[test]
    fn test_validate_catalog_entries_all_valid() {
        let entries = vec![
            make_entry("skill-a", "First skill"),
            make_entry("skill-b", "Second skill"),
        ];
        assert!(validate_catalog_entries(&entries).is_ok());
    }

    #[test]
    fn test_validate_catalog_entries_mixed() {
        let entries = vec![
            make_entry("valid-skill", "A valid skill"),
            make_entry("", "Missing name"),
            make_entry("valid-2", ""),
        ];
        let err = validate_catalog_entries(&entries).unwrap_err();
        match err {
            CatalogValidationError::MultipleFailures { count, failures } => {
                assert_eq!(count, 2);
                assert_eq!(failures.len(), 2);
            }
            other => panic!("expected MultipleFailures, got {other}"),
        }
    }
}
