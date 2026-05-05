//! Public DCC-MCP catalog for ecosystem discovery.
//!
//! Provides [`CatalogEntry`] (a typed YAML record), and two discovery
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
}

/// Top-level catalog document.
#[derive(Debug, Deserialize)]
struct CatalogDoc {
    #[allow(dead_code)]
    version: String,
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

/// Parse catalog entries from a YAML string.
pub fn load_from_str(yaml: &str) -> Result<Vec<CatalogEntry>, CatalogError> {
    let doc: CatalogDoc =
        serde_yaml_ng::from_str(yaml).map_err(|e| CatalogError::Parse(e.to_string()))?;
    Ok(doc.entries)
}

// ── search / describe ─────────────────────────────────────────────────────────

/// Search catalog entries.
///
/// `query` is matched case-insensitively against `name`, `description`,
/// `dcc`, and `tags`.  An empty query returns all entries.
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
}
