use super::*;

/// Populate `CallToolResult._meta["dcc.next_tools"]` per issue #342.
///
/// The key is only emitted when the relevant list (on-success for a
/// success result, on-failure for an error result) is non-empty. Other
/// existing `_meta` entries are preserved; callers are expected to own
/// their own vendor namespace inside the same map.
pub fn attach_next_tools_meta(result: &mut CallToolResult, next_tools: &dcc_mcp_models::NextTools) {
    let list = if result.is_error {
        &next_tools.on_failure
    } else {
        &next_tools.on_success
    };
    if list.is_empty() {
        return;
    }

    let key = if result.is_error {
        "on_failure"
    } else {
        "on_success"
    };
    let mut next_tools_meta = serde_json::Map::new();
    next_tools_meta.insert(
        key.to_string(),
        Value::Array(
            list.iter()
                .map(|name| Value::String(name.clone()))
                .collect(),
        ),
    );
    let meta = result.meta.get_or_insert_with(serde_json::Map::new);
    meta.insert("dcc.next_tools".to_string(), Value::Object(next_tools_meta));
}

pub async fn handle_list_roots(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(session) = session_id else {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "list_roots requires Mcp-Session-Id header",
            ))?,
        ));
    };

    let roots = state.sessions.get_client_roots(session);
    let payload = json!({
        "supports_roots": state.sessions.supports_roots(session),
        "count": roots.len(),
        "roots": roots,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string_pretty(
            &payload,
        )?))?,
    ))
}
