//! Axum request handlers for the three MCP Streamable HTTP verbs.
//!
//! - `POST /mcp`   — client sends JSON-RPC messages; response is JSON or SSE
//! - `GET  /mcp`   — client opens a long-lived SSE stream for server-push events
//! - `DELETE /mcp` — client closes its session

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::sse::Event,
    response::{IntoResponse, Response, Sse},
};
use futures::stream;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use super::dispatch::dispatch_request;
use super::notifications::{handle_notification, handle_response_message};
use super::state::AppState;
use crate::handlers::{json_error_response, parse_body, parse_raw_values};
use crate::protocol::{
    self, JsonRpcBatch, JsonRpcMessage, JsonRpcResponse, MCP_SESSION_HEADER, format_sse_event,
};

fn invalid_request_response(message: impl Into<String>) -> Response {
    json_error_response(
        StatusCode::OK,
        None,
        protocol::error_codes::INVALID_REQUEST,
        message,
    )
}

/// Handle `POST /mcp`: accept JSON-RPC message(s) and return response.
pub async fn handle_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Parse body — keep raw Value array for id-presence detection
    let raw_values: Vec<Value> = match parse_raw_values(&body) {
        Ok(v) => v,
        Err(e) => {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                None,
                protocol::error_codes::PARSE_ERROR,
                format!("Parse error: {e}"),
            );
        }
    };
    if raw_values.is_empty() {
        return invalid_request_response("Invalid Request: empty batch");
    }
    for raw in &raw_values {
        let Some(obj) = raw.as_object() else {
            return invalid_request_response("Invalid Request: message must be an object");
        };
        if obj.contains_key("id")
            && !obj.contains_key("method")
            && !obj.contains_key("result")
            && !obj.contains_key("error")
        {
            return invalid_request_response(
                "Invalid Request: message with id must include method, result, or error",
            );
        }
    }

    let messages: JsonRpcBatch = match parse_body(&body) {
        Ok(m) => m,
        Err(e) => {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                None,
                protocol::error_codes::PARSE_ERROR,
                format!("Parse error: {e}"),
            );
        }
    };

    // Only JSON-RPC requests need responses. Client responses may also carry
    // `id`, but they are acknowledgements for server-initiated requests.
    let has_requests = messages
        .iter()
        .any(|msg| matches!(msg, JsonRpcMessage::Request(req) if req.id.is_some()));

    // Always process notifications (fire-and-forget — no id) so that
    // `notifications/cancelled` can abort in-flight tool calls.
    for msg in &messages {
        if let JsonRpcMessage::Notification(notif) = msg {
            handle_notification(&state, &notif.method, notif.params.as_ref()).await;
        }
    }
    // Client responses to server-initiated elicitation requests arrive as
    // JSON-RPC responses. Correlate and wake the waiting oneshot channel.
    for msg in &messages {
        if let JsonRpcMessage::Response(resp) = msg {
            handle_response_message(&state, resp);
        }
    }

    if !has_requests {
        // Only notifications/responses — accept and return 202
        return StatusCode::ACCEPTED.into_response();
    }

    // Process requests and build responses
    let mut responses: Vec<JsonRpcResponse> = Vec::new();
    let mut use_sse = false;

    // Check if client accepts SSE
    if let Some(accept) = headers.get(header::ACCEPT) {
        if accept.to_str().unwrap_or("").contains("text/event-stream") {
            use_sse = true;
        }
    }

    for msg in &messages {
        if let JsonRpcMessage::Request(req) = msg {
            match dispatch_request(&state, req, session_id.as_deref()).await {
                Ok(resp) => responses.push(resp),
                Err(e) => {
                    responses.push(JsonRpcResponse::internal_error(
                        req.id.clone(),
                        e.to_string(),
                    ));
                }
            }
        }
    }

    if use_sse && session_id.is_some() {
        // Return as SSE stream (allows server push alongside response)
        let events: Vec<String> = responses
            .iter()
            .map(|r| format_sse_event(r, None))
            .collect();

        let stream = stream::iter(events).map(Ok::<_, std::convert::Infallible>);

        let body = Body::from_stream(stream);
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("X-Accel-Buffering", "no")
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    } else {
        // Return as JSON
        let body = if responses.len() == 1 {
            serde_json::to_string(&responses[0]).unwrap_or_default()
        } else {
            serde_json::to_string(&responses).unwrap_or_default()
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}

/// Handle `GET /mcp`: open SSE stream for server-push events.
pub async fn handle_get(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Validate Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !accept.contains("text/event-stream") {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let rx: broadcast::Receiver<String> = if let Some(id) = &session_id {
        match state.sessions.subscribe(id) {
            Some(rx) => rx,
            None => {
                return json_error_response(
                    StatusCode::NOT_FOUND,
                    None,
                    -32600,
                    "Session not found",
                );
            }
        }
    } else {
        // No session — create an ephemeral one. `subscribe` only returns None
        // if the session was concurrently dropped between create() and
        // subscribe(); fall back to a 500 error rather than panicking so a
        // future SessionManager refactor cannot silently take down the server.
        let id = state.sessions.create();
        match state.sessions.subscribe(&id) {
            Some(rx) => rx,
            None => {
                return json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                    -32603,
                    "Failed to subscribe to freshly created session",
                );
            }
        }
    };

    let sse_stream = BroadcastStream::new(rx)
        .filter_map(|res| res.ok())
        .map(|data| {
            // Each item is already a formatted SSE event string
            // Parse it back to send as axum SSE Event
            Ok::<_, std::convert::Infallible>(Event::default().data(data))
        });

    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::new())
        .into_response()
}

/// Handle `DELETE /mcp`: terminate a session.
pub async fn handle_delete(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok());

    match session_id {
        Some(id) if state.sessions.remove(id) => {
            if state.enable_resources {
                state.resources.drop_session(id);
            }
            StatusCode::NO_CONTENT
        }
        Some(_) => StatusCode::NOT_FOUND,
        None => StatusCode::BAD_REQUEST,
    }
}
