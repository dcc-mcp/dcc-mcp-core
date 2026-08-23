//! Shared response negotiation for stable admin/debug endpoints.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;

use crate::gateway::response_codec::{ResponseFormat, negotiated_response_with_default};

pub use dcc_mcp_gateway_admin::DebugListQuery;

pub(crate) fn debug_response(
    headers: &HeaderMap,
    params: &DebugListQuery,
    status: StatusCode,
    legacy_json: Value,
    compact_json: Option<Value>,
) -> Response {
    let request_body = params.response_format_body();
    negotiated_response_with_default(
        headers,
        &request_body,
        status,
        legacy_json,
        compact_json,
        ResponseFormat::Json,
    )
}
