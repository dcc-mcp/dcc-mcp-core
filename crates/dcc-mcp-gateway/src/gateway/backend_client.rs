//! HTTP client used by the gateway to talk to each backend DCC server.
//!
//! ## Architecture after #818 phase 2
//!
//! Backends are `McpHttpServer` instances listening on
//! `http://{host}:{port}`.  The gateway historically spoke MCP JSON-RPC
//! (`/mcp`) to them for every operation.  After #818 phase 2 every
//! per-backend call goes through the per-DCC REST surface (`/v1/*`)
//! instead:
//!
//! | Operation            | Was (MCP JSON-RPC)      | Now (REST)              |
//! |----------------------|-------------------------|-------------------------|
//! | list tools           | `tools/list`            | `GET  /v1/search`       |
//! | call a tool          | `tools/call`            | `POST /v1/call`         |
//! | list prompts         | `prompts/list`          | `GET  /v1/prompts`      |
//! | render a prompt      | `prompts/get`           | `GET  /v1/prompts/{n}`  |
//! | list resources       | `resources/list`        | `GET  /v1/resources`    |
//! | read a resource      | `resources/read`        | `GET  /v1/resources/{u}`|
//! | liveness             | `GET /health`           | `GET /health` (unchanged)|
//! | readiness            | `GET /v1/readyz`        | `GET /v1/readyz` (unchanged)|
//!
//! The gateway MCP client face (`/mcp`) is **unchanged** — this file
//! only affects how the gateway contacts *backends*.
//!
//! `subscribe_resource` (backed by the SSE subscriber pool) is retained
//! until #818 phase 3 when `sse_subscriber.rs` is retired.

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};

// McpTool / McpPrompt are still used by callers of fetch_tools / fetch_prompts
// (capability index, aggregator). The REST surface returns compatible JSON so
// the deserialization path is unchanged.
use dcc_mcp_jsonrpc::{McpPrompt, McpTool};
use dcc_mcp_skill_rest::ReadinessReport;

// ── URL helpers ────────────────────────────────────────────────────────

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

/// Derive the per-DCC REST base path from the MCP endpoint URL.
///
/// `http://host:port/mcp` → `http://host:port`
///
/// This is the root onto which `/v1/{search,call,prompts,resources,...}`
/// are appended.  Used by all REST-based backend calls (#818 phase 2).
pub(crate) fn rest_base_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(str::to_owned)
        .unwrap_or_else(|| mcp_url.trim_end_matches('/').to_owned())
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

#[derive(Debug)]
enum BackendCallError {
    Booting {
        mcp_url: String,
    },
    Unreachable {
        mcp_url: String,
    },
    Transport {
        mcp_url: String,
        reason: String,
    },
    Http {
        mcp_url: String,
        status: String,
        body: String,
    },
    ReadBody {
        mcp_url: String,
        reason: String,
    },
    InvalidJson {
        mcp_url: String,
        reason: String,
    },
    Backend {
        mcp_url: String,
        code: i64,
        message: String,
    },
    EmptyResult {
        mcp_url: String,
    },
}

impl fmt::Display for BackendCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Booting { mcp_url } => write!(
                f,
                "{mcp_url}: backend not ready (GET /v1/readyz reports not ready — host DCC still initialising)"
            ),
            Self::Unreachable { mcp_url } => write!(
                f,
                "{mcp_url}: not a DCC MCP HTTP endpoint (GET /v1/readyz and /health both failed)"
            ),
            Self::Transport { mcp_url, reason } => {
                write!(f, "{mcp_url}: transport error: {reason}")
            }
            Self::Http {
                mcp_url,
                status,
                body,
            } => write!(f, "{mcp_url}: HTTP {status}: {body}"),
            Self::ReadBody { mcp_url, reason } => write!(f, "{mcp_url}: read body: {reason}"),
            Self::InvalidJson { mcp_url, reason } => {
                write!(f, "{mcp_url}: invalid JSON-RPC response: {reason}")
            }
            Self::Backend {
                mcp_url,
                code,
                message,
            } => write!(f, "{mcp_url}: backend error {code}: {message}"),
            Self::EmptyResult { mcp_url } => write!(f, "{mcp_url}: empty JSON-RPC result"),
        }
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
#[cfg(test)]
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
/// Retained for `subscribe_resource` and test helpers that still use
/// MCP JSON-RPC directly. New code should use the REST helpers below.
#[allow(dead_code)]
pub async fn call_backend(
    client: &reqwest::Client,
    mcp_url: &str,
    method: &str,
    params: Option<Value>,
    request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    match probe_mcp_readiness(client, mcp_url, timeout).await {
        ProbeOutcome::Ready => {}
        ProbeOutcome::Booting => {
            return Err(BackendCallError::Booting {
                mcp_url: mcp_url.to_string(),
            }
            .to_string());
        }
        ProbeOutcome::Unreachable => {
            return Err(BackendCallError::Unreachable {
                mcp_url: mcp_url.to_string(),
            }
            .to_string());
        }
    }

    let id = request_id.unwrap_or_else(uuid_like_id);
    let req_body = {
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            body["params"] = p;
        }
        body
    };

    post_jsonrpc(client, mcp_url, req_body, None, timeout)
        .await
        .map_err(|e| e.to_string())
}

// ── REST helpers (#818 phase 2) ───────────────────────────────────────

/// Percent-encode a URI string for use as a URL path segment.
///
/// Encodes `:`, `/`, `?`, `#`, and other chars that would be
/// misinterpreted in a URL path.  We avoid pulling in a full
/// percent-encoding crate by covering the characters that appear in
/// MCP resource URIs (`scheme://path`).
fn percent_encode_uri(uri: &str) -> String {
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
async fn rest_get(client: &reqwest::Client, url: &str, timeout: Duration) -> Result<Value, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .header("accept", "application/json")
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
async fn rest_post(
    client: &reqwest::Client,
    url: &str,
    body: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let resp = client
        .post(url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_string())
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

/// Fetch tool list from a backend via `GET /v1/search?loaded_only=false&limit=5000`.
///
/// Maps each search hit to a [`McpTool`] so the capability index builder
/// receives the same type it always has.  `input_schema` is a minimal
/// `{"type":"object"}` — the builder only uses it to set `has_schema`,
/// which correctly becomes `false` for tools without declared parameters.
///
/// The `action` field from the search hit (bare tool name such as
/// `hello-world.greet`) is used as `McpTool.name` so the capability
/// builder receives the same bare-name input it expects.  The `slug`
/// field is ignored here — the builder recomputes the gateway-level
/// slug itself via `tool_slug(dcc_type, instance_id, callable_id)`.
pub async fn try_fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let url = format!("{base}/v1/search?loaded_only=false&limit=5000");
    let val = rest_get(client, &url, timeout).await?;
    Ok(val
        .get("hits")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    // Use `action` (bare tool name) as the McpTool name so
                    // the capability builder's skill-extraction and slug-
                    // computation logic works the same way it did with the
                    // old `tools/list` JSON-RPC response.
                    let action = v
                        .get("action")
                        .and_then(Value::as_str)
                        .or_else(|| v.get("slug").and_then(Value::as_str))?
                        .to_owned();
                    let description = v
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let has_schema = v
                        .get("has_schema")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    Some(McpTool {
                        name: action,
                        description,
                        input_schema: if has_schema {
                            json!({"type": "object", "properties": {}})
                        } else {
                            json!({"type": "object"})
                        },
                        output_schema: None,
                        annotations: None,
                        meta: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Fetch tool list from a backend; fail-soft on errors.
///
/// On any failure returns an empty vector and logs a warning — callers
/// aggregate tools across many backends and should not fail the whole
/// fan-out because one instance is unreachable.
pub async fn fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<McpTool> {
    match try_fetch_tools(client, mcp_url, timeout).await {
        Ok(tools) => tools,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend GET /v1/search failed");
            Vec::new()
        }
    }
}

/// Fetch prompt list from a backend via `GET /v1/prompts`.
///
/// Unlike [`fetch_prompts`], this reports transport / protocol failures
/// to callers that need deterministic errors for a specific backend.
pub async fn try_fetch_prompts(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<McpPrompt>, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let url = format!("{base}/v1/prompts");
    let val = rest_get(client, &url, timeout).await?;
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

/// Fetch prompt list from a backend; fail-soft on errors.
pub async fn fetch_prompts(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<McpPrompt> {
    match try_fetch_prompts(client, mcp_url, timeout).await {
        Ok(prompts) => prompts,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend GET /v1/prompts failed");
            Vec::new()
        }
    }
}

/// Fetch resource list from a backend via `GET /v1/resources`.
///
/// Unlike [`fetch_resources`], this reports transport / protocol failures.
pub async fn try_fetch_resources(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<Vec<Value>, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let url = format!("{base}/v1/resources");
    let val = rest_get(client, &url, timeout).await?;
    Ok(val
        .get("resources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// Fetch resource list from a backend; fail-soft on errors.
pub async fn fetch_resources(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Vec<Value> {
    match try_fetch_resources(client, mcp_url, timeout).await {
        Ok(resources) => resources,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend GET /v1/resources failed");
            Vec::new()
        }
    }
}

/// Read one resource from a backend via `GET /v1/resources/{uri}`.
///
/// The result is returned unchanged (including `contents[].blob` for
/// binary mime-types), so byte-for-byte round-trip through the gateway
/// is preserved.
pub async fn read_resource(
    client: &reqwest::Client,
    mcp_url: &str,
    uri: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let encoded = percent_encode_uri(uri);
    let url = format!("{base}/v1/resources/{encoded}");
    rest_get(client, &url, timeout).await
}

/// Forward a `tools/call` to a backend via `POST /v1/call`.
///
/// `tool_name` is already in slug form (`<dcc>.<skill>.<action>`) —
/// the REST surface maps it to `tool_slug` directly.  `request_id` is
/// accepted for API compatibility but not forwarded (the REST surface
/// does not use JSON-RPC request ids).
pub async fn forward_tools_call(
    client: &reqwest::Client,
    mcp_url: &str,
    tool_name: &str,
    arguments: Option<Value>,
    meta: Option<Value>,
    _request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let url = format!("{base}/v1/call");
    let mut body = json!({
        "tool_slug": tool_name,
        "arguments": arguments.unwrap_or(json!({})),
    });
    if let Some(m) = meta {
        body["meta"] = m;
    }
    rest_post(client, &url, body, timeout).await
}

/// Forward a `prompts/get` to a backend via `GET /v1/prompts/{name}`.
///
/// Arguments are not yet forwarded by the REST surface (phase 1a
/// deferred them); if non-empty arguments are supplied a warning is
/// logged and the request proceeds without them.
pub async fn forward_prompts_get(
    client: &reqwest::Client,
    mcp_url: &str,
    prompt_name: &str,
    arguments: Option<Value>,
    _request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    if arguments
        .as_ref()
        .is_some_and(|a| !a.is_null() && a != &json!({}))
    {
        tracing::warn!(
            mcp_url = %mcp_url,
            prompt = %prompt_name,
            "forward_prompts_get: arguments not yet forwarded by REST surface (#818 phase 1b follow-up)",
        );
    }
    let base = rest_base_from_mcp_url(mcp_url);
    let encoded = percent_encode_uri(prompt_name);
    let url = format!("{base}/v1/prompts/{encoded}");
    rest_get(client, &url, timeout).await
}

async fn post_jsonrpc(
    client: &reqwest::Client,
    mcp_url: &str,
    req_body: Value,
    session_id: Option<&str>,
    timeout: Duration,
) -> Result<Value, BackendCallError> {
    let mut request = client
        .post(mcp_url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(req_body.to_string());
    if let Some(session_id) = session_id {
        request = request.header("Mcp-Session-Id", session_id);
    }

    let resp = request
        .send()
        .await
        .map_err(|e| BackendCallError::Transport {
            mcp_url: mcp_url.to_string(),
            reason: e.to_string(),
        })?;

    if !resp.status().is_success() {
        let status = resp.status().to_string();
        let body = resp.text().await.unwrap_or_default();
        return Err(BackendCallError::Http {
            mcp_url: mcp_url.to_string(),
            status,
            body,
        });
    }

    let text = resp.text().await.map_err(|e| BackendCallError::ReadBody {
        mcp_url: mcp_url.to_string(),
        reason: e.to_string(),
    })?;

    parse_jsonrpc_result(mcp_url, &text)
}

fn parse_jsonrpc_result(mcp_url: &str, text: &str) -> Result<Value, BackendCallError> {
    let parsed: Value = serde_json::from_str(text).map_err(|e| BackendCallError::InvalidJson {
        mcp_url: mcp_url.to_string(),
        reason: e.to_string(),
    })?;

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

/// Forward a `resources/subscribe` (or `resources/unsubscribe` when `subscribe`
/// is `false`) to a backend.
///
/// `session_id` is sent as `Mcp-Session-Id` so the backend binds the
/// subscription to the gateway's long-lived SSE session — that is the
/// only stream onto which the backend will push
/// `notifications/resources/updated` for this URI (#732).
///
/// Retained until #818 phase 3 when `sse_subscriber.rs` is retired.
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
    let id = uuid_like_id();
    let req_body = {
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        body["params"] = json!({"uri": uri});
        body
    };

    post_jsonrpc(client, mcp_url, req_body, Some(session_id), timeout)
        .await
        .map_err(|e| e.to_string())
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

    /// Every `BackendCallError` variant must stringify in the format
    /// gateway callers already match on (e.g. `probe_test_refuses_forward_while_backend_is_booting`
    /// asserts on "backend not ready" + "/v1/readyz"). A future refactor
    /// must not silently drop or rephrase these markers.
    #[test]
    fn backend_call_error_display_is_stable() {
        let cases: &[(BackendCallError, &[&str])] = &[
            (
                BackendCallError::Booting {
                    mcp_url: "http://127.0.0.1:9/mcp".into(),
                },
                &[
                    "http://127.0.0.1:9/mcp",
                    "backend not ready",
                    "/v1/readyz",
                    "host DCC still initialising",
                ],
            ),
            (
                BackendCallError::Unreachable {
                    mcp_url: "http://127.0.0.1:9/mcp".into(),
                },
                &[
                    "http://127.0.0.1:9/mcp",
                    "not a DCC MCP HTTP endpoint",
                    "/v1/readyz",
                    "/health",
                ],
            ),
            (
                BackendCallError::Transport {
                    mcp_url: "http://x/mcp".into(),
                    reason: "connection refused".into(),
                },
                &["http://x/mcp", "transport error", "connection refused"],
            ),
            (
                BackendCallError::Http {
                    mcp_url: "http://x/mcp".into(),
                    status: "500 Internal Server Error".into(),
                    body: "oops".into(),
                },
                &["http://x/mcp", "HTTP ", "500 Internal Server Error", "oops"],
            ),
            (
                BackendCallError::ReadBody {
                    mcp_url: "http://x/mcp".into(),
                    reason: "eof".into(),
                },
                &["http://x/mcp", "read body", "eof"],
            ),
            (
                BackendCallError::InvalidJson {
                    mcp_url: "http://x/mcp".into(),
                    reason: "expected value".into(),
                },
                &[
                    "http://x/mcp",
                    "invalid JSON-RPC response",
                    "expected value",
                ],
            ),
            (
                BackendCallError::Backend {
                    mcp_url: "http://x/mcp".into(),
                    code: -32601,
                    message: "Method not found".into(),
                },
                &[
                    "http://x/mcp",
                    "backend error",
                    "-32601",
                    "Method not found",
                ],
            ),
            (
                BackendCallError::EmptyResult {
                    mcp_url: "http://x/mcp".into(),
                },
                &["http://x/mcp", "empty JSON-RPC result"],
            ),
        ];

        for (err, needles) in cases {
            let rendered = err.to_string();
            for needle in *needles {
                assert!(
                    rendered.contains(needle),
                    "variant {err:?} missing {needle:?} in output: {rendered}",
                );
            }
        }
    }

    /// `post_jsonrpc` forwards `session_id` as `Mcp-Session-Id`. The
    /// gateway's resource-subscribe path depends on this header reaching
    /// the backend — without it the backend would bind its
    /// `notifications/resources/updated` fan-out to the wrong SSE
    /// stream. Intercept the request via a fake backend and assert the
    /// header round-trips.
    #[tokio::test]
    async fn post_jsonrpc_forwards_session_header_when_provided() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let saw_header = Arc::new(AtomicBool::new(false));
        let saw_header_clone = saw_header.clone();
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, _body: axum::body::Bytes| {
                    let saw = saw_header_clone.clone();
                    async move {
                        if headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
                            == Some("session-abc")
                        {
                            saw.store(true, Ordering::SeqCst);
                        }
                        axum::Json(json!({"jsonrpc":"2.0","id":"x","result":{"ok":true}}))
                    }
                },
            ),
        );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let body = json!({"jsonrpc":"2.0","id":"x","method":"ping"});
        let result = post_jsonrpc(
            &client,
            &mcp_url,
            body,
            Some("session-abc"),
            Duration::from_secs(2),
        )
        .await
        .expect("post_jsonrpc must succeed against the fake backend");
        assert_eq!(result, json!({"ok": true}));
        assert!(
            saw_header.load(Ordering::SeqCst),
            "backend must observe the Mcp-Session-Id header the caller requested",
        );
        let _ = stop.send(());
    }

    /// Omitting `session_id` must NOT attach an empty `Mcp-Session-Id`
    /// header — a stray value would trigger the backend's per-session
    /// routing and cause notifications to fan out to a phantom SSE
    /// stream. Confirm absence rather than just shape.
    #[tokio::test]
    async fn post_jsonrpc_omits_session_header_when_none() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let had_header = Arc::new(AtomicBool::new(false));
        let had_header_clone = had_header.clone();
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, _body: axum::body::Bytes| {
                    let h = had_header_clone.clone();
                    async move {
                        if headers.get("mcp-session-id").is_some() {
                            h.store(true, Ordering::SeqCst);
                        }
                        axum::Json(json!({"jsonrpc":"2.0","id":"x","result":{}}))
                    }
                },
            ),
        );
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let _ = post_jsonrpc(
            &client,
            &mcp_url,
            json!({"jsonrpc":"2.0","id":"x","method":"ping"}),
            None,
            Duration::from_secs(2),
        )
        .await
        .expect("must succeed");
        assert!(
            !had_header.load(Ordering::SeqCst),
            "no session id → no Mcp-Session-Id header leaks to the backend",
        );
        let _ = stop.send(());
    }

    /// Helper used only in tests — parse a canned JSON-RPC response body the
    /// same way `call_backend` does after HTTP success, so we can unit-test the
    /// error / empty-result / tools-list-extraction branches without spinning
    /// up a real HTTP server.
    fn parse_response_body(body: &str) -> Result<Value, String> {
        let parsed: Value =
            serde_json::from_str(body).map_err(|e| format!("invalid JSON-RPC response: {e}"))?;
        if let Some(err) = parsed.get("error") {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or(-1);
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!("backend error {code}: {msg}"));
        }
        parsed
            .get("result")
            .cloned()
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

    // ── #732 / #818 integration tests: REST resource/prompt helpers ──────

    /// Helper that builds an axum router with REST endpoints for testing.
    fn rest_backend_router() -> axum::Router {
        use axum::extract::Path;
        axum::Router::new()
            .route(
                "/health",
                axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
            )
            .route("/v1/resources", axum::routing::get(|| async {
                axum::Json(json!({
                    "total": 2,
                    "resources": [
                        {"uri": "scene://current", "name": "Current scene", "mimeType": "application/json"},
                        {"uri": "capture://current_window", "name": "Window capture", "mimeType": "image/png"}
                    ]
                }))
            }))
            .route("/v1/resources/{uri}", axum::routing::get(|Path(uri): Path<String>| async move {
                axum::Json(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "image/png",
                        "blob": "aGVsbG8sIHdvcmxkIQ==",
                    }]
                }))
            }))
    }

    #[tokio::test]
    async fn try_fetch_resources_returns_backend_resources() {
        let app = rest_backend_router();
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let resources = try_fetch_resources(&client, &mcp_url, Duration::from_secs(2))
            .await
            .expect("GET /v1/resources must succeed");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0]["uri"], json!("scene://current"));
        assert_eq!(resources[1]["mimeType"], json!("image/png"));
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn fetch_resources_returns_empty_on_error() {
        // Router with no `/v1/resources` route → 404 → fail-soft empty vector.
        let app = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        );
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
        // `GET /v1/resources/{encoded_uri}` — gateway must not corrupt
        // base64 blob data on the way through.
        const BLOB_B64: &str = "aGVsbG8sIHdvcmxkIQ=="; // "hello, world!"
        let app = rest_backend_router();
        let (mcp_url, stop) = spawn_fake_backend(app).await;

        let client = reqwest::Client::new();
        let result = read_resource(
            &client,
            &mcp_url,
            "capture://current_window",
            Duration::from_secs(2),
        )
        .await
        .expect("GET /v1/resources/{uri} must succeed");
        let content = &result["contents"][0];
        assert_eq!(content["mimeType"], json!("image/png"));
        assert_eq!(content["blob"], json!(BLOB_B64));
        let _ = stop.send(());
    }

    #[tokio::test]
    async fn subscribe_resource_forwards_subscribe_and_unsubscribe_methods() {
        // subscribe_resource still uses MCP JSON-RPC (retained for phase 3).
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

    #[test]
    fn rest_base_from_mcp_url_strips_mcp_suffix() {
        assert_eq!(
            rest_base_from_mcp_url("http://127.0.0.1:64954/mcp"),
            "http://127.0.0.1:64954"
        );
        assert_eq!(
            rest_base_from_mcp_url("http://127.0.0.1:64954/mcp/"),
            "http://127.0.0.1:64954"
        );
        assert_eq!(
            rest_base_from_mcp_url("http://127.0.0.1:64954"),
            "http://127.0.0.1:64954"
        );
    }

    #[test]
    fn percent_encode_uri_encodes_colons_and_slashes() {
        let encoded = percent_encode_uri("capture://current_window");
        assert!(!encoded.contains(':'), "colon must be encoded");
        assert!(!encoded.contains('/'), "slash must be encoded");
        assert!(encoded.contains('%'), "must have percent-encoded chars");
    }
}
