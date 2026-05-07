//! Gateway-native MCP resources (issue #813 phase 1).
//!
//! These resources expose read-only registry views that previously had
//! dedicated MCP tools (`list_dcc_instances`, `get_dcc_instance`,
//! `connect_to_dcc`). Promoting them to resources keeps the gateway tool
//! surface lean — clients that need the view fetch it on demand via
//! `resources/read` instead of paying for tool definitions in every
//! `tools/list`. See the epic body for the full motivation.
//!
//! ## URI scheme
//!
//! - `gateway://instances` — full registry list (defaults to the same
//!   filter set as the legacy `list_dcc_instances` tool: stale rows
//!   visible, dead-PID rows pruned).
//! - `gateway://instances?include_stale=false` — only routable rows.
//! - `gateway://instances?include_dead=true` — raw registry view, no
//!   prune-on-read.
//! - `gateway://instances/{instance_id}` — single entry by UUID (full or
//!   unique prefix).
//!
//! Each entry in the payload includes `mcp_url`, so a client that reads
//! this resource has everything it needs to connect — there is no
//! follow-up tool call required.
//!
//! ## What the layer does NOT do
//!
//! - It does not subscribe / push: `resources/subscribe` for
//!   `gateway://*` URIs falls through to the existing no-op handler.
//!   The `notifications/resources/list_changed` debouncer in
//!   [`super::tasks`] watches backend resources only; wiring it to
//!   registry mutations is a separate change (#813 phase 1 follow-up).
//! - It does not expose every per-instance URI in `resources/list` —
//!   only the root `gateway://instances` pointer. Callers can still
//!   `resources/read` any `gateway://instances/{id}` they have learned
//!   from the list payload. Listing one entry per instance would
//!   reproduce the very fan-out the epic exists to remove.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};

/// Root URI for the gateway-native instance list.
pub const GATEWAY_INSTANCES_URI: &str = "gateway://instances";

/// URI prefix for single-instance reads (e.g. `gateway://instances/abc-123`).
pub const GATEWAY_INSTANCES_PREFIX: &str = "gateway://instances/";

/// Resource pointer emitted in `resources/list`.
pub fn instances_pointer() -> Value {
    json!({
        "uri":         GATEWAY_INSTANCES_URI,
        "name":        "DCC instance registry",
        "description": "List of every DCC server registered with the gateway. Each entry carries `mcp_url` so a client can connect without a follow-up tool call. Query params: ?include_stale=false to hide stale rows, ?include_dead=true for the raw registry view.",
        "mimeType":    "application/json"
    })
}

/// Parsed form of a `gateway://instances[/{id}][?...]` URI.
#[derive(Debug, Clone, PartialEq)]
pub enum InstancesQuery {
    /// Full list with optional filters from the URI query string.
    List {
        include_stale: bool,
        include_dead: bool,
    },
    /// Single instance lookup by UUID (full or unique prefix).
    Single { instance_id: String },
}

/// Recognise a `gateway://instances` URI (with or without query string /
/// trailing instance id) and return the parsed query. Returns `None` when
/// the URI does not target this resource family — callers fall through to
/// the next handler.
pub fn parse_instances_uri(uri: &str) -> Option<InstancesQuery> {
    // Path-only forms first ---------------------------------------------------
    if let Some(rest) = uri.strip_prefix(GATEWAY_INSTANCES_PREFIX) {
        // `gateway://instances/{id}[?...]` — strip optional query string.
        let id = rest.split('?').next().unwrap_or(rest).trim();
        if id.is_empty() {
            // `gateway://instances/` is treated as the list root, with no
            // query string.
            return Some(InstancesQuery::List {
                include_stale: true,
                include_dead: false,
            });
        }
        return Some(InstancesQuery::Single {
            instance_id: id.to_string(),
        });
    }

    // List form (with or without query string) --------------------------------
    let (path, query) = match uri.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (uri, None),
    };
    if path != GATEWAY_INSTANCES_URI {
        return None;
    }
    let mut include_stale = true;
    let mut include_dead = false;
    if let Some(q) = query {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, "true"));
            match k {
                "include_stale" => include_stale = parse_bool(v).unwrap_or(include_stale),
                "include_dead" => include_dead = parse_bool(v).unwrap_or(include_dead),
                _ => { /* unknown key: ignore (forward compatibility) */ }
            }
        }
    }
    Some(InstancesQuery::List {
        include_stale,
        include_dead,
    })
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Render the payload for a `gateway://instances*` read. Mirrors the
/// shape historically returned by `tool_list_instances` /
/// `tool_get_instance` (now removed in #813 phase 1) so any pre-existing
/// client code that learned to parse those payloads keeps working —
/// only the transport switches from `tools/call` to `resources/read`.
pub async fn build_payload(gs: &GatewayState, query: &InstancesQuery) -> Result<Value, String> {
    match query {
        InstancesQuery::List {
            include_stale,
            include_dead,
        } => {
            let reg = gs.registry.read().await;
            let (raw, evicted_dead) = if *include_dead {
                (gs.all_instances(&reg), 0usize)
            } else {
                gs.read_alive_instances(&reg).map_err(|e| e.to_string())?
            };

            let mut stale_count: usize = 0;
            let mut instances: Vec<Value> = raw
                .iter()
                .filter(|e| {
                    let stale = e.is_stale(gs.stale_timeout);
                    if stale {
                        stale_count += 1;
                    }
                    *include_stale || !stale
                })
                .map(|e| entry_to_json(e, gs.stale_timeout))
                .collect();

            instances.sort_by(|a, b| {
                a["dcc_type"]
                    .as_str()
                    .cmp(&b["dcc_type"].as_str())
                    .then(a["port"].as_u64().cmp(&b["port"].as_u64()))
            });

            Ok(json!({
                "total":        instances.len(),
                "stale_count":  stale_count,
                "evicted_dead": evicted_dead,
                "instances":    instances,
            }))
        }
        InstancesQuery::Single { instance_id } => {
            let reg = gs.registry.read().await;
            let all = gs.live_instances(&reg);
            let entry = all
                .iter()
                .find(|e| {
                    let s = e.instance_id.to_string();
                    s == *instance_id || s.starts_with(instance_id.as_str())
                })
                .ok_or_else(|| format!("Instance '{instance_id}' not found"))?;
            Ok(entry_to_json(entry, gs.stale_timeout))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_defaults() {
        assert_eq!(
            parse_instances_uri("gateway://instances"),
            Some(InstancesQuery::List {
                include_stale: true,
                include_dead: false
            })
        );
    }

    #[test]
    fn parse_root_with_query() {
        assert_eq!(
            parse_instances_uri("gateway://instances?include_stale=false"),
            Some(InstancesQuery::List {
                include_stale: false,
                include_dead: false
            })
        );
        assert_eq!(
            parse_instances_uri("gateway://instances?include_dead=true&include_stale=false"),
            Some(InstancesQuery::List {
                include_stale: false,
                include_dead: true
            })
        );
    }

    #[test]
    fn parse_unknown_query_keys_are_ignored() {
        assert_eq!(
            parse_instances_uri("gateway://instances?future=1&include_stale=false"),
            Some(InstancesQuery::List {
                include_stale: false,
                include_dead: false
            })
        );
    }

    #[test]
    fn parse_single_by_full_uuid() {
        let uuid = "01234567-89ab-cdef-0123-456789abcdef";
        assert_eq!(
            parse_instances_uri(&format!("gateway://instances/{uuid}")),
            Some(InstancesQuery::Single {
                instance_id: uuid.to_string()
            })
        );
    }

    #[test]
    fn parse_single_by_prefix() {
        assert_eq!(
            parse_instances_uri("gateway://instances/abc1234"),
            Some(InstancesQuery::Single {
                instance_id: "abc1234".to_string()
            })
        );
    }

    #[test]
    fn parse_single_strips_query_string() {
        assert_eq!(
            parse_instances_uri("gateway://instances/abc?fresh=1"),
            Some(InstancesQuery::Single {
                instance_id: "abc".to_string()
            })
        );
    }

    #[test]
    fn parse_returns_none_for_unrelated_uris() {
        assert_eq!(parse_instances_uri("dcc://maya/abc"), None);
        assert_eq!(parse_instances_uri("gateway://events"), None);
        assert_eq!(parse_instances_uri("resources://gateway/events"), None);
        assert_eq!(parse_instances_uri(""), None);
    }

    #[test]
    fn instances_pointer_carries_uri_name_and_mime() {
        let p = instances_pointer();
        assert_eq!(p["uri"], GATEWAY_INSTANCES_URI);
        assert_eq!(p["mimeType"], "application/json");
        assert!(p["name"].is_string());
        assert!(p["description"].is_string());
    }
}
