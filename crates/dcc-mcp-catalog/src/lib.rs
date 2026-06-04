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
pub use error::CatalogError;

// ── types ─────────────────────────────────────────────────────────────────────

/// A single entry in the public DCC-MCP catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogEntry {
    /// Unique package / adapter name (e.g. `"dcc-mcp-maya-skills"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// DCC application(s) this entry targets (e.g. `["maya", "blender"]`).
    #[serde(default)]
    pub dcc: Vec<String>,
    /// Canonical URL (GitHub repo, docs site, …).
    #[serde(default)]
    pub url: Option<String>,
    /// Searchable tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Package version advertised by a marketplace catalog.
    #[serde(default)]
    pub version: Option<String>,
    /// Minimum dcc-mcp-core version required by this package.
    #[serde(default)]
    pub min_core_version: Option<String>,
    /// Installation metadata for CLI-driven marketplace installs.
    #[serde(default)]
    pub install: Option<CatalogInstall>,
    /// Maintainer or publishing organization.
    #[serde(default)]
    pub maintainer: Option<String>,
}

/// Installation metadata for a marketplace catalog entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogInstall {
    /// Install source type (`git`, `zip`, `path`, ...).
    #[serde(rename = "type")]
    pub install_type: String,
    /// Source URL or local path.
    #[serde(default)]
    pub url: Option<String>,
    /// Git ref, tag, branch, or revision where applicable.
    #[serde(default, rename = "ref")]
    pub ref_: Option<String>,
    /// Optional content hash for archive installs.
    #[serde(default)]
    pub sha256: Option<String>,
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
}
