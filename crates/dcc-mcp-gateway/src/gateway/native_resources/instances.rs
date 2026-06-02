//! `gateway://instances` MCP resource — DCC instance registry view.
//!
//! Replaces the legacy `list_dcc_instances` / `get_dcc_instance` /
//! `connect_to_dcc` MCP tools (#813 phase 1). Each entry carries
//! `mcp_url` so a client that has read this resource has everything it
//! needs to connect — there is no follow-up tool call.

use serde_json::{Value, json};

use super::super::state::GatewayState;
use super::util::{parse_bool, parse_query, split_uri};

/// Root URI for the gateway-native instance list.
pub const ROOT_URI: &str = "gateway://instances";

/// URI prefix for single-instance reads (e.g. `gateway://instances/abc-123`).
pub const PREFIX: &str = "gateway://instances/";

/// Resource pointer emitted in `resources/list`.
pub fn pointer() -> Value {
    json!({
        "uri":         ROOT_URI,
        "name":        "DCC instance registry",
        "description": "List of every DCC server registered with the gateway. Each entry carries `mcp_url` so a client can connect without a follow-up tool call. Query params: ?include_stale=false to hide stale rows, ?include_dead=true for the raw registry view.",
        "mimeType":    "application/json"
    })
}

/// Parsed form of a `gateway://instances[/{id}][?...]` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// Full list with optional filters from the URI query string.
    List {
        include_stale: bool,
        include_dead: bool,
    },
    /// Single instance lookup by UUID (full or unique prefix).
    Single { instance_id: String },
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
            return Some(Query::List {
                include_stale: true,
                include_dead: false,
            });
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
    let mut include_stale = true;
    let mut include_dead = false;
    if let Some(q) = query {
        let params = parse_query(q);
        if let Some(v) = params.get("include_stale")
            && let Some(b) = parse_bool(v)
        {
            include_stale = b;
        }
        if let Some(v) = params.get("include_dead")
            && let Some(b) = parse_bool(v)
        {
            include_dead = b;
        }
    }
    Some(Query::List {
        include_stale,
        include_dead,
    })
}

/// Render the payload for a `gateway://instances*` read. Mirrors the
/// shape historically returned by `tool_list_instances` /
/// `tool_get_instance` (now removed) so any pre-existing client code
/// that learned to parse those payloads keeps working — only the
/// transport switches from `tools/call` to `resources/read`.
pub async fn build_payload(gs: &GatewayState, query: &Query) -> Result<Value, String> {
    match query {
        Query::List {
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
                .map(|e| gs.instance_json(e))
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
                "by_source":    super::super::state::instance_source_counts(&instances),
                "instances":    instances,
            }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_defaults() {
        assert_eq!(
            parse("gateway://instances"),
            Some(Query::List {
                include_stale: true,
                include_dead: false
            })
        );
    }

    #[test]
    fn parse_root_with_query() {
        assert_eq!(
            parse("gateway://instances?include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: false
            })
        );
        assert_eq!(
            parse("gateway://instances?include_dead=true&include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: true
            })
        );
    }

    #[test]
    fn parse_unknown_query_keys_are_ignored() {
        assert_eq!(
            parse("gateway://instances?future=1&include_stale=false"),
            Some(Query::List {
                include_stale: false,
                include_dead: false
            })
        );
    }

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
}
