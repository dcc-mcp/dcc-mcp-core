use super::super::namespace::encode_tool_name_cursor_safe;
use super::*;

/// Build the unified `tools/list` result by aggregating every live backend.
///
/// Tool order:
/// 1. Gateway discovery / pooling meta-tools (`list_dcc_instances`,
///    `get_dcc_instance`, `connect_to_dcc`, `acquire_dcc_instance`,
///    `release_dcc_instance`).
/// 2. Skill-management tools (one canonical set for the whole gateway).
/// 3. Backend-provided tools from every live instance, prefixed with the
///    8-char instance id, annotated with `_instance_id` / `_dcc_type` in the
///    tool's `annotations` map so agents can display origin context.
///
/// Tier 3 fan-out is skipped entirely when the gateway is configured with
/// [`GatewayToolExposure::Slim`] or [`GatewayToolExposure::Rest`]
/// (issue #652). In those modes the visible surface stays bounded to Tier
/// 1 + 2 regardless of how many live backends are registered; agents are
/// expected to discover and invoke backend capabilities through the
/// dynamic wrapper layer described in #657.
///
/// The Tier 3 encoding is selected by
/// [`GatewayState::cursor_safe_tool_names`](super::super::state::GatewayState::cursor_safe_tool_names)
/// (issue #656). When `true` (default), tools are published as
/// `i_<id8>__<escaped>` names that survive the stricter
/// `^[A-Za-z0-9_]+$` regex enforced by Cursor and other MCP clients;
/// no bare aliases are emitted from the gateway; the prefixed form is
/// the only advertised backend tool identifier. When `false`, pre-#656
/// SEP-986 dotted names are emitted for diagnostic parity.
///
/// Pagination uses the same cursor scheme as the per-DCC server:
/// `cursor` is an opaque hex-encoded offset into the flat tool list.
///
/// [`GatewayToolExposure::Slim`]: super::super::config::GatewayToolExposure::Slim
/// [`GatewayToolExposure::Rest`]: super::super::config::GatewayToolExposure::Rest
pub async fn aggregate_tools_list(gs: &GatewayState, cursor: Option<&str>) -> Value {
    let mut tools: Vec<Value> = Vec::new();

    // Tier 1 + 2: local gateway tools (meta + skill management).
    if let Value::Array(local) = gateway_tool_defs() {
        tools.extend(local);
    }
    tools.extend(skill_management_tool_defs());

    // Tier 3: fan out to every live backend — but only in modes that
    // publish per-backend tools (#652). Slim / Rest keep the gateway
    // surface bounded so multi-instance setups do not blow up client
    // context.
    if gs.tool_exposure.publishes_backend_tools() {
        // Issue #556: skip Unreachable instances so stale tools are not exposed.
        let instances: Vec<_> = live_backends(gs)
            .await
            .into_iter()
            .filter(|e| {
                !matches!(
                    e.status,
                    dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                )
            })
            .collect();
        let client = &gs.http_client;
        let backend_timeout = gs.backend_timeout;
        let futs = instances.iter().map(|entry| async move {
            let url = format!("http://{}:{}/mcp", entry.host, entry.port);
            let backend_tools = fetch_tools(client, &url, backend_timeout).await;
            (entry.instance_id, entry.dcc_type.clone(), backend_tools)
        });
        let results = join_all(futs).await;
        let cursor_safe = gs.cursor_safe_tool_names;

        for (iid, dcc_type, backend_tools) in results {
            for mut tool in backend_tools {
                // Skip any tool whose name would collide with a gateway-local name
                // AFTER encoding — cannot happen today because local tools are
                // already filtered by `is_local_tool`, but guard defensively.
                if is_local_tool(&tool.name) {
                    continue;
                }
                let encoded = if cursor_safe {
                    encode_tool_name_cursor_safe(&iid, &tool.name)
                } else {
                    encode_tool_name(&iid, &tool.name)
                };
                tool.name = encoded;
                let mut json_val = serde_json::to_value(&tool).unwrap_or(Value::Null);
                inject_instance_metadata(&mut json_val, &iid, &dcc_type);
                tools.push(json_val);
            }
        }
    }

    // ── Pagination ───────────────────────────────────────────────────────
    let offset = cursor.and_then(decode_cursor).unwrap_or(0);
    let total = tools.len();
    let page_end = (offset + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<Value> = if offset < total {
        tools.drain(offset..page_end).collect()
    } else {
        Vec::new()
    };

    let mut result = json!({"tools": page});
    if page_end < total {
        result["nextCursor"] = json!(encode_cursor(page_end));
    }
    result
}
