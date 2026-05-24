//! `gateway://catalog*` MCP resources — public DCC-MCP package catalog.
//!
//! Replaces the legacy `dcc_catalog__search` / `dcc_catalog__describe`
//! MCP tools (#813 phase 2). The catalog is a static YAML file shipped
//! beside the binary; it is a textbook read-only view that belongs in
//! `resources/read`, not `tools/list`.

use serde_json::{Value, json};

use super::util::{parse_query, split_uri};

/// Root URI for the catalog index.
pub const ROOT_URI: &str = "gateway://catalog";

/// URI prefix for single-entry reads (e.g. `gateway://catalog/dcc-mcp-maya-skills`).
pub const PREFIX: &str = "gateway://catalog/";

/// Resource pointer emitted in `resources/list`.
pub fn pointer() -> Value {
    json!({
        "uri":         ROOT_URI,
        "name":        "Public DCC-MCP catalog",
        "description": "Searchable index of adapters, skill packs, and plugins. Optional ?query=<keyword> filters by name / description / DCC / tags. Single entries: gateway://catalog/{name}.",
        "mimeType":    "application/json"
    })
}

/// Parsed form of a `gateway://catalog*` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// Index, optionally filtered by keyword.
    List { query: String },
    /// Single entry by exact name.
    Single { name: String },
}

/// Recognise a `gateway://catalog*` URI. Returns `None` when the URI does
/// not target this family.
pub fn parse(uri: &str) -> Option<Query> {
    if let Some(rest) = uri.strip_prefix(PREFIX) {
        let name = rest.split('?').next().unwrap_or(rest).trim();
        if name.is_empty() {
            // `gateway://catalog/` collapses to the list root.
            return Some(Query::List {
                query: String::new(),
            });
        }
        return Some(Query::Single {
            name: name.to_string(),
        });
    }

    let (path, query) = split_uri(uri);
    if path != ROOT_URI {
        return None;
    }
    let q = query
        .map(parse_query)
        .and_then(|m| m.get("query").map(|s| s.to_string()))
        .unwrap_or_default();
    Some(Query::List { query: q })
}

/// Render the payload for a `gateway://catalog*` read.
///
/// Catalog I/O is synchronous (small YAML file); we expose an `async fn`
/// for symmetry with the other resource families and to leave room for
/// I/O pools in the future.
pub async fn build_payload(query: &Query) -> Result<Value, String> {
    let path = catalog_yml_path();
    let entries =
        dcc_mcp_catalog::load_from_file(&path).map_err(|e| format!("catalog load error: {e}"))?;

    match query {
        Query::List { query } => {
            let mut hits = dcc_mcp_catalog::search(&entries, query);
            hits.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(json!({
                "total":   hits.len(),
                "query":   query,
                "entries": hits,
            }))
        }
        Query::Single { name } => match dcc_mcp_catalog::describe(&entries, name) {
            Some(entry) => serde_json::to_value(entry).map_err(|e| e.to_string()),
            None => Err(format!("catalog entry '{name}' not found")),
        },
    }
}

/// Resolve the path to `dcc-mcp-catalog.yml`.
///
/// Priority:
/// 1. `DCC_MCP_CATALOG_PATH` env var (absolute path override)
/// 2. Adjacent to the running executable (`exe_dir/dcc-mcp-catalog.yml`)
/// 3. Current working directory
fn catalog_yml_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("DCC_MCP_CATALOG_PATH") {
        return std::path::PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("dcc-mcp-catalog.yml");
        if candidate.exists() {
            return candidate;
        }
    }
    std::path::PathBuf::from("dcc-mcp-catalog.yml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_no_query() {
        assert_eq!(
            parse("gateway://catalog"),
            Some(Query::List {
                query: String::new()
            })
        );
    }

    #[test]
    fn parse_root_with_query() {
        assert_eq!(
            parse("gateway://catalog?query=maya"),
            Some(Query::List {
                query: "maya".to_string()
            })
        );
    }

    #[test]
    fn parse_single_by_name() {
        assert_eq!(
            parse("gateway://catalog/dcc-mcp-maya-skills"),
            Some(Query::Single {
                name: "dcc-mcp-maya-skills".to_string()
            })
        );
    }

    #[test]
    fn parse_single_strips_query_string() {
        assert_eq!(
            parse("gateway://catalog/abc?fresh=1"),
            Some(Query::Single {
                name: "abc".to_string()
            })
        );
    }

    #[test]
    fn parse_returns_none_for_unrelated_uris() {
        assert_eq!(parse("gateway://instances"), None);
        assert_eq!(parse("gateway://diagnostics/process"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn pointer_carries_uri_name_and_mime() {
        let p = pointer();
        assert_eq!(p["uri"], ROOT_URI);
        assert_eq!(p["mimeType"], "application/json");
    }

    // ── build_payload end-to-end (YAML → JSON) ───────────────────────────

    use std::io::Write;
    use tempfile::NamedTempFile;

    static CATALOG_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn write_catalog_yaml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    const TWO_ENTRY_YAML: &str = r#"
version: "1"
entries:
  - name: maya-skills
    description: Maya skill pack
    dcc: [maya]
    url: https://example.com/maya
    tags: [skills]
  - name: blender-skills
    description: Blender skill pack
    dcc: [blender]
    url: https://example.com/blender
    tags: [skills]
"#;

    const ONE_ENTRY_YAML: &str = r#"
version: "1"
entries:
  - name: maya-skills
    description: Maya skill pack
    dcc: [maya]
    url: https://example.com/maya
    tags: [skills]
"#;

    #[tokio::test]
    async fn build_payload_list_empty_query_returns_all() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let v = build_payload(&Query::List {
            query: String::new(),
        })
        .await
        .expect("build_payload should succeed");

        assert_eq!(v["total"], 2);
        // SAFETY: cleanup symmetrical to set_var above.
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[tokio::test]
    async fn build_payload_list_keyword_filters_results() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let v = build_payload(&Query::List {
            query: "maya".to_string(),
        })
        .await
        .expect("build_payload should succeed");

        assert_eq!(v["total"], 1);
        assert_eq!(v["entries"][0]["name"], "maya-skills");
        // SAFETY: see above.
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[tokio::test]
    async fn build_payload_single_existing_entry_returns_data() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let v = build_payload(&Query::Single {
            name: "maya-skills".to_string(),
        })
        .await
        .expect("describe should succeed");

        assert_eq!(v["name"], "maya-skills");
        assert_eq!(v["dcc"][0], "maya");
        // SAFETY: see above.
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[tokio::test]
    async fn build_payload_single_missing_entry_errors() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let err = build_payload(&Query::Single {
            name: "does-not-exist".to_string(),
        })
        .await
        .expect_err("missing entry should return Err");

        assert!(err.contains("not found"));
        // SAFETY: see above.
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }
}
