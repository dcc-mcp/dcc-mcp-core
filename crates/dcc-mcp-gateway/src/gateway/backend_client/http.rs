use std::time::Duration;

use serde_json::Value;

use super::error::BackendCallError;
use super::urls::rest_base_from_mcp_url;
use crate::gateway::admin::trace::TraceContext;
use crate::gateway::metrics::record_gateway_backend_error_kind;
use crate::gateway::resilience::{GatewayResilienceState, is_circuit_worthy_jsonrpc_error};

/// Percent-encode a URI string for use as a URL path segment.
///
/// Encodes `:`, `/`, `?`, `#`, and other chars that would be
/// misinterpreted in a URL path.  We avoid pulling in a full
/// percent-encoding crate by covering the characters that appear in
/// MCP resource URIs (`scheme://path`).
pub(super) fn percent_encode_uri(uri: &str) -> String {
    let mut out = String::with_capacity(uri.len() * 2);
    for b in uri.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            other => {
                out.push('%');
                out.push(char::from_digit((other >> 4) as u32, 16).unwrap_or('0'));
                out.push(char::from_digit((other & 0xf) as u32, 16).unwrap_or('0'));
            }
        }
    }
    out
}

/// Issue a `GET` to a backend REST endpoint and return the parsed JSON body.
///
/// Does **not** perform a readiness probe — callers that route traffic
/// to a backend have already verified it is ready.
pub(super) async fn rest_get(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .header("accept", "application/json, text/event-stream")
        .send()
        .await
        .map_err(|e| format!("{url}: transport error: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("{url}: HTTP {status}: {body}"));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| format!("{url}: invalid JSON response: {e}"))
}

/// Issue a `POST` to a backend REST endpoint with a JSON body and
/// return the parsed JSON response body.
pub(super) async fn rest_post(
    client: &reqwest::Client,
    url: &str,
    body: Value,
    timeout: Duration,
) -> Result<Value, String> {
    rest_post_with_trace_context(client, url, body, timeout, None, None).await
}

/// Issue a `POST` with optional W3C Trace Context propagation headers.
pub(super) async fn rest_post_with_trace_context(
    client: &reqwest::Client,
    url: &str,
    body: Value,
    timeout: Duration,
    request_id: Option<&str>,
    trace_context: Option<&TraceContext>,
) -> Result<Value, String> {
    let mut request = client
        .post(url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(body.to_string());
    let expected_request_id =
        request_id.or_else(|| trace_context.map(|ctx| ctx.request_id.as_str()));
    if let Some(request_id) = expected_request_id {
        request = request.header("x-request-id", request_id);
    }
    if let Some(ctx) = trace_context {
        if let Some(parent_request_id) = ctx.parent_request_id.as_deref() {
            request = request.header("x-dcc-mcp-parent-request-id", parent_request_id);
        }
        if let Some(traceparent) = ctx.traceparent() {
            request = request.header("traceparent", traceparent);
        }
        if let Some(tracestate) = ctx.trace_state.as_deref() {
            request = request.header("tracestate", tracestate);
        }
    }

    let resp = request
        .send()
        .await
        .map_err(|e| format!("{url}: transport error: {e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("{url}: invalid JSON response: {e}"))?;
    if expected_request_id.is_none() && !status.is_success() {
        return Err(format!("{url}: HTTP {status}: {body}"));
    }
    let response: Value =
        serde_json::from_str(&body).map_err(|e| format!("{url}: invalid JSON response: {e}"))?;
    if let Some(expected) = expected_request_id {
        let actual = response.get("request_id").and_then(Value::as_str);
        if actual != Some(expected) {
            return Err(format!(
                "{url}: transport desync: expected response request_id {expected:?}, got {}",
                actual.map_or_else(
                    || response
                        .get("request_id")
                        .map_or_else(|| "<missing>".to_string(), Value::to_string),
                    |value| format!("{value:?}"),
                ),
            ));
        }
    }
    if !status.is_success() {
        return Err(format!("{url}: HTTP {status}: {body}"));
    }
    Ok(response)
}

pub(super) async fn post_jsonrpc(
    client: &reqwest::Client,
    resilience: &GatewayResilienceState,
    mcp_url: &str,
    req_body: Value,
    session_id: Option<&str>,
    timeout: Duration,
) -> Result<Value, BackendCallError> {
    post_jsonrpc_with_trace_context(
        client, resilience, mcp_url, req_body, session_id, timeout, None,
    )
    .await
}

pub(super) async fn post_jsonrpc_with_trace_context(
    client: &reqwest::Client,
    resilience: &GatewayResilienceState,
    mcp_url: &str,
    req_body: Value,
    session_id: Option<&str>,
    timeout: Duration,
    trace_context: Option<&TraceContext>,
) -> Result<Value, BackendCallError> {
    let circuit_key = rest_base_from_mcp_url(mcp_url);
    let expected_id = req_body.get("id").cloned().unwrap_or(Value::Null);
    if let Err(reason) = resilience.circuits().check_open(&circuit_key) {
        let err = BackendCallError::Transport {
            mcp_url: mcp_url.to_string(),
            reason,
        };
        record_gateway_backend_error_kind(err.prometheus_error_kind());
        return Err(err);
    }

    let mut request = client
        .post(mcp_url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(req_body.to_string());
    if let Some(session_id) = session_id {
        request = request.header("Mcp-Session-Id", session_id);
    }
    if let Some(request_id) = expected_id.as_str() {
        request = request.header("x-request-id", request_id);
    }
    if let Some(ctx) = trace_context {
        if let Some(parent_request_id) = ctx.parent_request_id.as_deref() {
            request = request.header("x-dcc-mcp-parent-request-id", parent_request_id);
        }
        if let Some(traceparent) = ctx.traceparent() {
            request = request.header("traceparent", traceparent);
        }
        if let Some(tracestate) = ctx.trace_state.as_deref() {
            request = request.header("tracestate", tracestate);
        }
    }

    let resp = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            resilience.circuits().on_transport_failure(&circuit_key);
            let err = BackendCallError::Transport {
                mcp_url: mcp_url.to_string(),
                reason: e.to_string(),
            };
            record_gateway_backend_error_kind(err.prometheus_error_kind());
            return Err(err);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status().to_string();
        let body = resp.text().await.unwrap_or_default();
        let err = BackendCallError::Http {
            mcp_url: mcp_url.to_string(),
            status,
            body,
        };
        if is_circuit_worthy_jsonrpc_error(&err) {
            resilience.circuits().on_transport_failure(&circuit_key);
        } else {
            resilience.circuits().on_success(&circuit_key);
        }
        record_gateway_backend_error_kind(err.prometheus_error_kind());
        return Err(err);
    }

    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            resilience.circuits().on_transport_failure(&circuit_key);
            let err = BackendCallError::ReadBody {
                mcp_url: mcp_url.to_string(),
                reason: e.to_string(),
            };
            record_gateway_backend_error_kind(err.prometheus_error_kind());
            return Err(err);
        }
    };

    let out = parse_jsonrpc_result(mcp_url, &text, &expected_id);
    match &out {
        Ok(_) => resilience.circuits().on_success(&circuit_key),
        Err(e) => {
            if is_circuit_worthy_jsonrpc_error(e) {
                resilience.circuits().on_transport_failure(&circuit_key);
            } else {
                resilience.circuits().on_success(&circuit_key);
            }
            record_gateway_backend_error_kind(e.prometheus_error_kind());
        }
    }
    out
}

pub(super) fn parse_jsonrpc_result(
    mcp_url: &str,
    text: &str,
    expected_id: &Value,
) -> Result<Value, BackendCallError> {
    let parsed: Value = serde_json::from_str(text).map_err(|e| BackendCallError::InvalidJson {
        mcp_url: mcp_url.to_string(),
        reason: e.to_string(),
    })?;

    if parsed.get("id") != Some(expected_id) {
        return Err(BackendCallError::ResponseIdMismatch {
            mcp_url: mcp_url.to_string(),
            expected: expected_id.to_string(),
            actual: parsed
                .get("id")
                .map_or_else(|| "<missing>".to_string(), Value::to_string),
        });
    }

    if let Some(err) = parsed.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string();
        return Err(BackendCallError::Backend {
            mcp_url: mcp_url.to_string(),
            code,
            message,
        });
    }

    parsed
        .get("result")
        .cloned()
        .ok_or_else(|| BackendCallError::EmptyResult {
            mcp_url: mcp_url.to_string(),
        })
}

/// Short non-cryptographic unique ID for JSON-RPC request correlation.
///
/// We don't need uuid-level uniqueness here; the client issues one request per
/// call and reads its own response synchronously.  A timestamp-derived value is
/// enough to keep request IDs distinct in tracing.
pub(super) fn uuid_like_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    format!("gw-{n}")
}
