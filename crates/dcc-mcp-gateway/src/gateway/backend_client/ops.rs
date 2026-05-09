use std::time::Duration;

use serde_json::{Value, json};

use dcc_mcp_jsonrpc::{McpPrompt, McpTool};

use super::error::BackendCallError;
use super::http::{percent_encode_uri, post_jsonrpc, rest_get, rest_post, uuid_like_id};
use super::probe::{ProbeOutcome, probe_mcp_readiness};
use super::urls::rest_base_from_mcp_url;

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

/// Fetch tool list from a backend via `POST /v1/search` with `loaded_only=false`.
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
    let url = format!("{base}/v1/search");
    // `/v1/search` is a POST endpoint; pass the filter params in the JSON body.
    let val = rest_post(
        client,
        &url,
        json!({"loaded_only": false, "limit": 5000}),
        timeout,
    )
    .await?;
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
/// Prompt arguments are encoded as a compact JSON object in the `args`
/// query parameter so the REST hop preserves the MCP `prompts/get`
/// contract without falling back to backend JSON-RPC.
pub async fn forward_prompts_get(
    client: &reqwest::Client,
    mcp_url: &str,
    prompt_name: &str,
    arguments: Option<Value>,
    _request_id: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let encoded = percent_encode_uri(prompt_name);
    let mut url = format!("{base}/v1/prompts/{encoded}");
    if let Some(args) = arguments
        .as_ref()
        .filter(|a| !a.is_null() && **a != json!({}))
    {
        let encoded_args = serde_json::to_string(args)
            .map(|raw| percent_encode_uri(&raw))
            .map_err(|e| format!("failed to encode prompt arguments for {prompt_name}: {e}"))?;
        url.push_str("?args=");
        url.push_str(&encoded_args);
    }
    rest_get(client, &url, timeout).await
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
