use super::*;

/// Build the unified `tools/list` result — read-only gateway discovery only.
///
/// The gateway MCP surface is intentionally **minimal + static**: `tools/list`
/// returns only the read-only discover/inspect primitives. Per-action backend
/// tools are never fanned out here; agents discover them dynamically through
/// `search` / `describe`, then invoke through the canonical REST plane
/// (`POST /v1/call` or `/v1/call_batch`). Hidden MCP compatibility routes still
/// accept older gateway calls, but they are not advertised.
///
/// Tool order:
/// 1. `search` — compact capability and skill discovery.
/// 2. `describe` — schema/detail lookup for one discovered item.
///
/// Pagination uses the same cursor scheme as the per-DCC server:
/// `cursor` is an opaque hex-encoded offset into the flat tool list.
///
/// [`GATEWAY_LOCAL_TOOLS`]: dcc_mcp_gateway_core::naming::GATEWAY_LOCAL_TOOLS
pub async fn aggregate_tools_list(gs: &GatewayState, cursor: Option<&str>) -> Value {
    // Touch `gs` so clippy stops warning about the unused argument; future
    // expansions (auth-aware filtering etc.) will consume it again.
    let _ = gs;

    let mut tools: Vec<Value> = Vec::new();

    // Local gateway tools. This is the entire advertised gateway MCP surface.
    if let Value::Array(local) = gateway_tool_defs() {
        tools.extend(local);
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
