//! Notification + response message routing for the MCP HTTP layer.
//!
//! Both paths are fire-and-forget from the transport's perspective —
//! notifications never generate a JSON-RPC reply, and responses to
//! server-initiated elicitations are correlated back to waiting
//! oneshot channels.

use std::time::Instant;

use serde_json::Value;

use super::state::AppState;
use crate::handlers::refresh_roots_cache_for_session;
use crate::protocol::{ElicitationCreateResult, JsonRpcResponse};

/// Process a JSON-RPC notification (a message without an `id`).
///
/// Notifications are fire-and-forget; the server must never reply to them.
/// The main notification of interest is `notifications/cancelled`, which
/// records that the client no longer needs the result of a previous request.
pub(crate) async fn handle_notification(state: &AppState, method: &str, params: Option<&Value>) {
    match method {
        "notifications/cancelled" => {
            // Extract the `requestId` field (string or number)
            let id_str = params.and_then(|p| p.get("requestId")).map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => serde_json::to_string(other).unwrap_or_default(),
            });

            if let Some(id) = id_str
                && !id.is_empty()
            {
                tracing::info!(request_id = %id, "MCP request cancelled by client");
                state.cancelled_requests.insert(id.clone(), Instant::now());
                if state.in_flight.request_cancel(&id) {
                    tracing::debug!(request_id = %id, "cancel flag set on in-flight request");
                }
            }
        }
        "notifications/roots/list_changed" => {
            let sid = params
                .and_then(|p| p.get("sessionId"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if sid.is_empty() {
                tracing::debug!(
                    "received notifications/roots/list_changed without sessionId; ignoring"
                );
                return;
            }
            if !state.sessions.supports_roots(sid) {
                tracing::debug!(
                    session_id = sid,
                    "ignoring roots/list_changed for session without roots support"
                );
                return;
            }
            let sid_owned = sid.to_string();
            let sessions = state.sessions.clone();
            tokio::spawn(async move {
                let refreshed = refresh_roots_cache_for_session(&sessions, &sid_owned).await;
                tracing::debug!(
                    session_id = sid_owned,
                    root_count = refreshed.len(),
                    "refreshed roots cache from roots/list_changed notification"
                );
            });
        }
        // Already handled as a request-shaped message; safe to ignore here.
        "notifications/initialized" => {}
        other => {
            tracing::debug!(method = other, "ignoring unknown MCP notification");
        }
    }
}

/// Correlate a client-originated JSON-RPC response against any pending
/// server-initiated elicitation request, waking the associated oneshot.
pub(crate) fn handle_response_message(state: &AppState, resp: &JsonRpcResponse) {
    let id = match &resp.id {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => return,
    };
    if id.is_empty() {
        return;
    }
    let Some((_, tx)) = state.pending_elicitations.remove(&id) else {
        return;
    };
    let resolved = if let Some(result) = resp.result.clone() {
        serde_json::from_value::<ElicitationCreateResult>(result).unwrap_or(
            ElicitationCreateResult {
                action: "decline".to_string(),
                content: None,
            },
        )
    } else {
        ElicitationCreateResult {
            action: "decline".to_string(),
            content: None,
        }
    };
    let _ = tx.send(resolved);
}
