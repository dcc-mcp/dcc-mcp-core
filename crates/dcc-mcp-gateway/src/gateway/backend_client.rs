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

use dcc_mcp_jsonrpc::{JsonRpcRequestBuilder, JsonRpcResponse, McpPrompt, McpTool};
use dcc_mcp_skill_rest::ReadinessReport;

/// Build the lightweight HTTP health URL that identifies a real MCP backend.
pub(crate) fn health_url_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}/health"))
        .unwrap_or_else(|| format!("{}/health", mcp_url.trim_end_matches('/')))
}

/// Build the three-state readiness URL exposed by `dcc-mcp-skill-rest`
/// (issue #660 — `GET /v1/readyz`).
///
/// Mirrors [`health_url_from_mcp_url`]: strip the trailing `/mcp` segment
/// from the JSON-RPC endpoint and append the REST path.
pub(crate) fn readyz_url_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}/v1/readyz"))
        .unwrap_or_else(|| format!("{}/v1/readyz", mcp_url.trim_end_matches('/')))
}

/// Outcome of the gateway's three-state readiness probe (#713).
///
/// * [`Ready`] — `/v1/readyz` answered `200` with all three bits
///   green, or a pre-#660 backend answered `/health`.
///   Safe to forward `tools/call`.
/// * [`Booting`] — `/v1/readyz` answered (typically `503`) with at
///   least one bit red. The process is alive, just not done
///   initialising — keep the registry row, but do **not** route
///   traffic to it.
/// * [`Unreachable`] — Neither `/v1/readyz` nor `/health` answered.
///   Eligible for the existing stale-cleanup pipeline.
///
/// [`Ready`]: ProbeOutcome::Ready
/// [`Booting`]: ProbeOutcome::Booting
/// [`Unreachable`]: ProbeOutcome::Unreachable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// Backend is fully ready.
    Ready,
    /// Backend is alive but some readiness bit is red (still booting).
    Booting,
    /// Backend answered neither `/v1/readyz` nor `/health`.
    Unreachable,
}

impl ProbeOutcome {
    /// True when the backend may service `tools/call` right now.
    pub(crate) fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// True when the backend process is alive (ready or booting).
    ///
    /// Callers use this to keep a registry row instead of marking it
    /// [`ServiceStatus::Unreachable`](dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable).
    pub(crate) fn is_alive(self) -> bool {
        matches!(self, Self::Ready | Self::Booting)
    }
}

/// Three-state probe of a backend's `/v1/readyz` surface (#713 / #660).
///
/// Returns a [`ReadinessReport`] when the backend answered `/v1/readyz`
/// with a parseable JSON body (on either `200` or `503`), and `None`
/// when the REST surface is absent — callers should then fall back to
/// the legacy `/health` check.
pub(crate) async fn probe_readiness(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Option<ReadinessReport> {
    let url = readyz_url_from_mcp_url(mcp_url);
    let resp = client
        .get(&url)
        .timeout(timeout)
        .header("accept", "application/json")
        .send()
        .await
        .ok()?;

    // `/v1/readyz` returns 200 when all three bits are green and 503 when
    // any bit is red — in **both** cases the body is a full
    // `ReadinessReport` (see `dcc-mcp-skill-rest/src/router.rs::handle_readyz`).
    // Any other status (404, 500 without body, …) means "no readiness
    // surface", not "backend is red".
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 503 {
        return None;
    }
    resp.json::<ReadinessReport>().await.ok()
}

/// Classify a backend as [`Ready`] / [`Booting`] / [`Unreachable`] using
/// the three-state probe introduced in #713.
///
/// Order of checks:
/// 1. `GET /v1/readyz` — if the backend answered (200 *or* 503 with a
///    parseable body) we trust it:
///    * `is_ready() == true`  ⇒ [`Ready`]
///    * `is_ready() == false` ⇒ [`Booting`]
/// 2. Otherwise fall back to `GET /health` for pre-#660 backends that
///    never mounted the REST surface:
///    * `200 OK`  ⇒ [`Ready`]
///    * otherwise ⇒ [`Unreachable`]
///
/// [`Ready`]: ProbeOutcome::Ready
/// [`Booting`]: ProbeOutcome::Booting
/// [`Unreachable`]: ProbeOutcome::Unreachable
pub(crate) async fn probe_mcp_readiness(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> ProbeOutcome {
    if let Some(report) = probe_readiness(client, mcp_url, timeout).await {
        return if report.is_ready() {
            ProbeOutcome::Ready
        } else {
            ProbeOutcome::Booting
        };
    }

    let health_url = health_url_from_mcp_url(mcp_url);
    let ok = client
        .get(&health_url)
        .timeout(timeout)
        .header("accept", "application/json")
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success());
    if ok {
        ProbeOutcome::Ready
    } else {
        ProbeOutcome::Unreachable
    }
}

/// Return true when the target looks like a DCC MCP HTTP server.
///
/// This is the legacy boolean wrapper kept for callers that only need a
/// live/dead classification — notably [`call_backend`] below. #713 gave
/// us three states; prefer [`probe_mcp_readiness`] in new code so
/// "alive but booting" can be distinguished from "gone".
///
/// Behaviour change under #713: the underlying check first tries
/// `/v1/readyz` and treats a non-ready (`503`) report as *not* healthy,
/// falling back to `/health` only when the readiness surface is missing.
/// A backend whose host DCC is still initialising now reports `false`
/// instead of silently routing traffic.
pub(crate) async fn probe_mcp_health(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> bool {
    probe_mcp_readiness(client, mcp_url, timeout)
        .await
        .is_ready()
}

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
    if !probe_mcp_health(client, mcp_url, timeout).await {
        // #713: disambiguate "process dead" vs "booting" — an embedded-DCC
        // backend can be alive with `/v1/readyz` still red for 10–30 s
        // while Maya's main thread finishes plugin init, and blindly
        // forwarding JSON-RPC into that window is the source of the
        // "silent queue up until timeout" bug. Re-run the three-state
        // probe once more (cheap, same cache warmth) so the error
        // message tells callers which kind of not-ready it is.
        match probe_mcp_readiness(client, mcp_url, timeout).await {
            ProbeOutcome::Booting => {
                return Err(format!(
                    "{mcp_url}: backend not ready (GET /v1/readyz reports not ready — host DCC still initialising)"
                ));
            }
            ProbeOutcome::Unreachable => {
                return Err(format!(
                    "{mcp_url}: not a DCC MCP HTTP endpoint (GET /v1/readyz and /health both failed)"
                ));
            }
            ProbeOutcome::Ready => {
                // Race between the two probes — the backend flipped to
                // green between the initial `probe_mcp_health` returning
                // false and this re-probe. Proceed with the JSON-RPC
                // call rather than return a spurious error.
            }
        }
    }

    let id = request_id.unwrap_or_else(uuid_like_id);
    let req_body = JsonRpcRequestBuilder::new(id, method)
        .with_optional_params(params)
        .to_value();

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
/// Unlike [`fetch_tools`], this reports transport / protocol failures to callers
/// that need deterministic errors for a specific backend.
pub async fn try_fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let val = call_backend(client, mcp_url, "tools/list", None, None, timeout).await?;
    Ok(val
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default())
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
    match try_fetch_tools(client, mcp_url, timeout).await {
        Ok(tools) => tools,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend tools/list failed");
            Vec::new()
        }
    }
}

/// Fetch `prompts/list` from a backend and return the deserialised [`McpPrompt`] list.
///
/// Unlike [`fetch_prompts`], this reports transport / protocol failures to callers
/// that need deterministic errors for a specific backend. Mirrors
/// [`try_fetch_tools`] for the prompts primitive (issue #731).
pub async fn try_fetch_prompts(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<McpPrompt>, String> {
    let val = call_backend(client, mcp_url, "prompts/list", None, None, timeout).await?;
    Ok(val
        .get("prompts")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<McpPrompt>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default())
}

/// Fetch `prompts/list` from a backend and return the deserialised [`McpPrompt`] list.
///
/// On any failure returns an empty vector and logs a warning — mirrors
/// [`fetch_tools`] so the prompts aggregator can fan out fail-soft across
/// every live backend (issue #731).
pub async fn fetch_prompts(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<McpPrompt> {
    match try_fetch_prompts(client, mcp_url, timeout).await {
        Ok(prompts) => prompts,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend prompts/list failed");
            Vec::new()
        }
    }
}

/// Fetch `resources/list` from a backend and return the raw `resources` array.
///
/// Unlike [`fetch_resources`], this reports transport / protocol failures to callers
/// that need deterministic errors for a specific backend.
pub async fn try_fetch_resources(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<Value>, String> {
    let val = call_backend(client, mcp_url, "resources/list", None, None, timeout).await?;
    Ok(val
        .get("resources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// Fetch `resources/list` from a backend and return the raw `resources` array.
///
/// On any failure returns an empty vector and logs a warning — callers aggregate
/// resources across many backends and should not fail the whole fan-out because
/// one instance is unreachable. Mirrors [`fetch_tools`] for resources.
pub async fn fetch_resources(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<Value> {
    match try_fetch_resources(client, mcp_url, timeout).await {
        Ok(resources) => resources,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend resources/list failed");
            Vec::new()
        }
    }
}

/// Forward a `resources/read` to a backend and return the raw `result` JSON.
///
/// The result is returned unchanged (including `contents[].blob` entries for
/// binary mime-types), so byte-for-byte round-trip through the gateway is
/// preserved.
pub async fn read_resource(
    client: &reqwest::Client,
    mcp_url: &str,
    uri: &str,
    timeout: Duration,
) -> Result<Value, String> {
    call_backend(
        client,
        mcp_url,
        "resources/read",
        Some(json!({"uri": uri})),
        None,
        timeout,
    )
    .await
}

/// Forward a `resources/subscribe` (or `resources/unsubscribe` when `subscribe`
/// is `false`) to a backend.
///
/// `session_id` is sent as `Mcp-Session-Id` so the backend binds the
/// subscription to the gateway's long-lived SSE session — that is the
/// only stream onto which the backend will push
/// `notifications/resources/updated` for this URI (#732).
///
/// Returns the raw `result` JSON — typically `{}`.
pub async fn subscribe_resource(
    client: &reqwest::Client,
    mcp_url: &str,
    uri: &str,
    subscribe: bool,
    session_id: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let method = if subscribe {
        "resources/subscribe"
    } else {
        "resources/unsubscribe"
    };
    let req_body = JsonRpcRequestBuilder::new(uuid_like_id(), method)
        .with_optional_params(Some(json!({"uri": uri})))
        .to_value();

    let resp = client
        .post(mcp_url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("Mcp-Session-Id", session_id)
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

/// Forward a `prompts/get` to a backend and return the raw result JSON
/// (issue #731).
///
/// `prompt_name` is the **backend-local** prompt name — callers must decode
/// the gateway-prefixed wire name with [`super::namespace::decode_tool_name`]
/// before invoking this helper, so the request that reaches the backend
/// carries the same name the backend published in `prompts/list`.
pub async fn forward_prompts_get(
    client: &reqwest::Client,
    mcp_url: &str,
    prompt_name: &str,
    arguments: Option<Value>,
    request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut params = json!({ "name": prompt_name });
    if let Some(args) = arguments {
        params["arguments"] = args;
    }
    call_backend(
        client,
        mcp_url,
        "prompts/get",
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

    #[test]
    fn builds_health_url_from_mcp_url() {
        assert_eq!(
            health_url_from_mcp_url("http://127.0.0.1:64954/mcp"),
            "http://127.0.0.1:64954/health"
        );
        assert_eq!(
            health_url_from_mcp_url("http://127.0.0.1:64954/mcp/"),
            "http://127.0.0.1:64954/health"
        );
    }

    #[test]
    fn builds_readyz_url_from_mcp_url() {
        // Standard /mcp suffix is stripped and replaced with /v1/readyz.
        assert_eq!(
            readyz_url_from_mcp_url("http://127.0.0.1:64954/mcp"),
            "http://127.0.0.1:64954/v1/readyz"
        );
        // Trailing slash after /mcp is handled identically.
        assert_eq!(
            readyz_url_from_mcp_url("http://127.0.0.1:64954/mcp/"),
            "http://127.0.0.1:64954/v1/readyz"
        );
        // URL without the /mcp suffix appends the readyz path as-is
        // (same fallback semantics as health_url_from_mcp_url).
        assert_eq!(
            readyz_url_from_mcp_url("http://127.0.0.1:64954"),
            "http://127.0.0.1:64954/v1/readyz"
        );
    }

    #[test]
    fn probe_outcome_is_ready_and_is_alive() {
        // `Ready` is the only state that routes traffic.
        assert!(ProbeOutcome::Ready.is_ready());
        assert!(!ProbeOutcome::Booting.is_ready());
        assert!(!ProbeOutcome::Unreachable.is_ready());

        // `Booting` is alive (keep registry row) but not ready.
        assert!(ProbeOutcome::Ready.is_alive());
        assert!(ProbeOutcome::Booting.is_alive());
        assert!(!ProbeOutcome::Unreachable.is_alive());
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

    // ── #713 integration tests: three-state readiness probe ──────────────
    //
    // Each test spins up a tiny axum server with one or more of
    // `/v1/readyz`, `/v1/readyz` (red), `/health` mounted, then exercises
    // the new `probe_readiness` / `probe_mcp_readiness` / `probe_mcp_health`
    // helpers through a real `reqwest::Client` to confirm the wire-level
    // contract. These are integration-style but live in the unit test
    // module so they share `#[tokio::test]` machinery and don't need a
    // separate `tests/` harness crate.

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Spawn a short-lived axum server on `127.0.0.1:0`, return the bound
    /// `mcp_url` and a oneshot sender the caller uses to stop the server.
    async fn spawn_fake_backend(app: axum::Router) -> (String, tokio::sync::oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
                .ok();
        });
        (format!("http://127.0.0.1:{port}/mcp"), tx)
    }

    #[tokio::test]
    async fn probe_readiness_parses_200_green_report() {
        let app = axum::Router::new().route(
            "/v1/readyz",
            axum::routing::get(|| async {
                axum::Json(json!({
                    "process": true,
                    "dispatcher": true,
                    "dcc": true,
                }))
            }),
        );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let report = probe_readiness(&client, &mcp_url, Duration::from_secs(2))
            .await
            .expect("readyz should answer");
        assert!(report.is_ready(), "all three bits green -> is_ready()");
        assert_eq!(
            probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
            ProbeOutcome::Ready
        );
        assert!(probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn probe_readiness_parses_503_red_report_as_booting() {
        // `dcc-mcp-skill-rest` returns 503 with a full ReadinessReport body
        // when any bit is red — we must parse that body, not treat it as
        // "no readiness surface".
        let app = axum::Router::new().route(
            "/v1/readyz",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(json!({
                        "process": true,
                        "dispatcher": true,
                        "dcc": false,
                    })),
                )
            }),
        );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let report = probe_readiness(&client, &mcp_url, Duration::from_secs(2))
            .await
            .expect("red readyz still returns a parseable body");
        assert!(!report.is_ready());
        assert!(report.process);
        assert!(!report.dcc);

        let outcome = probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await;
        assert_eq!(outcome, ProbeOutcome::Booting);
        assert!(outcome.is_alive(), "booting backends stay in the registry");
        assert!(
            !outcome.is_ready(),
            "booting backends must not receive tools/call"
        );
        assert!(!probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn probe_mcp_readiness_falls_back_to_health_when_readyz_missing() {
        // Pre-#660 backend: only `/health` is mounted. The three-state
        // probe should still report Ready so existing deployments don't
        // regress.
        let app = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        assert!(
            probe_readiness(&client, &mcp_url, Duration::from_secs(2))
                .await
                .is_none(),
            "no /v1/readyz -> probe_readiness returns None"
        );
        assert_eq!(
            probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
            ProbeOutcome::Ready
        );
        assert!(probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn probe_mcp_readiness_returns_unreachable_when_nothing_answers() {
        // Empty router: 404 on every path, which mimics an HTTP endpoint
        // that isn't a DCC backend at all (the exact case that used to
        // trigger the false-positive WARN in the issue trace).
        let app = axum::Router::new();
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        assert_eq!(
            probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
            ProbeOutcome::Unreachable
        );
        assert!(!probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn call_backend_refuses_forward_while_backend_is_booting() {
        // Acceptance criterion: when /v1/readyz reports red the gateway
        // must NOT post JSON-RPC to /mcp. We assert by mounting a /mcp
        // handler that flips a flag if hit — and then checking the flag
        // is still false.
        let hit = Arc::new(AtomicBool::new(false));
        let hit_clone = hit.clone();
        let app = axum::Router::new()
            .route(
                "/v1/readyz",
                axum::routing::get(|| async {
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        axum::Json(json!({
                            "process": true,
                            "dispatcher": false,
                            "dcc": false,
                        })),
                    )
                }),
            )
            .route(
                "/mcp",
                axum::routing::post(move || {
                    let hit = hit_clone.clone();
                    async move {
                        hit.store(true, Ordering::SeqCst);
                        axum::Json(json!({"jsonrpc":"2.0","id":"gw-x","result":{}}))
                    }
                }),
            );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let err = call_backend(
            &client,
            &mcp_url,
            "tools/list",
            None,
            None,
            Duration::from_secs(2),
        )
        .await
        .expect_err("booting backend must surface an error");
        assert!(
            err.contains("backend not ready") && err.contains("/v1/readyz"),
            "expected booting diagnostic, got: {err}"
        );
        assert!(
            !hit.load(Ordering::SeqCst),
            "call_backend must not post to /mcp while backend is red"
        );
        let _ = stop.send(());
    }

    // ── #732 integration tests: resources forwarding helpers ─────────────

    /// Router that answers `/health` green plus a single JSON-RPC method
    /// handler at `/mcp`. Keeps test routers compact.
    fn healthy_mcp_router<H, Fut>(handler: H) -> axum::Router
    where
        H: Fn(axum::Json<Value>) -> Fut + Clone + Send + Sync + 'static,
        Fut: std::future::Future<Output = axum::Json<Value>> + Send,
    {
        axum::Router::new()
            .route(
                "/health",
                axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
            )
            .route(
                "/mcp",
                axum::routing::post(move |body: axum::Json<Value>| {
                    let handler = handler.clone();
                    async move { handler(body).await }
                }),
            )
    }

    #[tokio::test]
    async fn try_fetch_resources_returns_backend_resources() {
        let app = healthy_mcp_router(|body: axum::Json<Value>| async move {
            assert_eq!(
                body.get("method").and_then(|m| m.as_str()),
                Some("resources/list")
            );
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(json!("gw-test")),
                "result": {
                    "resources": [
                        {"uri": "scene://current", "name": "Current scene", "mimeType": "application/json"},
                        {"uri": "capture://current_window", "name": "Window capture", "mimeType": "image/png"}
                    ]
                }
            }))
        });
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let resources = try_fetch_resources(&client, &mcp_url, Duration::from_secs(2))
            .await
            .expect("resources/list must succeed");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0]["uri"], json!("scene://current"));
        assert_eq!(resources[1]["mimeType"], json!("image/png"));
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn fetch_resources_returns_empty_on_error() {
        // Backend responds with an error envelope — fail-soft contract
        // says: swallow the error, log a warn, return empty vector.
        let app = healthy_mcp_router(|body: axum::Json<Value>| async move {
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(json!("gw-test")),
                "error": {"code": -32601, "message": "Method not found"}
            }))
        });
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let resources = fetch_resources(&client, &mcp_url, Duration::from_secs(2)).await;
        assert!(
            resources.is_empty(),
            "fetch_resources must fail-soft to an empty vector"
        );
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn read_resource_preserves_blob_bytes() {
        // A capture://current_window response carries a base64 `blob` —
        // the gateway must not corrupt it on the way through.
        const BLOB_B64: &str = "aGVsbG8sIHdvcmxkIQ=="; // "hello, world!"
        let app = healthy_mcp_router(move |body: axum::Json<Value>| async move {
            assert_eq!(
                body.get("method").and_then(|m| m.as_str()),
                Some("resources/read")
            );
            assert_eq!(
                body.get("params")
                    .and_then(|p| p.get("uri"))
                    .and_then(|u| u.as_str()),
                Some("capture://current_window")
            );
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(json!("gw-test")),
                "result": {
                    "contents": [{
                        "uri": "capture://current_window",
                        "mimeType": "image/png",
                        "blob": BLOB_B64,
                    }]
                }
            }))
        });
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let result = read_resource(
            &client,
            &mcp_url,
            "capture://current_window",
            Duration::from_secs(2),
        )
        .await
        .expect("resources/read must succeed");
        let content = &result["contents"][0];
        assert_eq!(content["mimeType"], json!("image/png"));
        assert_eq!(content["blob"], json!(BLOB_B64));
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn subscribe_resource_forwards_subscribe_and_unsubscribe_methods() {
        // Verify the helper uses the correct method name, payload, and
        // Mcp-Session-Id header for both subscribe and unsubscribe.
        let hits = Arc::new(parking_lot::Mutex::new(
            Vec::<(String, Option<String>)>::new(),
        ));
        let hits_clone = hits.clone();
        let app = axum::Router::new()
            .route(
                "/health",
                axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
            )
            .route(
                "/mcp",
                axum::routing::post(
                    move |headers: axum::http::HeaderMap, body: axum::Json<Value>| {
                        let hits = hits_clone.clone();
                        async move {
                            let method = body
                                .get("method")
                                .and_then(|m| m.as_str())
                                .unwrap_or("")
                                .to_owned();
                            let session = headers
                                .get("mcp-session-id")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned);
                            hits.lock().push((method, session));
                            axum::Json(json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or(json!("gw-test")),
                                "result": {}
                            }))
                        }
                    },
                ),
            );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        subscribe_resource(
            &client,
            &mcp_url,
            "scene://current",
            true,
            "gw-sub-abc123",
            Duration::from_secs(2),
        )
        .await
        .expect("subscribe must succeed");
        subscribe_resource(
            &client,
            &mcp_url,
            "scene://current",
            false,
            "gw-sub-abc123",
            Duration::from_secs(2),
        )
        .await
        .expect("unsubscribe must succeed");

        let recorded = hits.lock().clone();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].0, "resources/subscribe");
        assert_eq!(recorded[1].0, "resources/unsubscribe");
        assert_eq!(
            recorded[0].1.as_deref(),
            Some("gw-sub-abc123"),
            "Mcp-Session-Id must be forwarded on subscribe",
        );
        assert_eq!(
            recorded[1].1.as_deref(),
            Some("gw-sub-abc123"),
            "Mcp-Session-Id must be forwarded on unsubscribe",
        );
        let _ = stop.send(());
    }
}
