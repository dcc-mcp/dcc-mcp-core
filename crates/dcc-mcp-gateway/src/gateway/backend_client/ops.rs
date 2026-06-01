use std::time::Duration;

use serde_json::{Value, json};

use dcc_mcp_jsonrpc::{McpPrompt, McpTool};

use crate::gateway::admin::trace::TraceContext;
use crate::gateway::capability::CapabilityGroupInfo;
use crate::gateway::metrics::record_gateway_backend_error_kind;
use crate::gateway::resilience::{
    circuits, is_circuit_worthy_rest_error, is_retryable_rest_error, jittered_backoff,
    read_retry_max,
};

use super::error::{BackendCallError, rest_error_prometheus_kind};
use super::http::{
    percent_encode_uri, post_jsonrpc, rest_get, rest_post, rest_post_with_trace_context,
    uuid_like_id,
};
use super::probe::{ProbeOutcome, probe_mcp_readiness};
use super::urls::rest_base_from_mcp_url;

/// Check whether `action` (which may be skill-prefixed like
/// `maya_geometry__create_sphere`) matches a group tool entry
/// (typically a bare tool name like `create_sphere`).
fn action_matches_group_tool(action: &str, group_tool_name: &str) -> bool {
    if action == group_tool_name {
        return true;
    }
    // Try the bare action name (strip skill prefix) for comparison.
    dcc_mcp_gateway_core::naming::decode_skill_tool_name(action)
        .is_some_and(|(_, bare)| bare == group_tool_name)
}

#[derive(Debug, Clone)]
pub struct UnloadedCapabilityHint {
    pub skill_name: String,
    pub tool_name: String,
    pub summary: String,
    pub search_tokens: Vec<String>,
    pub available_groups: Vec<CapabilityGroupInfo>,
    pub tool_group: Option<String>,
}

async fn rest_get_idempotent(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
    backend_key: &str,
) -> Result<Value, String> {
    let max = read_retry_max();
    for attempt in 0..=max {
        if let Err(reason) = circuits().check_open(backend_key) {
            let msg = format!("{url}: {reason}");
            record_gateway_backend_error_kind(rest_error_prometheus_kind(&msg));
            return Err(msg);
        }
        match rest_get(client, url, timeout).await {
            Ok(v) => {
                circuits().on_success(backend_key);
                return Ok(v);
            }
            Err(e) => {
                let will_retry = attempt < max && is_retryable_rest_error(&e);
                if will_retry {
                    jittered_backoff(attempt).await;
                    continue;
                }
                if is_circuit_worthy_rest_error(&e) {
                    circuits().on_transport_failure(backend_key);
                } else {
                    circuits().on_success(backend_key);
                }
                record_gateway_backend_error_kind(rest_error_prometheus_kind(&e));
                return Err(e);
            }
        }
    }
    unreachable!()
}

async fn rest_post_idempotent(
    client: &reqwest::Client,
    url: &str,
    body: Value,
    timeout: Duration,
    backend_key: &str,
) -> Result<Value, String> {
    let max = read_retry_max();
    for attempt in 0..=max {
        if let Err(reason) = circuits().check_open(backend_key) {
            let msg = format!("{url}: {reason}");
            record_gateway_backend_error_kind(rest_error_prometheus_kind(&msg));
            return Err(msg);
        }
        match rest_post(client, url, body.clone(), timeout).await {
            Ok(v) => {
                circuits().on_success(backend_key);
                return Ok(v);
            }
            Err(e) => {
                let will_retry = attempt < max && is_retryable_rest_error(&e);
                if will_retry {
                    jittered_backoff(attempt).await;
                    continue;
                }
                if is_circuit_worthy_rest_error(&e) {
                    circuits().on_transport_failure(backend_key);
                } else {
                    circuits().on_success(backend_key);
                }
                record_gateway_backend_error_kind(rest_error_prometheus_kind(&e));
                return Err(e);
            }
        }
    }
    unreachable!()
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

/// Fetch tool list from a backend via `POST /v1/search` with `loaded_only=false`.
///
/// Maps each search hit to a [`McpTool`] so the capability index builder
/// receives the same type it always has.  `input_schema` is a minimal
/// `{"type":"object"}` — the builder only uses it to set `has_schema`,
/// which correctly becomes `false` for tools without declared parameters.
///
/// The `action` field from the search hit (client-safe tool name such as
/// `hello-world__greet`) is used as `McpTool.name` so the capability
/// builder receives the same bare-name input it expects.  The `slug`
/// field is ignored here — the builder recomputes the gateway-level
/// slug itself via `tool_slug(dcc_type, instance_id, callable_id)`.
///
/// Returns `(loaded_tools, unloaded_hints)`. `loaded_tools` feeds
/// [`build_records_from_backend`] as before. `unloaded_hints` contains
/// `(skill_name, tool_name, summary)` triples for every hit where the
/// backend returned `"loaded": false`; the gateway refresh layer folds
/// them into that backend instance's capability slice so `search_tools`
/// and REST `/v1/search` can surface a routable `next_step: load_skill`
/// hint even before the skill is loaded.
pub async fn try_fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Result<(Vec<McpTool>, Vec<UnloadedCapabilityHint>), String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let key = base.as_str();
    let url = format!("{base}/v1/search");
    // `/v1/search` is a POST endpoint; pass the filter params in the JSON body.
    let val = rest_post_idempotent(
        client,
        &url,
        json!({"loaded_only": false, "limit": 5000}),
        timeout,
        key,
    )
    .await?;

    let mut loaded_tools: Vec<McpTool> = Vec::new();
    let mut unloaded_hints: Vec<UnloadedCapabilityHint> = Vec::new();

    if let Some(arr) = val.get("hits").and_then(Value::as_array) {
        for v in arr {
            // Use `action` (bare tool name) as the McpTool name so the
            // capability builder's skill-extraction and slug-computation
            // logic works the same way it did with the old `tools/list`
            // JSON-RPC response.
            let Some(action) = v
                .get("action")
                .and_then(Value::as_str)
                .or_else(|| v.get("slug").and_then(Value::as_str))
                .map(str::to_owned)
            else {
                continue;
            };
            let description = v
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let has_schema = v
                .get("has_schema")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // `loaded` is `true` when the owning skill is active on this
            // instance and `false` when the backend returned metadata for an
            // unloaded skill.  Old backends that predate the field default to
            // `true` (assume loaded) so the previous behaviour is preserved.
            let loaded = v.get("loaded").and_then(Value::as_bool).unwrap_or(true);

            if loaded {
                let annotations = parse_tool_annotations(v.get("annotations"));
                let metadata = v.get("metadata");
                let mut meta = mcp_meta_from_rest_metadata(metadata, v.get("skill"));

                // Inject available_groups and per-tool group info so the
                // capability builder can surface progressive group state.
                if let Some(available_groups) = v.get("available_groups")
                    && let Some(dcc) = meta
                        .get_or_insert_with(Default::default)
                        .entry("dcc".to_string())
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                {
                    dcc.insert("available_groups".to_string(), available_groups.clone());
                    // Determine which group this tool belongs to.
                    if let Some(arr) = available_groups.as_array() {
                        for group in arr {
                            if let Some(tools) = group.get("tools").and_then(Value::as_array)
                                && tools.iter().any(|t| {
                                    t.as_str().is_some_and(|tool_name| {
                                        action_matches_group_tool(&action, tool_name)
                                    })
                                })
                            {
                                if let Some(group_name) = group.get("name").and_then(Value::as_str)
                                {
                                    dcc.insert(
                                        "group".to_string(),
                                        Value::String(group_name.to_string()),
                                    );
                                }
                                break;
                            }
                        }
                    }
                }

                loaded_tools.push(McpTool {
                    name: action,
                    description,
                    input_schema: if has_schema {
                        json!({"type": "object", "properties": {}})
                    } else {
                        json!({"type": "object"})
                    },
                    output_schema: None,
                    annotations,
                    meta,
                });
            } else {
                // Collect the skill name for unloaded hint records.  The
                // `skill` field on the search hit carries the owning skill
                // name (e.g. `"maya-primitives"`).
                let skill_name = v
                    .get("skill")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let metadata = v.get("metadata");
                let available_groups: Vec<CapabilityGroupInfo> = v
                    .get("available_groups")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_default();
                // Determine which group this tool belongs to.
                let tool_group = available_groups
                    .iter()
                    .find(|g| {
                        g.tools
                            .iter()
                            .any(|t| action_matches_group_tool(&action, t))
                    })
                    .map(|g| g.name.clone());
                unloaded_hints.push(UnloadedCapabilityHint {
                    skill_name,
                    tool_name: action,
                    summary: description,
                    search_tokens: rest_metadata_search_tokens(metadata),
                    available_groups,
                    tool_group,
                });
            }
        }
    }

    Ok((loaded_tools, unloaded_hints))
}

/// Fetch the full tool definition (including `input_schema` with properties)
/// for a single backend action via `POST /v1/describe`.
///
/// Unlike `try_fetch_tools` (which uses `/v1/search` and intentionally omits
/// schemas for token-efficiency), this endpoint returns the complete
/// `input_schema` so the gateway's `describe_tool` surface can expose it to
/// agents.
///
/// `backend_tool_slug` is the slug in the backend's own format (e.g.
/// `maya.maya-primitives.create_sphere`).  The backend resolves it via its
/// `SkillRestService::describe` method.
pub async fn try_describe_tool(
    client: &reqwest::Client,
    mcp_url: &str,
    backend_tool_slug: &str,
    timeout: Duration,
) -> Result<McpTool, String> {
    let base = rest_base_from_mcp_url(mcp_url);
    let key = base.as_str();
    let url = format!("{base}/v1/describe");
    let val = rest_post_idempotent(
        client,
        &url,
        json!({"tool_slug": backend_tool_slug, "include_schema": true}),
        timeout,
        key,
    )
    .await?;

    // The /v1/describe response shape (DescribeResponse):
    //   { "entry": { "slug", "skill", "action", "dcc", "summary", "loaded", "scope" },
    //     "description": "...",
    //     "input_schema": { ... } | null,
    //     "annotations": { ... },
    //     "metadata": { "dcc": { ... } } }
    let name = val
        .get("entry")
        .and_then(|e| e.get("action"))
        .and_then(Value::as_str)
        .unwrap_or(backend_tool_slug)
        .to_owned();
    let description = val
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let input_schema = val
        .get("input_schema")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object"}));
    let annotations = parse_tool_annotations(val.get("annotations"));
    let meta = mcp_meta_from_rest_metadata(
        val.get("metadata"),
        val.get("entry").and_then(|entry| entry.get("skill")),
    );

    Ok(McpTool {
        name,
        description,
        input_schema,
        output_schema: None,
        annotations,
        meta,
    })
}

fn parse_tool_annotations(value: Option<&Value>) -> Option<dcc_mcp_jsonrpc::McpToolAnnotations> {
    let ann: dcc_mcp_jsonrpc::McpToolAnnotations = serde_json::from_value(value?.clone()).ok()?;
    if ann.title.is_none()
        && ann.read_only_hint.is_none()
        && ann.destructive_hint.is_none()
        && ann.idempotent_hint.is_none()
        && ann.open_world_hint.is_none()
        && ann.deferred_hint.is_none()
    {
        None
    } else {
        Some(ann)
    }
}

fn mcp_meta_from_rest_metadata(
    value: Option<&Value>,
    skill: Option<&Value>,
) -> Option<serde_json::Map<String, Value>> {
    let mut map = value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(skill_name) = skill
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        let dcc = map.entry("dcc".to_string()).or_insert_with(|| json!({}));
        if let Some(dcc_obj) = dcc.as_object_mut() {
            dcc_obj
                .entry("skill".to_string())
                .or_insert_with(|| Value::String(skill_name.to_string()));
        }
    }
    (!map.is_empty()).then_some(map)
}

fn rest_metadata_search_tokens(value: Option<&Value>) -> Vec<String> {
    let Some(dcc) = value
        .and_then(Value::as_object)
        .and_then(|map| map.get("dcc"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    append_metadata_values(dcc.get("searchAliases"), "alias:", &mut out);
    append_metadata_values(dcc.get("search_aliases"), "alias:", &mut out);
    append_metadata_values(dcc.get("aliases"), "alias:", &mut out);
    append_metadata_values(dcc.get("searchTokens"), "", &mut out);
    append_metadata_values(dcc.get("search_tokens"), "", &mut out);
    out
}

fn append_metadata_values(value: Option<&Value>, prefix: &str, out: &mut Vec<String>) {
    match value {
        Some(Value::String(s)) => {
            for item in s.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    out.push(prefixed_search_token(prefix, item));
                }
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    out.push(prefixed_search_token(prefix, s));
                }
            }
        }
        _ => {}
    }
}

fn prefixed_search_token(prefix: &str, value: &str) -> String {
    if prefix.is_empty()
        || value.starts_with("alias:")
        || value.starts_with("schema:")
        || value.starts_with("required:")
    {
        value.to_string()
    } else {
        format!("{prefix}{value}")
    }
}

/// Fetch tool list from a backend; fail-soft on errors.
///
/// On any failure returns an empty vector and logs a warning — callers
/// aggregate tools across many backends and should not fail the whole
/// fan-out because one instance is unreachable.
///
/// Returns `(loaded_tools, unloaded_hints)`.  See [`try_fetch_tools`]
/// for the semantics of each component.
pub async fn fetch_tools(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> (Vec<McpTool>, Vec<UnloadedCapabilityHint>) {
    match try_fetch_tools(client, mcp_url, timeout).await {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(mcp_url = %mcp_url, error = %e, "Backend GET /v1/search failed");
            (Vec::new(), Vec::new())
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
    let key = base.as_str();
    let url = format!("{base}/v1/prompts");
    let val = rest_get_idempotent(client, &url, timeout, key).await?;
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
    let key = base.as_str();
    let url = format!("{base}/v1/resources");
    let val = rest_get_idempotent(client, &url, timeout, key).await?;
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
    let key = base.as_str();
    let encoded = percent_encode_uri(uri);
    let url = format!("{base}/v1/resources/{encoded}");
    rest_get_idempotent(client, &url, timeout, key).await
}

/// Input for forwarding a `tools/call` to a backend via `POST /v1/call`.
pub struct ForwardToolsCallRequest<'a> {
    /// Slug-form tool name (`<dcc>.<skill>.<action>`). The REST surface maps
    /// this to `tool_slug` directly.
    pub tool_name: &'a str,
    pub arguments: Option<Value>,
    pub meta: Option<Value>,
    /// Accepted for API compatibility but not forwarded; the REST surface does
    /// not use JSON-RPC request ids.
    pub request_id: Option<String>,
    pub trace_context: Option<&'a TraceContext>,
    pub traffic_capture: Option<&'a crate::gateway::traffic::TrafficCapture>,
    pub timeout: Duration,
}

/// Forward a `tools/call` to a backend via `POST /v1/call`.
pub async fn forward_tools_call(
    client: &reqwest::Client,
    mcp_url: &str,
    request: ForwardToolsCallRequest<'_>,
) -> Result<Value, String> {
    let ForwardToolsCallRequest {
        tool_name,
        arguments,
        meta,
        request_id: _request_id,
        trace_context,
        traffic_capture,
        timeout,
    } = request;
    let base = rest_base_from_mcp_url(mcp_url);
    let key = base.as_str();
    let url = format!("{base}/v1/call");
    let mut body = json!({
        "tool_slug": tool_name,
        "arguments": arguments.unwrap_or(json!({})),
    });
    if let Some(m) = meta {
        body["meta"] = m;
    }
    if let Some(capture) = traffic_capture {
        emit_backend_traffic_frame(
            capture,
            trace_context,
            &url,
            "request",
            "gateway_to_adapter",
            None,
            body.clone(),
        );
    }
    if let Err(reason) = circuits().check_open(key) {
        let msg = format!("{mcp_url}: {reason}");
        record_gateway_backend_error_kind(rest_error_prometheus_kind(&msg));
        if let Some(capture) = traffic_capture {
            emit_backend_traffic_frame(
                capture,
                trace_context,
                &url,
                "response",
                "adapter_to_gateway",
                None,
                json!({"success": false, "error": {"kind": "circuit-open", "message": msg}}),
            );
        }
        return Err(msg);
    }
    match rest_post_with_trace_context(client, &url, body, timeout, trace_context).await {
        Ok(v) => {
            circuits().on_success(key);
            if let Some(capture) = traffic_capture {
                emit_backend_traffic_frame(
                    capture,
                    trace_context,
                    &url,
                    "response",
                    "adapter_to_gateway",
                    Some(200),
                    v.clone(),
                );
            }
            Ok(v)
        }
        Err(e) => {
            if is_circuit_worthy_rest_error(&e) {
                circuits().on_transport_failure(key);
            } else {
                circuits().on_success(key);
            }
            record_gateway_backend_error_kind(rest_error_prometheus_kind(&e));
            if let Some(capture) = traffic_capture {
                emit_backend_traffic_frame(
                    capture,
                    trace_context,
                    &url,
                    "response",
                    "adapter_to_gateway",
                    None,
                    json!({"success": false, "error": {"kind": "backend-error", "message": e}}),
                );
            }
            Err(e)
        }
    }
}

fn emit_backend_traffic_frame(
    capture: &crate::gateway::traffic::TrafficCapture,
    trace_context: Option<&TraceContext>,
    url: &str,
    kind: &str,
    leg: &'static str,
    status: Option<u16>,
    body: Value,
) {
    capture.emit_json_frame(
        crate::gateway::traffic::TrafficFrame::json(
            crate::gateway::traffic::basic_gateway_source(),
            trace_correlation(trace_context, None),
            "internal",
            leg,
            "http",
            body,
        )
        .with_http(crate::gateway::traffic::http_post(url, None, status))
        .with_mcp(crate::gateway::traffic::mcp_message(
            kind,
            "tools/call",
            None,
        )),
    );
}

fn trace_correlation(trace_context: Option<&TraceContext>, session_id: Option<&str>) -> Value {
    crate::gateway::traffic::correlation(
        trace_context.map(|ctx| ctx.request_id.as_str()),
        trace_context.map(|ctx| ctx.trace_id.as_str()),
        session_id,
    )
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
    let key = base.as_str();
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
    rest_get_idempotent(client, &url, timeout, key).await
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

#[cfg(test)]
mod tests {
    use super::action_matches_group_tool;

    #[test]
    fn exact_match() {
        assert!(action_matches_group_tool("create_sphere", "create_sphere"));
    }

    #[test]
    fn skill_prefixed_action_matches_bare_group_tool() {
        // Backend action: maya_geometry__create_sphere
        // Group tool entry: create_sphere
        assert!(action_matches_group_tool(
            "maya_geometry__create_sphere",
            "create_sphere"
        ));
    }

    #[test]
    fn non_matching_tool_returns_false() {
        assert!(!action_matches_group_tool(
            "maya_geometry__create_cube",
            "create_sphere"
        ));
    }

    #[test]
    fn hyphenated_skill_prefixed_action_matches() {
        assert!(action_matches_group_tool(
            "maya-animation__set_keyframe",
            "set_keyframe"
        ));
    }

    #[test]
    fn bare_action_not_in_any_group() {
        assert!(!action_matches_group_tool(
            "standalone_action",
            "create_sphere"
        ));
    }
}
