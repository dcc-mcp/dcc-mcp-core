//! Public-safe agent trace packet projection for stable debug routes.

use axum::Json;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::agent_trace_packet;
use serde_json::{Value, json};

use super::super::state::AdminState;
use dcc_mcp_gateway_admin::AdminLinkBuilder;
/// `GET /v1/debug/agent-traces/{lookup_id}` — compact agent packet by trace id or request id.
pub async fn handle_v1_debug_agent_trace_packet(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path(lookup_id): Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &lookup_id).await {
        Some(bundle) => {
            let request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&lookup_id);
            let packet = agent_trace_packet(&lookup_id, &bundle, links.request_links(request_id));
            (StatusCode::OK, Json(packet)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent trace packet not found", "lookup_id": lookup_id })),
        )
            .into_response(),
    }
}
