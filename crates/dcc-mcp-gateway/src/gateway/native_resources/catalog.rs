//! `gateway://catalog*` MCP resources — public DCC-MCP package catalog.
//!
//! Fetches from the official marketplace repository at
//! `https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json`
//! and falls back to a local `dcc-mcp-catalog.yml` when the network is
//! unavailable. Set `DCC_MCP_CATALOG_PATH` to override the local fallback.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::RwLock;

use super::util::{parse_query, split_uri};

/// Marketplace raw URL — canonical source of truth for the catalog.
pub const MARKETPLACE_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json";

/// Root URI for the catalog index.
pub const ROOT_URI: &str = "gateway://catalog";

/// URI prefix for single-entry reads (e.g. `gateway://catalog/dcc-mcp-maya-skills`).
pub const PREFIX: &str = "gateway://catalog/";

/// How long to cache the marketplace catalog in memory before re-fetching.
const CACHE_TTL: Duration = Duration::from_secs(300);

fn offline_mode() -> bool {
    std::env::var("DCC_MCP_MARKETPLACE_OFFLINE")
        .map(|v| matches!(v.as_str(), "1" | "true"))
        .unwrap_or(false)
}

/// Catalog entries plus the instant they were loaded.
struct CatalogSnapshot {
    entries: Vec<dcc_mcp_catalog::CatalogEntry>,
    fetched_at: Instant,
}

/// A cached catalog loader that fetches from the marketplace repo on first
/// access and refreshes after `CACHE_TTL`.
pub struct CatalogLoader {
    client: reqwest::Client,
    cache: RwLock<Option<CatalogSnapshot>>,
}

impl CatalogLoader {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("CatalogLoader reqwest::Client should build"),
            cache: RwLock::new(None),
        }
    }

    /// Return catalog entries, preferring a fresh fetch from the marketplace
    /// URL with local-file fallback.
    pub async fn load_entries(&self) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, String> {
        // Serve cached entries when within TTL.
        {
            let cache = self.cache.read().await;
            if let Some(ref snap) = *cache
                && snap.fetched_at.elapsed() < CACHE_TTL
            {
                return Ok(snap.entries.clone());
            }
        }

        // Try marketplace URL first (unless offline), then local file fallback.
        let entries = if offline_mode() {
            tracing::debug!("marketplace offline mode; loading from local file");
            load_from_local_file()?
        } else {
            match self.fetch_marketplace().await {
                Ok(entries) => {
                    tracing::info!(count = entries.len(), "catalog loaded from marketplace");
                    entries
                }
                Err(marketplace_err) => {
                    tracing::warn!(
                        error = %marketplace_err,
                        "marketplace catalog fetch failed, falling back to local file"
                    );
                    load_from_local_file()?
                }
            }
        };

        let mut cache = self.cache.write().await;
        *cache = Some(CatalogSnapshot {
            entries: entries.clone(),
            fetched_at: Instant::now(),
        });
        Ok(entries)
    }

    /// Clear the cached entries (useful for tests that swap catalog files).
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
    }

    async fn fetch_marketplace(&self) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, String> {
        let url = std::env::var("DCC_MCP_MARKETPLACE_CATALOG_URL")
            .unwrap_or_else(|_| MARKETPLACE_CATALOG_URL.to_string());
        if offline_mode() {
            return Err("marketplace offline (DCC_MCP_MARKETPLACE_OFFLINE=1)".into());
        }
        let text = self
            .client
            .get(&url)
            .header("User-Agent", "dcc-mcp-gateway catalog")
            .send()
            .await
            .map_err(|e| format!("marketplace fetch error: {e}"))?
            .error_for_status()
            .map_err(|e| format!("marketplace HTTP error: {e}"))?
            .text()
            .await
            .map_err(|e| format!("marketplace read error: {e}"))?;
        dcc_mcp_catalog::load_from_str(&text).map_err(|e| format!("marketplace parse error: {e}"))
    }
}

impl Default for CatalogLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-local default loader. Created lazily via `catalog_entries()`.
static LOADER: std::sync::LazyLock<CatalogLoader> = std::sync::LazyLock::new(CatalogLoader::new);

/// Convenience wrapper that always uses the default loader.
async fn catalog_entries() -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, String> {
    LOADER.load_entries().await
}

/// Resource pointer emitted in `resources/list`.
pub fn pointer() -> Value {
    json!({
        "uri":         ROOT_URI,
        "name":        "Public DCC-MCP catalog",
        "description": "Searchable index of adapters, skill packs, and plugins from dcc-mcp/marketplace. Optional ?query=<keyword> filters by name / description / DCC / tags. Single entries: gateway://catalog/{name}.",
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
pub async fn build_payload(query: &Query) -> Result<Value, String> {
    let entries = catalog_entries().await?;

    match query {
        Query::List { query } => {
            let mut hits = dcc_mcp_catalog::search(&entries, query);
            hits.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(json!({
                "total":   hits.len(),
                "query":   query,
                "source":  "marketplace",
                "entries": hits,
            }))
        }
        Query::Single { name } => match dcc_mcp_catalog::describe(&entries, name) {
            Some(entry) => serde_json::to_value(entry).map_err(|e| e.to_string()),
            None => Err(format!("catalog entry '{name}' not found")),
        },
    }
}

/// Fallback: load catalog from the local `.yml` file.
fn load_from_local_file() -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, String> {
    let path = catalog_yml_path();
    dcc_mcp_catalog::load_from_file(&path).map_err(|e| {
        format!(
            "local catalog load error ({path}): {e}",
            path = path.display()
        )
    })
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
        LOADER.clear_cache().await;
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_OFFLINE", "1");
            std::env::set_var("DCC_MCP_CATALOG_PATH", f.path());
        }

        let v = build_payload(&Query::List {
            query: String::new(),
        })
        .await
        .expect("build_payload should succeed");

        assert_eq!(v["total"], 2);
        // SAFETY: cleanup symmetrical to set_var above.
        unsafe {
            std::env::remove_var("DCC_MCP_MARKETPLACE_OFFLINE");
            std::env::remove_var("DCC_MCP_CATALOG_PATH");
        }
    }

    #[tokio::test]
    async fn build_payload_list_keyword_filters_results() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        LOADER.clear_cache().await;
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_OFFLINE", "1");
            std::env::set_var("DCC_MCP_CATALOG_PATH", f.path());
        }

        let v = build_payload(&Query::List {
            query: "maya".to_string(),
        })
        .await
        .expect("build_payload should succeed");

        assert_eq!(v["total"], 1);
        assert_eq!(v["entries"][0]["name"], "maya-skills");
        // SAFETY: see above.
        unsafe {
            std::env::remove_var("DCC_MCP_MARKETPLACE_OFFLINE");
            std::env::remove_var("DCC_MCP_CATALOG_PATH");
        }
    }

    #[tokio::test]
    async fn build_payload_single_existing_entry_returns_data() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        LOADER.clear_cache().await;
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_OFFLINE", "1");
            std::env::set_var("DCC_MCP_CATALOG_PATH", f.path());
        }

        let v = build_payload(&Query::Single {
            name: "maya-skills".to_string(),
        })
        .await
        .expect("describe should succeed");

        assert_eq!(v["name"], "maya-skills");
        assert_eq!(v["dcc"][0], "maya");
        // SAFETY: see above.
        unsafe {
            std::env::remove_var("DCC_MCP_MARKETPLACE_OFFLINE");
            std::env::remove_var("DCC_MCP_CATALOG_PATH");
        }
    }

    #[tokio::test]
    async fn build_payload_single_missing_entry_errors() {
        let _env_guard = CATALOG_ENV_LOCK.lock().await;
        LOADER.clear_cache().await;
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: process-wide env mutation is serialized by CATALOG_ENV_LOCK.
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_OFFLINE", "1");
            std::env::set_var("DCC_MCP_CATALOG_PATH", f.path());
        }

        let err = build_payload(&Query::Single {
            name: "does-not-exist".to_string(),
        })
        .await
        .expect_err("missing entry should return Err");

        assert!(err.contains("not found"));
        // SAFETY: see above.
        unsafe {
            std::env::remove_var("DCC_MCP_MARKETPLACE_OFFLINE");
            std::env::remove_var("DCC_MCP_CATALOG_PATH");
        }
    }
}
