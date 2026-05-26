//! `initialize` negotiation for the rmcp handler (issue #239, #354, delta tools).

use std::collections::BTreeMap;

use rmcp::model::{
    ClientCapabilities, ExperimentalCapabilities, InitializeRequestParams, InitializeResult,
    JsonObject, ProtocolVersion, ServerCapabilities,
};
use serde_json::{Value, json};

use crate::server_state::ServerState;
use dcc_mcp_jsonrpc::{DELTA_TOOLS_UPDATE_CAP, negotiate_protocol_version};

fn parse_protocol_version(raw: &str) -> ProtocolVersion {
    serde_json::from_value(json!(raw)).unwrap_or_default()
}

/// Build negotiated server capabilities + session flags from client params.
pub(crate) fn build_initialize_result(
    state: &ServerState,
    session_id: &str,
    params: &InitializeRequestParams,
) -> InitializeResult {
    let negotiated = negotiate_protocol_version(Some(params.protocol_version.as_str()));

    let _ = state.sessions.set_protocol_version(session_id, negotiated);

    let client_wants_delta = params
        .capabilities
        .experimental
        .as_ref()
        .and_then(|e| e.get(DELTA_TOOLS_UPDATE_CAP))
        .and_then(|d| d.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let _ = state
        .sessions
        .set_supports_delta_tools(session_id, client_wants_delta);

    let client_supports_roots = params.capabilities.roots.is_some();
    let _ = state
        .sessions
        .set_supports_roots(session_id, client_supports_roots);

    let mut result = InitializeResult::new(server_capabilities(state, client_wants_delta));
    result.protocol_version = parse_protocol_version(negotiated);
    result.server_info =
        rmcp::model::Implementation::new(state.server_name.clone(), state.server_version.clone());
    result.instructions = Some(
        "Direct DCC workflow: search_tools(query) or search_skills(query), inspect get_skill_info(skill_name), load_skill only when needed, then tools/call. tools/list is paginated; follow nextCursor if you list it."
            .to_string(),
    );
    result
}

/// Lenient parse for clients that omit `protocolVersion` (falls back to latest).
pub(crate) fn build_initialize_result_from_value(
    state: &ServerState,
    session_id: &str,
    params: Option<&Value>,
) -> InitializeResult {
    let p = params.cloned().unwrap_or(json!({}));
    let capabilities: ClientCapabilities =
        serde_json::from_value(p.get("capabilities").cloned().unwrap_or(json!({})))
            .unwrap_or_default();
    let client_info = p
        .get("clientInfo")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| rmcp::model::Implementation::new("unknown", "0"));

    let mut init_params = InitializeRequestParams::new(capabilities, client_info);
    if let Some(version) = p.get("protocolVersion").and_then(|v| v.as_str()) {
        init_params = init_params.with_protocol_version(parse_protocol_version(version));
    }
    build_initialize_result(state, session_id, &init_params)
}

fn server_capabilities(state: &ServerState, client_wants_delta: bool) -> ServerCapabilities {
    let mut caps = match (state.enable_resources, state.enable_prompts) {
        (true, true) => ServerCapabilities::builder()
            .enable_logging()
            .enable_tools()
            .enable_tool_list_changed()
            .enable_resources()
            .enable_resources_list_changed()
            .enable_resources_subscribe()
            .enable_prompts()
            .build(),
        (true, false) => ServerCapabilities::builder()
            .enable_logging()
            .enable_tools()
            .enable_tool_list_changed()
            .enable_resources()
            .enable_resources_list_changed()
            .enable_resources_subscribe()
            .build(),
        (false, true) => ServerCapabilities::builder()
            .enable_logging()
            .enable_tools()
            .enable_tool_list_changed()
            .enable_prompts()
            .build(),
        (false, false) => ServerCapabilities::builder()
            .enable_logging()
            .enable_tools()
            .enable_tool_list_changed()
            .build(),
    };
    if client_wants_delta {
        let mut enabled = JsonObject::new();
        enabled.insert("enabled".to_string(), json!(true));
        let mut experimental: ExperimentalCapabilities = BTreeMap::new();
        experimental.insert(DELTA_TOOLS_UPDATE_CAP.to_string(), enabled);
        caps.experimental = Some(experimental);
    }
    caps
}
