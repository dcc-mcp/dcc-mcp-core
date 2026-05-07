//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey};

// ── tools ──────────────────────────────────────────────────────────────────

/// `acquire_dcc_instance` — reserve an idle DCC instance for a workflow/client.
pub async fn tool_acquire_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_type = args
        .get("dcc_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide dcc_type".to_string())?;
    let owner = args
        .get("lease_owner")
        .and_then(|v| v.as_str())
        .unwrap_or("anonymous");
    let instance_id = args.get("instance_id").and_then(|v| v.as_str());
    let current_job_id = args
        .get("current_job_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let ttl_secs = args
        .get("ttl_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600)
        .max(1);

    let reg = gs.registry.read().await;
    let Some(entry) = reg
        .acquire_lease(
            dcc_type,
            instance_id,
            owner,
            current_job_id,
            Some(std::time::Duration::from_secs(ttl_secs)),
        )
        .map_err(|e| e.to_string())?
    else {
        return Err(format!(
            "No idle '{dcc_type}' instance is available for lease. \
             Release a busy instance or start another DCC process."
        ));
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Leased {dcc_type} instance {}", entry.instance_id),
        "instance": entry_to_json(&entry, gs.stale_timeout),
    }))
    .map_err(|e| e.to_string())
}

/// `release_dcc_instance` — release a previously acquired instance lease.
pub async fn tool_release_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let instance_id = args
        .get("instance_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide instance_id".to_string())?;
    let owner = args.get("lease_owner").and_then(|v| v.as_str());

    let reg = gs.registry.read().await;
    let all = gs.all_instances(&reg);
    let matches: Vec<&ServiceEntry> = all
        .iter()
        .filter(|e| {
            let full = e.instance_id.to_string();
            full == instance_id || full.starts_with(instance_id)
        })
        .collect();
    let entry = match matches.as_slice() {
        [] => return Err(format!("Instance '{instance_id}' not found")),
        [entry] => *entry,
        _ => return Err(format!("Instance prefix '{instance_id}' is ambiguous")),
    };
    let key = ServiceKey {
        dcc_type: entry.dcc_type.clone(),
        instance_id: entry.instance_id,
    };
    let Some(released) = reg.release_lease(&key, owner).map_err(|e| e.to_string())? else {
        return Err("No matching active lease to release".to_string());
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Released lease for instance {}", released.instance_id),
        "instance": entry_to_json(&released, gs.stale_timeout),
    }))
    .map_err(|e| e.to_string())
}

// ── #655 dynamic-capability MCP wrappers ──────────────────────────────────

/// `search_tools` — MCP wrapper that routes to
/// [`crate::gateway::capability_service::search_service`].
///
/// Kept alongside the REST handler so both transports produce
/// byte-identical responses for the same query.
pub async fn tool_search_tools(gs: &GatewayState, args: &Value) -> Result<String, String> {
    // Refresh on demand so the first query after startup (or after
    // a skill load/unload) always sees current capabilities.
    crate::gateway::capability_service::refresh_all_live_backends(
        gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let query = crate::gateway::capability_service::parse_search_payload(args);
    let hits = crate::gateway::capability_service::search_service(&gs.capability_index, &query);
    serde_json::to_string_pretty(&json!({
        "total": hits.len(),
        "hits":  hits,
    }))
    .map_err(|e| e.to_string())
}

/// `describe_tool` — MCP wrapper around
/// [`crate::gateway::capability_service::describe_service`].
pub async fn tool_describe_tool(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return Err("missing required argument: tool_slug".to_string());
    };
    // Refresh on demand — a `describe_tool` call immediately after
    // `load_skill` must see the newly-registered action.
    crate::gateway::capability_service::refresh_all_live_backends(
        gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    match crate::gateway::capability_service::describe_tool_full(gs, slug).await {
        Ok((record, tool)) => serde_json::to_string_pretty(&json!({
            "record": record,
            "tool":   tool,
        }))
        .map_err(|e| e.to_string()),
        Err(err) => {
            let payload = crate::gateway::capability_service::service_error_to_json(&err);
            Err(serde_json::to_string_pretty(&payload).unwrap_or_else(|_| err.message.clone()))
        }
    }
}

/// `call_tool` — MCP wrapper around
/// [`crate::gateway::capability_service::call_service`].
///
/// Returns the raw backend `tools/call` envelope on success so
/// progress events and structured content survive the wrapper.
pub async fn tool_call_tool(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
) -> (String, bool) {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return ("missing required argument: tool_slug".to_string(), true);
    };
    let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let forwarded_meta = args.get("meta").cloned().or_else(|| meta.cloned());
    // No refresh here on purpose: `call_tool` is the hot path and
    // we trust that the caller used `describe_tool` / `search_tools`
    // to obtain the slug, both of which refresh. An unknown-slug
    // error from `describe_service` will trigger one refresh at the
    // end of this function if the record is missing, keeping the
    // happy path fast.
    match crate::gateway::capability_service::call_service(
        gs,
        slug,
        arguments.clone(),
        forwarded_meta.clone(),
    )
    .await
    {
        Ok(result) => (
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            false,
        ),
        Err(err) if err.kind == "unknown-slug" => {
            // Refresh once in case the caller supplied a slug that
            // just became valid (e.g. a skill loaded between
            // `search_tools` and `call_tool`), then retry.
            crate::gateway::capability_service::refresh_all_live_backends(
                gs,
                crate::gateway::capability::RefreshReason::Periodic,
            )
            .await;
            match crate::gateway::capability_service::call_service(
                gs,
                slug,
                arguments,
                forwarded_meta,
            )
            .await
            {
                Ok(result) => (
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
                    false,
                ),
                Err(err2) => {
                    let payload = crate::gateway::capability_service::service_error_to_json(&err2);
                    (
                        serde_json::to_string_pretty(&payload)
                            .unwrap_or_else(|_| err2.message.clone()),
                        true,
                    )
                }
            }
        }
        Err(err) => {
            let payload = crate::gateway::capability_service::service_error_to_json(&err);
            (
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| err.message.clone()),
                true,
            )
        }
    }
}

/// Gateway-native `diagnostics__process_status`.
pub async fn tool_diagnostics_process_status(
    gs: &GatewayState,
    args: &Value,
) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.all_instances(&reg);
    let dcc_filter = args.get("dcc_type").and_then(|v| v.as_str());

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
            } else if matches!(
                e.status,
                dcc_mcp_transport::discovery::types::ServiceStatus::Available
                    | dcc_mcp_transport::discovery::types::ServiceStatus::Busy
            ) {
                live_count += 1;
            } else {
                unhealthy_count += 1;
            }
            entry_to_json(e, gs.stale_timeout)
        })
        .collect();

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway process status",
        "gateway": {
            "server_name": gs.server_name,
            "server_version": gs.server_version,
            "own_host": gs.own_host,
            "own_port": gs.own_port,
        },
        "instances": instances,
        "counts": {
            "total": instances.len(),
            "live": live_count,
            "stale": stale_count,
            "unhealthy": unhealthy_count,
        }
    }))
    .map_err(|e| e.to_string())
}

/// Gateway-native `diagnostics__audit_log`.
pub async fn tool_diagnostics_audit_log(
    gs: &GatewayState,
    _args: &Value,
) -> Result<String, String> {
    let pending_calls = gs.pending_calls.read().await.len();
    let subscriptions = gs.resource_subscriptions.read().await.len();
    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway audit summary",
        "entries": [],
        "summary": {
            "pending_calls": pending_calls,
            "resource_subscription_sessions": subscriptions,
            "note": "Gateway-native audit history is not persisted; backend audit logs remain available via prefixed diagnostics tools when exposed by a DCC instance."
        }
    }))
    .map_err(|e| e.to_string())
}

/// Gateway-native `diagnostics__tool_metrics`.
pub async fn tool_diagnostics_tool_metrics(
    gs: &GatewayState,
    _args: &Value,
) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let live_instances = gs.live_instances(&reg);
    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway tool metrics summary",
        "metrics": {
            "gateway_local_tools": gateway_tool_defs().as_array().map_or(0, Vec::len),
            "live_instances": live_instances.len(),
            "backend_timeout_ms": gs.backend_timeout.as_millis(),
            "async_dispatch_timeout_ms": gs.async_dispatch_timeout.as_millis(),
            "mcp_surface": "discover+dispatch",
            "publishes_backend_tools": false,
        }
    }))
    .map_err(|e| e.to_string())
}

/// `dcc_catalog__search` — search the public DCC-MCP catalog by keyword.
///
/// Accepts an optional `query` string; an empty / absent query returns all
/// entries.  Results are sorted by name and serialised as a JSON array.
pub fn tool_catalog_search(args: &Value) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let catalog_path = catalog_yml_path();
    let entries = dcc_mcp_catalog::load_from_file(&catalog_path)
        .map_err(|e| format!("catalog load error: {e}"))?;

    let mut hits = dcc_mcp_catalog::search(&entries, query);
    hits.sort_by(|a, b| a.name.cmp(&b.name));

    serde_json::to_string_pretty(&json!({
        "total": hits.len(),
        "query": query,
        "entries": hits,
    }))
    .map_err(|e| e.to_string())
}

/// `dcc_catalog__describe` — look up a single catalog entry by exact name.
pub fn tool_catalog_describe(args: &Value) -> Result<String, String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required argument: name".to_string())?;

    let catalog_path = catalog_yml_path();
    let entries = dcc_mcp_catalog::load_from_file(&catalog_path)
        .map_err(|e| format!("catalog load error: {e}"))?;

    match dcc_mcp_catalog::describe(&entries, name) {
        Some(entry) => serde_json::to_string_pretty(&entry).map_err(|e| e.to_string()),
        None => Err(format!("catalog entry '{name}' not found")),
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

// ── private helpers ────────────────────────────────────────────────────────

/// Return the JSON schema for gateway discovery and diagnostics tools.
pub fn gateway_tool_defs() -> serde_json::Value {
    json!([
        {
            "name": "acquire_dcc_instance",
            "description": "Reserve an idle DCC instance for a workflow or long-running job. \
                This marks the instance busy and stores lease metadata in the shared registry. \
                Pooling is optional; simple single-instance adapters can ignore this tool.",
            "inputSchema": {
                "type": "object",
                "required": ["dcc_type"],
                "properties": {
                    "dcc_type":       {"type": "string", "description": "DCC type to lease (e.g. 'maya')"},
                    "instance_id":    {"type": "string", "description": "Optional UUID or unique prefix to lease a specific instance"},
                    "lease_owner":    {"type": "string", "description": "Client/workflow owner label for the lease"},
                    "current_job_id": {"type": "string", "description": "Optional job id associated with the lease"},
                    "ttl_secs":       {"type": "integer", "minimum": 1, "description": "Lease TTL in seconds (default: 3600)"}
                }
            }
        },
        {
            "name": "release_dcc_instance",
            "description": "Release a DCC instance lease acquired with acquire_dcc_instance.",
            "inputSchema": {
                "type": "object",
                "required": ["instance_id"],
                "properties": {
                    "instance_id": {"type": "string", "description": "UUID or unique prefix from list_dcc_instances"},
                    "lease_owner": {"type": "string", "description": "Optional owner guard; when provided it must match the active lease owner"}
                }
            }
        },
        {
            "name": "search_tools",
            "description": "Search dynamic DCC capabilities by keyword, DCC type, tags, or scene hint. \
                Returns compact records (not full schemas) so token cost stays bounded even when \
                many DCC instances are live. Use `describe_tool` to fetch one capability's schema, \
                then `call_tool` to invoke it. Part of the REST-backed dynamic-capability API (#657).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":      {"type": "string", "description": "Keyword(s) matched against tool name, summary, tags, and skill name."},
                    "dcc_type":   {"type": "string", "description": "Optional DCC bucket filter (e.g. 'maya', 'blender')."},
                    "tags":       {"type": "array", "items": {"type": "string"}, "description": "Require every tag to be present."},
                    "scene_hint": {"type": "string", "description": "Optional scene/document hint used as a soft boost."},
                    "limit":      {"type": "integer", "minimum": 0, "description": "Page size cap (default 25, max 100)."}
                }
            }
        },
        {
            "name": "describe_tool",
            "description": "Resolve a single capability slug returned by `search_tools` back to its \
                compact record (name, skill, summary, tags, whether it has a schema, and the \
                backing instance id). Use this before `call_tool` when the caller needs the \
                action's metadata without invoking it.",
            "inputSchema": {
                "type": "object",
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {"type": "string", "description": "Capability slug in the form `<dcc>.<id8>.<tool>`."}
                }
            }
        },
        {
            "name": "call_tool",
            "description": "Invoke a DCC action by capability slug. Routes through the same \
                backend-forwarding machinery as the legacy prefixed gateway tools, so progress \
                notifications, cancellation, and job routing all work identically. Arguments are \
                the backend tool's usual JSON payload; `meta` is optional MCP `_meta` passthrough.",
            "inputSchema": {
                "type": "object",
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {"type": "string", "description": "Capability slug from `search_tools` / `describe_tool`."},
                    "arguments": {"type": "object", "description": "Tool arguments forwarded to the backend."},
                    "meta":      {"type": "object", "description": "Optional MCP `_meta` passthrough (e.g. `dcc.async`)."}
                }
            }
        },
        {
            "name": "diagnostics__process_status",
            "description": "Gateway-native process and instance health summary. Lists live, stale, and unhealthy DCC registrations without requiring a backend instance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dcc_type": {"type": "string", "description": "Optional DCC type filter."}
                }
            }
        },
        {
            "name": "diagnostics__audit_log",
            "description": "Gateway-native audit summary for pending calls and resource subscriptions. Backend audit logs remain available through instance tools.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "diagnostics__tool_metrics",
            "description": "Gateway-native tool metrics summary for local gateway tools and live backend count.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "dcc_catalog__search",
            "description": "Search the public DCC-MCP catalog for adapters, skill packs, and plugins. \
                Pass a `query` string to filter by name, description, DCC type, or tag. \
                Omit `query` (or pass an empty string) to list every catalog entry.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to search for (matched against name, description, dcc, tags). Omit for all entries."
                    }
                }
            }
        },
        {
            "name": "dcc_catalog__describe",
            "description": "Return the full catalog record for a single DCC-MCP package. \
                Use the exact `name` returned by `dcc_catalog__search`.",
            "inputSchema": {
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact catalog entry name (e.g. 'dcc-mcp-maya-skills')."
                    }
                }
            }
        }
    ])
}

// ── catalog tool tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Write YAML content to a temp file and return the handle (keep it alive).
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

    #[test]
    fn catalog_search_empty_query_returns_all() {
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: single-threaded test; no concurrent env access
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let args = serde_json::json!({"query": ""});
        let result = tool_catalog_search(&args).expect("search should succeed");
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(v["total"], 2, "empty query should return all 2 entries");
        // SAFETY: same as set_var above
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[test]
    fn catalog_search_absent_query_returns_all() {
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: single-threaded test; no concurrent env access
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let args = serde_json::json!({});
        let result = tool_catalog_search(&args).expect("search should succeed");
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(v["total"], 2, "absent query key should return all entries");
        // SAFETY: same as set_var above
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[test]
    fn catalog_search_keyword_filters_results() {
        let f = write_catalog_yaml(TWO_ENTRY_YAML);
        // SAFETY: single-threaded test; no concurrent env access
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let args = serde_json::json!({"query": "maya"});
        let result = tool_catalog_search(&args).expect("search should succeed");
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(v["total"], 1);
        assert_eq!(v["entries"][0]["name"], "maya-skills");
        // SAFETY: same as set_var above
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[test]
    fn catalog_describe_existing_entry_returns_data() {
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: single-threaded test; no concurrent env access
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let args = serde_json::json!({"name": "maya-skills"});
        let result = tool_catalog_describe(&args).expect("describe should succeed");
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(v["name"], "maya-skills");
        assert_eq!(v["dcc"][0], "maya");
        // SAFETY: same as set_var above
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[test]
    fn catalog_describe_missing_name_returns_not_found() {
        let f = write_catalog_yaml(ONE_ENTRY_YAML);
        // SAFETY: single-threaded test; no concurrent env access
        unsafe { std::env::set_var("DCC_MCP_CATALOG_PATH", f.path()) };

        let args = serde_json::json!({"name": "does-not-exist"});
        let err = tool_catalog_describe(&args).expect_err("missing entry should return Err");

        assert!(
            err.contains("not found"),
            "error message should contain 'not found', got: {err}"
        );
        // SAFETY: same as set_var above
        unsafe { std::env::remove_var("DCC_MCP_CATALOG_PATH") };
    }

    #[test]
    fn catalog_describe_missing_name_arg_returns_error() {
        let args = serde_json::json!({});
        let err = tool_catalog_describe(&args).expect_err("missing 'name' arg should return Err");
        assert!(err.contains("name"), "error should mention 'name': {err}");
    }
}
