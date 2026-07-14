//! `gateway://instances` MCP resource — DCC instance registry view.
//!
//! Replaces the legacy `list_dcc_instances` / `get_dcc_instance` /
//! `connect_to_dcc` MCP tools (#813 phase 1). Each entry carries
//! `mcp_url` so a client that has read this resource has everything it
//! needs to connect — there is no follow-up tool call.
//!
//! ## PIP-2725: instance query surface (first slice)
//!
//! The list endpoint now supports filtering (`dcc_type`, `query`,
//! `status`, `limit`/`offset`) and compact projection by default.
//! Compact hits (~500B) carry only the fields an agent needs to select
//! an `instance_id`; `verbose=true` restores the full instance JSON.
//! Honesty fields (`list_ok`, `index_health`) separate "we listed
//! successfully" from "these instances are dispatch-ready".

use serde_json::{Value, json};

use dcc_mcp_transport::discovery::types::ServiceEntry;

use super::super::state::GatewayState;
use super::util::{parse_bool, parse_query, split_uri};

/// Root URI for the gateway-native instance list.
pub const ROOT_URI: &str = "gateway://instances";

/// URI prefix for single-instance reads (e.g. `gateway://instances/abc-123`).
pub const PREFIX: &str = "gateway://instances/";

/// Default page size when `limit` is not specified.
const DEFAULT_LIMIT: usize = 20;

/// Maximum page size to prevent context blow-up.
const MAX_LIMIT: usize = 200;

/// Resource pointer emitted in `resources/list`.
pub fn pointer() -> Value {
    json!({
        "uri":         ROOT_URI,
        "name":        "DCC instance registry",
        "description": "List of DCC instances registered with the gateway. Supports filtering: ?dcc_type=maya, ?query=<substring>, ?status=available, ?limit=N, ?offset=N, ?verbose=true. Default response is compact (~500B/hit); verbose returns full instance objects. Honesty fields (list_ok, index_health) separate listing success from dispatch readiness (PIP-2725).",
        "mimeType":    "application/json"
    })
}

/// Parsed form of a `gateway://instances[/{id}][?...]` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// Full list with optional filters from the URI query string.
    List {
        /// Include stale (no-heartbeat) entries (default: true).
        include_stale: bool,
        /// Include dead-PID entries via `read_alive_instances` (default: false).
        include_dead: bool,
        /// Exact DCC type filter (e.g. "blender", "maya").
        dcc_type: Option<String>,
        /// Substring search across display_id, version, instance_short, scene.
        query: Option<String>,
        /// Status filter: "available", "busy", "stale".
        status: Option<String>,
        /// Page size (1..MAX_LIMIT, default: DEFAULT_LIMIT).
        limit: usize,
        /// Zero-based page offset.
        offset: usize,
        /// When true, return full instance objects instead of compact projection.
        verbose: bool,
    },
    /// Single instance lookup by UUID (full or unique prefix).
    Single { instance_id: String },
}

impl Query {
    /// Default list query with backward-compatible defaults.
    fn default_list() -> Self {
        Query::List {
            include_stale: true,
            include_dead: false,
            dcc_type: None,
            query: None,
            status: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
            verbose: false,
        }
    }
}

/// Recognise a `gateway://instances*` URI and return the parsed query.
/// Returns `None` when the URI does not target this resource family —
/// callers fall through to the next handler.
pub fn parse(uri: &str) -> Option<Query> {
    // Path-only forms first: `gateway://instances/{id}[?...]`.
    if let Some(rest) = uri.strip_prefix(PREFIX) {
        let id = rest.split('?').next().unwrap_or(rest).trim();
        if id.is_empty() {
            // `gateway://instances/` collapses to the list root.
            return Some(Query::default_list());
        }
        return Some(Query::Single {
            instance_id: id.to_string(),
        });
    }

    // List form (with or without query string).
    let (path, query) = split_uri(uri);
    if path != ROOT_URI {
        return None;
    }

    let mut q = Query::default_list();

    if let Some(query_str) = query {
        let params = parse_query(query_str);
        if let Query::List {
            include_stale,
            include_dead,
            dcc_type,
            query: query_filter,
            status,
            limit,
            offset,
            verbose,
        } = &mut q
        {
            if let Some(v) = params.get("include_stale")
                && let Some(b) = parse_bool(v)
            {
                *include_stale = b;
            }
            if let Some(v) = params.get("include_dead")
                && let Some(b) = parse_bool(v)
            {
                *include_dead = b;
            }
            if let Some(v) = params.get("dcc_type") {
                *dcc_type = Some(v.to_string());
            }
            if let Some(v) = params.get("query") {
                *query_filter = Some(v.to_string());
            }
            if let Some(v) = params.get("status") {
                *status = Some(v.to_string());
            }
            if let Some(v) = params.get("limit")
                && let Ok(n) = v.parse::<usize>()
            {
                *limit = n.clamp(1, MAX_LIMIT);
            }
            if let Some(v) = params.get("offset")
                && let Ok(n) = v.parse::<usize>()
            {
                *offset = n;
            }
            if let Some(v) = params.get("verbose")
                && let Some(b) = parse_bool(v)
            {
                *verbose = b;
            }
        }
    }

    Some(q)
}

/// Render the payload for a `gateway://instances*` read.
///
/// List responses include `list_ok` and `index_health` honesty fields
/// (PIP-2725) that separate the listing operation's success from
/// per-instance dispatch readiness. A response with `list_ok: true` may
/// still carry `index_health: "degraded"` or instances whose
/// `dispatch.ready` is `false` — callers must check both.
pub async fn build_payload(gs: &GatewayState, query: &Query) -> Result<Value, String> {
    match query {
        Query::List {
            include_stale,
            include_dead,
            dcc_type,
            query: query_filter,
            status,
            limit,
            offset,
            verbose,
        } => {
            let reg = gs.registry.read().await;

            // ── Fetch raw entries ──────────────────────────────────────
            let (raw, evicted_dead) = if *include_dead {
                (gs.all_instances(&reg), 0usize)
            } else {
                gs.read_alive_instances(&reg).map_err(|e| e.to_string())?
            };

            // ── Compute index health before filtering ──────────────────
            let index_health = compute_index_health(raw.len(), evicted_dead, gs.stale_timeout);

            let stale_timeout = gs.stale_timeout;
            let query_lower = query_filter.as_ref().map(|s| s.to_ascii_lowercase());
            let status_lower = status.as_ref().map(|s| s.trim().to_ascii_lowercase());

            let mut stale_count: usize = 0;
            let mut total_before_paging: usize = 0;

            // ── Filter pipeline ────────────────────────────────────────
            let filtered: Vec<&ServiceEntry> = raw
                .iter()
                .filter(|e| {
                    // dcc_type exact match (case-insensitive)
                    if let Some(dt) = dcc_type
                        && !e.dcc_type.eq_ignore_ascii_case(dt)
                    {
                        return false;
                    }
                    // staleness filter
                    let stale = e.is_stale(stale_timeout);
                    if stale {
                        stale_count += 1;
                    }
                    if !include_stale && stale {
                        return false;
                    }
                    // status filter
                    if let Some(ref s) = status_lower {
                        match s.as_str() {
                            "available" if (e.status.to_string() != "available" || stale) => {
                                return false;
                            }
                            "busy" if (e.status.to_string() != "busy" || stale) => {
                                return false;
                            }
                            "stale" if !stale => {
                                return false;
                            }
                            _ => {} // unknown status values pass through
                        }
                    }
                    // substring query filter
                    if let Some(ref q) = query_lower
                        && !instance_matches_query(e, q)
                    {
                        return false;
                    }
                    true
                })
                .inspect(|_| total_before_paging += 1)
                .collect();

            // ── Pagination ─────────────────────────────────────────────
            let capped = total_before_paging > *limit;
            let page: Vec<&ServiceEntry> = filtered
                .iter()
                .skip(*offset)
                .take(*limit)
                .copied()
                .collect();

            // ── Project to JSON ────────────────────────────────────────
            let instances: Vec<Value> = if *verbose {
                page.iter().map(|e| gs.instance_json(e)).collect()
            } else {
                page.iter()
                    .map(|e| compact_instance_json(e, stale_timeout))
                    .collect()
            };

            // ── Assemble response ──────────────────────────────────────
            let mut resp = json!({
                "list_ok":       true,
                "index_health":  index_health,
                "total":         total_before_paging,
                "capped":        capped,
                "limit":         limit,
                "offset":        offset,
                "stale_count":   stale_count,
                "evicted_dead":  evicted_dead,
                "instances":     instances,
            });

            // Include by_source for backward compatibility (verbose only)
            if *verbose {
                resp["by_source"] = json!(super::super::state::instance_source_counts(&instances));
            }

            Ok(resp)
        }
        Query::Single { instance_id } => {
            let reg = gs.registry.read().await;
            let entry = gs
                .resolve_instance(&reg, Some(instance_id.as_str()), None)
                .map_err(|err| err.to_string())?;
            Ok(gs.instance_json(&entry))
        }
    }
}

/// Compact instance projection (~500B/hit target, PIP-2725).
///
/// Includes only the fields an agent needs to select an `instance_id`:
/// identity, type, version, status, staleness, and separated
/// readiness/dispatch signals. Omits `lifecycle`, `gateway`, `metadata`,
/// `diagnostics`, `pool`, `host_execution`, and other verbose blocks
/// unless `verbose=true`.
pub fn compact_instance_json(e: &ServiceEntry, stale_timeout: std::time::Duration) -> Value {
    let stale = e.is_stale(stale_timeout)
        || matches!(
            e.status,
            dcc_mcp_transport::discovery::types::ServiceStatus::Stale
        );

    let status_str = if stale {
        "stale".to_string()
    } else {
        e.status.to_string()
    };

    // ── dispatch readiness (separated from list success) ──────────────
    let dispatch_status = e
        .metadata
        .get("dispatch_status")
        .map(|s| s.trim().to_ascii_lowercase());
    let dispatch_ready = dispatch_status.as_deref() == Some("ready")
        && e.metadata.contains_key("mcp_url")
        && matches!(
            e.status,
            dcc_mcp_transport::discovery::types::ServiceStatus::Available
                | dcc_mcp_transport::discovery::types::ServiceStatus::Busy
        )
        && !stale;
    let dispatch_ready_value = if dispatch_status.is_some() {
        Value::Bool(dispatch_ready)
    } else {
        Value::Null
    };

    json!({
        "instance_id":    e.instance_id.to_string(),
        "instance_short": dcc_mcp_gateway_core::naming::instance_short(&e.instance_id),
        "display_id":     e.display_id(),
        "dcc_type":       e.dcc_type,
        "version":        e.version,
        "host":           e.host,
        "port":           e.port,
        "mcp_url":        super::super::http_registration::entry_mcp_url(e),
        "source":         super::super::http_registration::entry_registry_source(e),
        "status":         status_str,
        "stale":          stale,
        "scene":          e.scene,
        "dispatch": {
            "reported": dispatch_status.is_some(),
            "ready":    dispatch_ready_value,
            "status":   dispatch_status.as_deref().unwrap_or("not_reported"),
        },
    })
}

/// Check whether an instance matches a substring query across
/// display_id, version, instance_short, and scene fields.
pub fn instance_matches_query(e: &ServiceEntry, query_lower: &str) -> bool {
    e.display_id().to_ascii_lowercase().contains(query_lower)
        || e.version
            .as_ref()
            .is_some_and(|v| v.to_ascii_lowercase().contains(query_lower))
        || dcc_mcp_gateway_core::naming::instance_short(&e.instance_id)
            .to_ascii_lowercase()
            .contains(query_lower)
        || e.scene
            .as_ref()
            .is_some_and(|s| s.to_ascii_lowercase().contains(query_lower))
}

/// Compute a human-readable index health signal (PIP-2725 honesty field).
///
/// - `"healthy"`: registry has live entries, no evictions.
/// - `"degraded"`: entries present but evictions occurred.
/// - `"empty"`: no entries at all.
/// - `"stale"`: entries exist but all are stale (future enhancement).
pub fn compute_index_health(
    total_raw: usize,
    evicted_dead: usize,
    _stale_timeout: std::time::Duration,
) -> &'static str {
    if total_raw == 0 {
        return "empty";
    }
    if evicted_dead > 0 {
        return "degraded";
    }
    // We can't determine staleness without iterating entries here,
    // but evicted_dead > 0 is the strongest signal of degraded state.
    // The caller (build_payload) already tracks stale_count; if needed,
    // a future iteration can thread that through.
    "healthy"
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parse tests (backward-compatible) ────────────────────────────────

    #[test]
    fn parse_root_defaults() {
        assert_eq!(parse("gateway://instances"), Some(Query::default_list()));
    }

    #[test]
    fn parse_root_with_legacy_query() {
        assert_eq!(
            parse("gateway://instances?include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: false,
                dcc_type: None,
                query: None,
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
        assert_eq!(
            parse("gateway://instances?include_dead=true&include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: true,
                dcc_type: None,
                query: None,
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
    }

    #[test]
    fn parse_unknown_query_keys_are_ignored() {
        assert_eq!(
            parse("gateway://instances?future=1&include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: false,
                dcc_type: None,
                query: None,
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
    }

    // ── New PIP-2725 parse tests ─────────────────────────────────────────

    #[test]
    fn parse_with_dcc_type() {
        assert_eq!(
            parse("gateway://instances?dcc_type=blender"),
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: Some("blender".to_string()),
                query: None,
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
    }

    #[test]
    fn parse_with_query() {
        assert_eq!(
            parse("gateway://instances?query=4.2"),
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: None,
                query: Some("4.2".to_string()),
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
    }

    #[test]
    fn parse_with_status() {
        assert_eq!(
            parse("gateway://instances?status=available"),
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: None,
                query: None,
                status: Some("available".to_string()),
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: false,
            })
        );
    }

    #[test]
    fn parse_with_limit_and_offset() {
        assert_eq!(
            parse("gateway://instances?limit=10&offset=5"),
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: None,
                query: None,
                status: None,
                limit: 10,
                offset: 5,
                verbose: false,
            })
        );
    }

    #[test]
    fn parse_limit_clamped_to_max() {
        let q = parse("gateway://instances?limit=9999").unwrap();
        if let Query::List { limit, .. } = q {
            assert_eq!(limit, MAX_LIMIT);
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_limit_clamped_to_min() {
        let q = parse("gateway://instances?limit=0").unwrap();
        if let Query::List { limit, .. } = q {
            assert_eq!(limit, 1);
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_with_verbose() {
        assert_eq!(
            parse("gateway://instances?verbose=true"),
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: None,
                query: None,
                status: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
                verbose: true,
            })
        );
    }

    #[test]
    fn parse_combined_filters() {
        let q = parse(
            "gateway://instances?dcc_type=maya&query=2024&status=available&limit=5&offset=0&verbose=false",
        );
        assert_eq!(
            q,
            Some(Query::List {
                include_stale: true,
                include_dead: false,
                dcc_type: Some("maya".to_string()),
                query: Some("2024".to_string()),
                status: Some("available".to_string()),
                limit: 5,
                offset: 0,
                verbose: false,
            })
        );
    }

    // ── Single-instance parse tests ──────────────────────────────────────

    #[test]
    fn parse_single_by_full_uuid() {
        let uuid = "01234567-89ab-cdef-0123-456789abcdef";
        assert_eq!(
            parse(&format!("gateway://instances/{uuid}")),
            Some(Query::Single {
                instance_id: uuid.to_string()
            })
        );
    }

    #[test]
    fn parse_single_by_prefix() {
        assert_eq!(
            parse("gateway://instances/abc1234"),
            Some(Query::Single {
                instance_id: "abc1234".to_string()
            })
        );
    }

    #[test]
    fn parse_single_strips_query_string() {
        assert_eq!(
            parse("gateway://instances/abc?fresh=1"),
            Some(Query::Single {
                instance_id: "abc".to_string()
            })
        );
    }

    #[test]
    fn parse_returns_none_for_unrelated_uris() {
        assert_eq!(parse("dcc://maya/abc"), None);
        assert_eq!(parse("gateway://events"), None);
        assert_eq!(parse("resources://gateway/events"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn pointer_carries_uri_name_and_mime() {
        let p = pointer();
        assert_eq!(p["uri"], ROOT_URI);
        assert_eq!(p["mimeType"], "application/json");
        assert!(p["name"].is_string());
        assert!(p["description"].is_string());
    }

    // ── Compact projection tests ─────────────────────────────────────────

    #[test]
    fn compact_json_has_required_fields() {
        let mut entry = ServiceEntry::new("blender", "127.0.0.1", 9876);
        entry.version = Some("4.2.0".to_string());
        entry.scene = Some("/tmp/test.blend".to_string());
        let json = compact_instance_json(&entry, std::time::Duration::from_secs(30));

        // Required identity fields
        assert!(json["instance_id"].is_string());
        assert!(json["instance_short"].is_string());
        assert!(json["display_id"].is_string());
        assert_eq!(json["dcc_type"], "blender");
        assert_eq!(json["version"], "4.2.0");

        // Honesty fields
        assert!(json["status"].is_string());
        assert!(json["stale"].as_bool().is_some());
        assert!(json["dispatch"]["ready"].is_null() || json["dispatch"]["ready"].is_boolean());
        assert!(json["dispatch"]["status"].is_string());

        // Compact projection must NOT include verbose fields
        assert!(json.get("lifecycle").is_none());
        assert!(json.get("gateway").is_none());
        assert!(json.get("metadata").is_none());
        assert!(json.get("diagnostics").is_none());
        assert!(json.get("pool").is_none());
        assert!(json.get("host_execution").is_none());

        // Size check: compact hit should be well under 500B
        let compact_str = serde_json::to_string(&json).unwrap();
        assert!(
            compact_str.len() < 500,
            "compact hit size {}B exceeds 500B target",
            compact_str.len()
        );
    }

    #[test]
    fn compact_json_with_stale_entry() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 9877);
        entry.version = Some("2024.2".to_string());
        // Set last_heartbeat far in the past to make it stale
        entry.last_heartbeat = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(120))
            .unwrap();
        let json = compact_instance_json(&entry, std::time::Duration::from_secs(30));
        assert_eq!(json["status"], "stale");
        assert_eq!(json["stale"], true);
    }

    #[test]
    fn compact_json_dispatch_ready() {
        let mut entry = ServiceEntry::new("houdini", "127.0.0.1", 9878);
        entry.version = Some("21.0".to_string());
        entry.metadata = std::collections::HashMap::from([
            ("dispatch_status".to_string(), "ready".to_string()),
            (
                "mcp_url".to_string(),
                "http://127.0.0.1:9878/mcp".to_string(),
            ),
        ]);
        let json = compact_instance_json(&entry, std::time::Duration::from_secs(30));
        assert_eq!(json["dispatch"]["ready"], true);
        assert_eq!(json["dispatch"]["reported"], true);
        assert_eq!(json["dispatch"]["status"], "ready");
    }

    // ── Query matching tests ─────────────────────────────────────────────

    #[test]
    fn query_matches_display_id() {
        let mut entry = ServiceEntry::new("blender", "127.0.0.1", 9876);
        entry.version = Some("4.2.0".to_string());
        // display_id is derived as "blender@4.2.0-{short8}"
        assert!(instance_matches_query(&entry, "blender"));
        assert!(instance_matches_query(&entry, "4.2"));
    }

    #[test]
    fn query_matches_version() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 9877);
        entry.version = Some("2024.2".to_string());
        assert!(instance_matches_query(&entry, "2024"));
        assert!(instance_matches_query(&entry, "2024.2"));
        assert!(!instance_matches_query(&entry, "blender"));
    }

    #[test]
    fn query_matches_scene() {
        let mut entry = ServiceEntry::new("blender", "127.0.0.1", 9876);
        entry.version = Some("4.2.0".to_string());
        entry.scene = Some("/projects/hero_asset.blend".to_string());
        assert!(instance_matches_query(&entry, "hero"));
        assert!(instance_matches_query(&entry, "asset"));
        assert!(!instance_matches_query(&entry, "villain"));
    }

    // ── Index health tests ───────────────────────────────────────────────

    #[test]
    fn index_health_empty() {
        assert_eq!(
            compute_index_health(0, 0, std::time::Duration::from_secs(30)),
            "empty"
        );
    }

    #[test]
    fn index_health_degraded() {
        assert_eq!(
            compute_index_health(5, 2, std::time::Duration::from_secs(30)),
            "degraded"
        );
    }

    #[test]
    fn index_health_healthy() {
        assert_eq!(
            compute_index_health(5, 0, std::time::Duration::from_secs(30)),
            "healthy"
        );
    }
}
