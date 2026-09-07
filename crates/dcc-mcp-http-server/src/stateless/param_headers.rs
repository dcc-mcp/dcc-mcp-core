//! HTTP-only parameter preflight: no dispatcher, registry, or legacy mutation.

use dcc_mcp_jsonrpc::{
    JsonRpcRequest, JsonRpcResponse, McpParamValidationError, scan_mcp_param_headers,
    validate_mcp_param_headers,
};
use serde_json::json;

use crate::rmcp_tool_call_dispatch::{stateless_wire_tool, warn_invalid_parameter_schema};
use crate::server_state::ServerState;

use super::StatelessDispatchOutcome;

pub(super) fn parameter_header_error(
    state: &ServerState,
    req: &JsonRpcRequest,
    header: impl Fn(&str) -> Option<String>,
) -> Option<StatelessDispatchOutcome> {
    if req.method != "tools/call" || req.id.is_none() {
        return None;
    }
    let params = req.params.as_ref()?;
    let name = params.get("name")?.as_str()?;
    // The normal modern argument-shape gate owns malformed business params.
    if params
        .get("arguments")
        .is_some_and(|value| !value.is_object())
    {
        return None;
    }
    let (tool, _) = stateless_wire_tool(state, name)?;
    let declarations = match scan_mcp_param_headers(&tool.input_schema) {
        Ok(declarations) => declarations,
        Err(issue) => {
            warn_invalid_parameter_schema(name, issue.reason);
            return Some(StatelessDispatchOutcome::Response(
                serde_json::to_value(JsonRpcResponse::internal_error(
                    req.id.clone(),
                    "Invalid tool parameter header configuration",
                ))
                .expect("configuration error response"),
            ));
        }
    };
    let empty = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty);
    match validate_mcp_param_headers(&declarations, arguments, header) {
        Ok(()) => None,
        Err(McpParamValidationError::InvalidArgument { reason, .. }) => {
            Some(StatelessDispatchOutcome::Response(
                serde_json::to_value(JsonRpcResponse::invalid_params(req.id.clone(), reason))
                    .expect("argument error response"),
            ))
        }
        Err(McpParamValidationError::HeaderMismatch { reason }) => {
            Some(StatelessDispatchOutcome::InvalidEnvelope(
                serde_json::to_value(JsonRpcResponse::header_mismatch(
                    req.id.clone(),
                    "Mcp-Param-*",
                    reason,
                ))
                .expect("parameter header error response"),
            ))
        }
    }
}
