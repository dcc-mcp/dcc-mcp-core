//! `gateway://diagnostics/*` MCP resources — gateway-native health views.
//!
//! Replaces the legacy `diagnostics__process_status` /
//! `diagnostics__audit_log` / `diagnostics__tool_metrics` MCP tools
//! (#813 phase 2). These are read-only snapshots — they belong in
//! `resources/read`, not `tools/list`.

use serde_json::{Value, json};

use super::super::state::GatewayState;
use super::util::{parse_query, split_uri};

/// URI for the gateway-native process / instance health summary.
pub const PROCESS_URI: &str = "gateway://diagnostics/process";
/// URI for the gateway-native audit summary.
pub const AUDIT_URI: &str = "gateway://diagnostics/audit";
/// URI for the gateway-native tool metrics summary.
pub const METRICS_URI: &str = "gateway://diagnostics/metrics";
/// URI for the gateway-native search and capability index diagnostics.
pub const SEARCH_URI: &str = "gateway://diagnostics/search";

/// Resource pointers emitted in `resources/list`.
pub fn pointers() -> [Value; 4] {
    [
        json!({
            "uri":         PROCESS_URI,
            "name":        "Gateway process & instance health",
            "description": "Live, stale, and unhealthy instance counts plus per-row entries. Optional ?dcc_type=<type> filters by DCC.",
            "mimeType":    "application/json"
        }),
        json!({
            "uri":         AUDIT_URI,
            "name":        "Gateway audit summary",
            "description": "Pending-call and resource-subscription counts. Backend audit logs remain available through per-instance facilities.",
            "mimeType":    "application/json"
        }),
        json!({
            "uri":         METRICS_URI,
            "name":        "Gateway tool metrics",
            "description": "Local gateway tool count, live backend count, and dispatch timeouts.",
            "mimeType":    "application/json"
        }),
        json!({
            "uri":         SEARCH_URI,
            "name":        "Gateway search & capability diagnostics",
            "description": "Capability index health and semantic ranking provider status.",
            "mimeType":    "application/json"
        }),
    ]
}

/// Parsed form of a `gateway://diagnostics/*` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// Process / instance health, optionally filtered by `dcc_type`.
    Process { dcc_type: Option<String> },
    /// Audit summary (no parameters).
    Audit,
    /// Tool metrics summary (no parameters).
    Metrics,
    /// Search and semantic ranking status.
    Search,
}

/// Recognise a `gateway://diagnostics/*` URI. Returns `None` when the URI
/// does not target this family.
pub fn parse(uri: &str) -> Option<Query> {
    let (path, query) = split_uri(uri);
    match path {
        PROCESS_URI => {
            let dcc_type = query
                .map(parse_query)
                .and_then(|m| m.get("dcc_type").map(|s| s.to_string()));
            Some(Query::Process { dcc_type })
        }
        AUDIT_URI => Some(Query::Audit),
        METRICS_URI => Some(Query::Metrics),
        SEARCH_URI => Some(Query::Search),
        _ => None,
    }
}

/// Render the payload for any `gateway://diagnostics/*` read.
///
/// `local_tool_count` is injected from the caller (the `gateway_tool_defs()`
/// length) to avoid this module's pulling in the full tool-defs surface.
/// This keeps `native_resources::diagnostics` free of the upward `tools`
/// dependency direction (DIP — high-level metrics builder depends on a
/// scalar, not on the tool registry).
pub async fn build_payload(
    gs: &GatewayState,
    query: &Query,
    local_tool_count: usize,
) -> Result<Value, String> {
    use dcc_mcp_transport::discovery::types::ServiceStatus;
    match query {
        Query::Process { dcc_type } => {
            let reg = gs.registry.read().await;
            let all = gs.all_instances(&reg);
            let dcc_filter = dcc_type.as_deref();

            let mut live_count = 0usize;
            let mut stale_count = 0usize;
            let mut unhealthy_count = 0usize;
            let instances: Vec<Value> = all
                .iter()
                .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type == f))
                .map(|e| {
                    let stale = e.is_stale(gs.stale_timeout);
                    if stale {
                        stale_count += 1;
                    } else if matches!(e.status, ServiceStatus::Available | ServiceStatus::Busy) {
                        live_count += 1;
                    } else {
                        unhealthy_count += 1;
                    }
                    gs.instance_json(e)
                })
                .collect();

            Ok(json!({
                "success": true,
                "message": "Gateway process status",
                "gateway": {
                    "server_name":    gs.server_name,
                    "server_version": gs.server_version,
                    "own_host":       gs.own_host,
                    "own_port":       gs.own_port,
                },
                "instances": instances,
                "counts": {
                    "total":     instances.len(),
                    "live":      live_count,
                    "stale":     stale_count,
                    "unhealthy": unhealthy_count,
                }
            }))
        }
        Query::Audit => {
            let pending_calls = gs.pending_calls.read().await.len();
            let subscriptions = gs.resource_subscriptions.read().await.len();
            Ok(json!({
                "success": true,
                "message": "Gateway audit summary",
                "entries": [],
                "summary": {
                    "pending_calls": pending_calls,
                    "resource_subscription_sessions": subscriptions,
                    "note": "Gateway-native audit history is not persisted; backend audit logs remain available through per-instance facilities."
                }
            }))
        }
        Query::Metrics => {
            let reg = gs.registry.read().await;
            let live_instances = gs.live_instances(&reg);
            Ok(json!({
                "success": true,
                "message": "Gateway tool metrics summary",
                "metrics": {
                    "gateway_local_tools":       local_tool_count,
                    "live_instances":            live_instances.len(),
                    "backend_timeout_ms":        gs.backend_timeout.as_millis(),
                    "async_dispatch_timeout_ms": gs.async_dispatch_timeout.as_millis(),
                    "mcp_surface":               "discover+dispatch",
                    "publishes_backend_tools":   false,
                }
            }))
        }
        Query::Search => {
            let total_records = gs.capability_index.total_records();
            let unloaded = gs.capability_index.unloaded_count();
            Ok(json!({
                "success": true,
                "message": "Gateway search and capability diagnostics",
                "semantic_provider": {
                    "active": gs.semantic_search_enabled,
                    "backend": if gs.semantic_search_enabled {
                        "dcc-mcp-gateway-search (lexical + semantic boost available)"
                    } else {
                        "dcc-mcp-gateway-search (lexical only)"
                    },
                    "note": if gs.semantic_search_enabled {
                        "Semantic boost available for mode=hybrid queries."
                    } else {
                        "Semantic ranking is not yet supported in the Rust gateway daemon."
                    },
                },
                "capability_index": {
                    "total_records": total_records,
                    "unloaded_records": unloaded,
                    "empty": total_records == 0 && unloaded == 0,
                }
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_process_root() {
        assert_eq!(
            parse("gateway://diagnostics/process"),
            Some(Query::Process { dcc_type: None })
        );
    }

    #[test]
    fn parse_process_with_dcc_filter() {
        assert_eq!(
            parse("gateway://diagnostics/process?dcc_type=maya"),
            Some(Query::Process {
                dcc_type: Some("maya".to_string())
            })
        );
    }

    #[test]
    fn parse_audit() {
        assert_eq!(parse("gateway://diagnostics/audit"), Some(Query::Audit));
    }

    #[test]
    fn parse_metrics() {
        assert_eq!(parse("gateway://diagnostics/metrics"), Some(Query::Metrics));
    }

    #[test]
    fn parse_search() {
        assert_eq!(parse("gateway://diagnostics/search"), Some(Query::Search));
    }

    #[test]
    fn parse_returns_none_for_unrelated_uris() {
        assert_eq!(parse("gateway://instances"), None);
        assert_eq!(parse("gateway://diagnostics"), None);
        assert_eq!(parse("gateway://diagnostics/unknown"), None);
        assert_eq!(parse("dcc://maya/abc"), None);
    }

    #[test]
    fn pointers_cover_all_four_uris() {
        let ps = pointers();
        let uris: Vec<&str> = ps.iter().filter_map(|p| p["uri"].as_str()).collect();
        assert!(uris.contains(&PROCESS_URI));
        assert!(uris.contains(&AUDIT_URI));
        assert!(uris.contains(&METRICS_URI));
        assert!(uris.contains(&SEARCH_URI));
    }
}
