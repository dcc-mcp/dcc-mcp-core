//! Thin JSON-RPC client used by the gateway to talk to each backend DCC server.
//!
//! Each backend is a full `McpHttpServer` listening on `http://{host}:{port}/mcp`.
//! The gateway calls `tools/list` and `tools/call` on them to aggregate the
//! facade-style unified MCP endpoint exposed by the gateway itself.
//!
//! Intentionally stateless and session-less: backends accept `tools/list` /
//! `tools/call` without a prior `initialize` handshake (see `dispatch_request`
//! in `handler.rs`), which keeps the client trivial and race-free under
//! parallel fan-out.

use std::time::Duration;

use serde_json::{Value, json};

use crate::protocol::{JsonRpcResponse, McpTool};

/// Call a JSON-RPC method on a backend `/mcp` endpoint.
///
/// Returns the raw `result` value on success, or an error string on transport
/// / protocol failure. Timeouts are inherited from the `reqwest::Client`.
///
/// `request_id` lets the caller control the JSON-RPC `id` field.  When `None`
/// a fresh gateway-local id is minted.  Supplying an explicit id is required
/// for cancellation tracking (the gateway must know which backend request id
/// to cancel).
pub async fn call_backend(
    client: &reqwest::Client,
    mcp_url: &str,
    method: &str,
    params: Option<Value>,
    request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let id = request_id.unwrap_or_else(uuid_like_id);
    let req_body = if let Some(p) = params {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": p,
        })
    } else {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        })
    };

    let resp = client
        .post(mcp_url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(req_body.to_string())
        .send()
        .await
        .map_err(|e| format!("{mcp_url}: transport error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "{mcp_url}: HTTP {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("{mcp_url}: read body: {e}"))?;

    let parsed: JsonRpcResponse = serde_json::from_str(&text)
        .map_err(|e| format!("{mcp_url}: invalid JSON-RPC response: {e}"))?;

    if let Some(err) = parsed.error {
        return Err(format!(
            "{mcp_url}: backend error {}: {}",
            err.code, err.message
        ));
    }

    parsed
        .result
        .ok_or_else(|| format!("{mcp_url}: empty JSON-RPC result"))
}

/// Fetch `tools/list` from a backend and return the deserialised [`McpTool`] list.
///
/// On any failure returns an empty vector and logs a warning — callers aggregate
/// tools across many backends and should not fail the whole fan-out because one
/// instance is unreachable.
pub async fn fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<McpTool> {
    match call_backend(client, mcp_url, "tools/list", None, None, timeout).await {
        Ok(val) => val
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend tools/list failed");
            Vec::new()
        }
    }
}

/// Forward a `tools/call` to a backend and return the raw result JSON.
///
/// `request_id` is forwarded as the JSON-RPC `id` so that the gateway can
/// correlate a later `notifications/cancelled` with this backend call.
pub async fn forward_tools_call(
    client: &reqwest::Client,
    mcp_url: &str,
    tool_name: &str,
    arguments: Option<Value>,
    meta: Option<Value>,
    request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut params = json!({
        "name": tool_name,
        "arguments": arguments.unwrap_or(json!({}))
    });
    if let Some(m) = meta {
        params["_meta"] = m;
    }
    call_backend(
        client,
        mcp_url,
        "tools/call",
        Some(params),
        request_id,
        timeout,
    )
    .await
}

/// Short non-cryptographic unique ID for JSON-RPC request correlation.
///
/// We don't need uuid-level uniqueness here; the client issues one request per
/// call and reads its own response synchronously.  A timestamp-derived value is
/// enough to keep request IDs distinct in tracing.
fn uuid_like_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    format!("gw-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_like_id_increments_monotonically() {
        // Same-process sequence never collides; relative ordering preserved.
        let a = uuid_like_id();
        let b = uuid_like_id();
        assert_ne!(a, b);
        assert!(a.starts_with("gw-"));
        assert!(b.starts_with("gw-"));
    }

    /// Helper used only in tests — parse a canned JSON-RPC response body the
    /// same way `call_backend` does after HTTP success, so we can unit-test the
    /// error / empty-result / tools-list-extraction branches without spinning
    /// up a real HTTP server.
    fn parse_response_body(body: &str) -> Result<Value, String> {
        let parsed: JsonRpcResponse =
            serde_json::from_str(body).map_err(|e| format!("invalid JSON-RPC response: {e}"))?;
        if let Some(err) = parsed.error {
            return Err(format!("backend error {}: {}", err.code, err.message));
        }
        parsed
            .result
            .ok_or_else(|| "empty JSON-RPC result".to_string())
    }

    #[test]
    fn parses_success_result() {
        let body = r#"{"jsonrpc":"2.0","id":"gw-1","result":{"tools":[]}}"#;
        let result = parse_response_body(body).unwrap();
        assert_eq!(result, json!({"tools": []}));
    }

    #[test]
    fn parses_backend_error_into_error_string() {
        let body =
            r#"{"jsonrpc":"2.0","id":"gw-1","error":{"code":-32601,"message":"Method not found"}}"#;
        let err = parse_response_body(body).unwrap_err();
        assert!(err.contains("-32601"));
        assert!(err.contains("Method not found"));
    }

    #[test]
    fn treats_missing_result_as_error() {
        let body = r#"{"jsonrpc":"2.0","id":"gw-1"}"#;
        let err = parse_response_body(body).unwrap_err();
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn rejects_malformed_json() {
        let body = "not json at all";
        let err = parse_response_body(body).unwrap_err();
        assert!(err.contains("invalid JSON-RPC"), "got: {err}");
    }

    #[test]
    fn extracts_tools_array_from_tools_list_result() {
        // Mirrors the inner `.get("tools").and_then(as_array)` path in fetch_tools.
        let result = json!({
            "tools": [
                {"name": "create_sphere", "description": "make sphere", "inputSchema": {"type": "object"}},
                {"name": "delete_node", "description": "delete", "inputSchema": {"type": "object"}}
            ]
        });
        let tools: Vec<McpTool> = result
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "create_sphere");
        assert_eq!(tools[1].name, "delete_node");
    }

    #[test]
    fn handles_tools_list_with_malformed_entries_gracefully() {
        // One good tool + one malformed entry should yield a list of exactly
        // the good tool — the bad one is silently dropped (fetch_tools policy).
        let result = json!({
            "tools": [
                {"name": "good_tool", "description": "ok", "inputSchema": {"type": "object"}},
                {"not_a_tool": true}
            ]
        });
        let tools: Vec<McpTool> = result
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "good_tool");
    }
}
