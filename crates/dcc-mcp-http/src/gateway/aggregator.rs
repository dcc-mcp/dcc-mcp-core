//! Tools-list aggregation and tools-call routing for the facade gateway.
//!
//! This module is the core of the "one endpoint, every DCC" façade:
//!
//! * `aggregate_tools_list` — fan out `tools/list` to every live backend and
//!   merge the results.  Backend-provided tools get an instance prefix so
//!   identical tool names across multiple DCCs never clash (see [`namespace`]).
//! * `route_tools_call` — dispatch a `tools/call` based on the tool name:
//!   - Meta / skill-management tools are handled locally or fanned-out with
//!     instance-scoped semantics.
//!   - Prefixed tools are forwarded to the backend that owns them.
//!
//! All network I/O goes through the stateless helpers in
//! [`super::backend_client`], so fan-out works concurrently via `join_all`.

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::backend_client::{fetch_tools, forward_tools_call};
use super::namespace::{decode_tool_name, encode_tool_name, instance_short, is_local_tool};
use super::state::GatewayState;
use super::tools::{
    gateway_tool_defs, tool_connect_to_dcc, tool_get_instance, tool_list_instances,
};
use crate::protocol::{TOOLS_LIST_PAGE_SIZE, decode_cursor, encode_cursor};
use dcc_mcp_transport::discovery::types::ServiceEntry;

/// Terminal job statuses that end a wait-for-terminal block (#321).
///
/// Mirrors the backend's [`crate::job::JobStatus`] terminal states; the
/// gateway does not import the enum directly to keep the dependency
/// graph flat.
const TERMINAL_JOB_STATUSES: &[&str] = &["completed", "failed", "cancelled", "interrupted"];

/// Build the unified `tools/list` result by aggregating every live backend.
///
/// Tool order:
/// 1. Gateway discovery meta-tools (`list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`).
/// 2. Skill-management tools (one canonical set for the whole gateway).
/// 3. Backend-provided tools from every live instance, prefixed with the
///    8-char instance id, annotated with `_instance_id` / `_dcc_type` in the
///    tool's `annotations` map so agents can display origin context.
///
/// Pagination uses the same cursor scheme as the per-DCC server:
/// `cursor` is an opaque hex-encoded offset into the flat tool list.
pub async fn aggregate_tools_list(gs: &GatewayState, cursor: Option<&str>) -> Value {
    let mut tools: Vec<Value> = Vec::new();

    // Tier 1 + 2: local gateway tools (meta + skill management).
    if let Value::Array(local) = gateway_tool_defs() {
        tools.extend(local);
    }
    tools.extend(skill_management_tool_defs());

    // Tier 3: fan out to every live backend.
    let instances = live_backends(gs).await;
    let client = &gs.http_client;
    let backend_timeout = gs.backend_timeout;
    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let backend_tools = fetch_tools(client, &url, backend_timeout).await;
        (entry.instance_id, entry.dcc_type.clone(), backend_tools)
    });
    let results = join_all(futs).await;

    for (iid, dcc_type, backend_tools) in results {
        for mut tool in backend_tools {
            // Skip any tool whose name would collide with a gateway-local name
            // AFTER encoding — cannot happen today because local tools are
            // already filtered by `is_local_tool`, but guard defensively.
            if is_local_tool(&tool.name) {
                continue;
            }
            let encoded = encode_tool_name(&iid, &tool.name);
            tool.name = encoded;
            let mut json_val = serde_json::to_value(&tool).unwrap_or(Value::Null);
            inject_instance_metadata(&mut json_val, &iid, &dcc_type);
            tools.push(json_val);
        }
    }

    // ── Pagination ───────────────────────────────────────────────────────
    let offset = cursor.and_then(decode_cursor).unwrap_or(0);
    let total = tools.len();
    let page_end = (offset + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<Value> = if offset < total {
        tools.drain(offset..page_end).collect()
    } else {
        Vec::new()
    };

    let mut result = json!({"tools": page});
    if page_end < total {
        result["nextCursor"] = json!(encode_cursor(page_end));
    }
    result
}

/// Dispatch a gateway `tools/call` to the right place.
///
/// Returns `(text_body, is_error)` so the caller can wrap into an MCP
/// `CallToolResult`.  Agents never see backend URLs; results look identical
/// to those produced by a single-DCC server.
pub async fn route_tools_call(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
    meta: Option<&Value>,
    request_id: Option<String>,
    client_session_id: Option<&str>,
) -> (String, bool) {
    // ── Local meta-tools ────────────────────────────────────────────────
    match tool {
        "list_dcc_instances" => return to_text_result(tool_list_instances(gs, args).await),
        "get_dcc_instance" => return to_text_result(tool_get_instance(gs, args).await),
        "connect_to_dcc" => return to_text_result(tool_connect_to_dcc(gs, args).await),
        _ => {}
    }

    // ── Skill-management tools ──────────────────────────────────────────
    if matches!(
        tool,
        "list_skills"
            | "find_skills"
            | "search_skills"
            | "get_skill_info"
            | "load_skill"
            | "unload_skill"
    ) {
        return skill_mgmt_dispatch(gs, tool, args).await;
    }

    // ── Prefixed backend tool ───────────────────────────────────────────
    let Some((prefix, original)) = decode_tool_name(tool) else {
        return (
            format!(
                "Unknown tool: {tool}. Call list_dcc_instances or search_skills to discover tools."
            ),
            true,
        );
    };

    let Some(entry) = find_instance_by_prefix(gs, prefix).await else {
        return (
            format!("No live DCC instance matches prefix '{prefix}' in tool '{tool}'."),
            true,
        );
    };

    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
    // Update the pending-calls map with the real backend URL so that a
    // later notifications/cancelled can be forwarded to the right server.
    if let Some(ref rid) = request_id {
        let mut pending = gs.pending_calls.write().await;
        if let Some(call) = pending.get_mut(rid) {
            call.backend_url = url.clone();
        }
    }

    // ── #320: wire SSE correlation ─────────────────────────────────────
    // (a) Ensure the backend has an SSE subscriber so notifications
    //     produced during this call are captured.
    // (b) If the caller supplied `_meta.progressToken`, remember the
    //     session → token mapping so `notifications/progress` from the
    //     backend can be routed back here.
    gs.subscriber.ensure_subscribed(&url);
    if let (Some(sid), Some(m)) = (client_session_id, meta) {
        if let Some(token) = m.get("progressToken") {
            gs.subscriber.bind_progress_token(token, sid);
        }
    }

    // ── #321: pick the right timeout ──────────────────────────────────
    // Async-opt-in calls may legitimately take longer than the short
    // sync `backend_timeout` just to queue the job. Bump to
    // `async_dispatch_timeout` when any of the opt-in signals fire.
    let async_opt_in = meta_signals_async_dispatch(meta);
    let dispatch_timeout = if async_opt_in {
        gs.async_dispatch_timeout
    } else {
        gs.backend_timeout
    };
    let wait_for_terminal = async_opt_in && meta_wants_wait_for_terminal(meta);

    // The outbound body must not carry `_meta.dcc.wait_for_terminal` —
    // that flag is gateway-local bookkeeping, not a backend contract.
    let forwarded_meta = meta.cloned().map(strip_gateway_meta_flags);

    let forward = forward_tools_call(
        &gs.http_client,
        &url,
        original,
        Some(args.clone()),
        forwarded_meta,
        request_id,
        dispatch_timeout,
    )
    .await;

    match forward {
        Ok(mut result) => {
            // (c) Backend reply may carry `_meta.dcc.jobId` (async job
            //     dispatch path, #318) or `structuredContent.job_id`.
            //     Either way, bind the job → session mapping so later
            //     `notifications/$/dcc.jobUpdated` arriving over SSE can
            //     be routed to the originating client session.
            let job_id = extract_job_id(&result);
            if let (Some(sid), Some(jid)) = (client_session_id, job_id.as_deref()) {
                gs.subscriber.bind_job(jid, sid, &url);
            }

            // ── #321: wait-for-terminal passthrough ────────────────────
            if wait_for_terminal {
                if let Some(jid) = job_id.as_deref() {
                    return wait_for_terminal_reply(
                        gs,
                        jid,
                        &mut result,
                        &entry,
                        gs.wait_terminal_timeout,
                    )
                    .await;
                }
                // Synchronous reply on an async-opt-in path: nothing to
                // wait for — fall through and return the envelope as-is.
            }

            inject_instance_metadata(&mut result, &entry.instance_id, &entry.dcc_type);
            envelope_to_text_result(&result)
        }
        Err(e) => {
            if async_opt_in && e.contains("timeout") {
                // Backend was unresponsive while we tried to queue the
                // job. Surface a JSON-RPC style `-32000 backend
                // unresponsive` payload so clients can distinguish it
                // from a legitimate tool error.
                let payload = json!({
                    "error": {
                        "code": -32000,
                        "message": format!(
                            "backend unresponsive ({}): {e}",
                            entry.dcc_type
                        ),
                        "data": {
                            "instance_id": entry.instance_id.to_string(),
                            "dcc_type": entry.dcc_type,
                        }
                    }
                });
                (
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                    true,
                )
            } else {
                (format!("Backend call failed: {e}"), true)
            }
        }
    }
}

/// Common envelope-to-text extraction used by both the sync and wait-
/// for-terminal paths. Keeps the gateway's response shape a single
/// `CallToolResult` rather than a nested envelope.
fn envelope_to_text_result(result: &Value) -> (String, bool) {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(result).unwrap_or_default());
    (text, is_error)
}

/// Detect whether the outbound `tools/call` has signalled async
/// dispatch opt-in (#318 + #321). Any of the three signals listed in
/// the backend handler (`handler.rs::should_dispatch_async`) triggers
/// the longer gateway timeout — we do NOT need to consult the tool's
/// `ActionMeta` here because the backend will do so itself; if none of
/// these signals are present the call will always be synchronous and
/// the short timeout is correct.
fn meta_signals_async_dispatch(meta: Option<&Value>) -> bool {
    let Some(m) = meta else {
        return false;
    };
    let async_flag = m
        .get("dcc")
        .and_then(|d| d.get("async"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_progress_token = m.get("progressToken").is_some();
    async_flag || has_progress_token
}

/// Detect the `_meta.dcc.wait_for_terminal = true` opt-in (#321).
fn meta_wants_wait_for_terminal(meta: Option<&Value>) -> bool {
    meta.and_then(|m| m.get("dcc"))
        .and_then(|d| d.get("wait_for_terminal"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Remove gateway-only bookkeeping keys from a `_meta` value before we
/// forward it to the backend (`wait_for_terminal` is useless to the
/// backend — keep the wire protocol clean).
fn strip_gateway_meta_flags(mut meta: Value) -> Value {
    if let Some(dcc) = meta.get_mut("dcc").and_then(Value::as_object_mut) {
        dcc.remove("wait_for_terminal");
    }
    meta
}

/// Block the gateway's `tools/call` response until the backend reports
/// a terminal `$/dcc.jobUpdated` for `job_id`, or until the
/// [`GatewayState::wait_terminal_timeout`] elapses.
///
/// Returns the same `(text, is_error)` shape as the synchronous path so
/// the caller's wrapping into `CallToolResult` is identical.
///
/// On timeout we return the **initial `{pending}` envelope annotated
/// with `_meta.dcc.timed_out = true`** and leave the job running on the
/// backend — the caller can keep polling `jobs.get_status` or reconnect
/// SSE to collect the result later.
async fn wait_for_terminal_reply(
    gs: &GatewayState,
    job_id: &str,
    pending_envelope: &mut Value,
    entry: &ServiceEntry,
    timeout: Duration,
) -> (String, bool) {
    // Subscribe BEFORE we return to the caller — the publish happens
    // inside [`SubscriberManager::deliver`] regardless of any
    // client-session binding, so the only race window we need to
    // defend against is between "backend replied {pending}" and "we
    // call `.recv()` below". Binding happened in the caller via
    // `bind_job`, but the bus is independent — create it here.
    let mut rx: broadcast::Receiver<Value> = gs.subscriber.job_event_channel(job_id);

    // Capture the latest-seen job update so that on timeout we can
    // return the richest envelope we observed.
    let mut latest: Option<Value> = None;
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(value)) => {
                let status = value
                    .get("params")
                    .and_then(|p| p.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let is_terminal = TERMINAL_JOB_STATUSES
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(status));
                latest = Some(value);
                if is_terminal {
                    // Retire per-job bus + routing (best-effort — we may
                    // have an in-flight notification still buffered, but
                    // the waiter has consumed the terminal event).
                    gs.subscriber.forget_job_bus(job_id);
                    gs.subscriber.forget_job(job_id);
                    break;
                }
            }
            // Broadcast lag: the backend emitted notifications faster
            // than we could consume them. Keep going; the next call
            // will deliver the most recent events.
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            // Sender was dropped — the subscriber's backend loop tore
            // down. This is NOT a terminal state; surface a clear
            // error so the client knows the job is in limbo on the
            // backend. (#328 will later mark it `interrupted`.)
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                gs.subscriber.forget_job_bus(job_id);
                let payload = json!({
                    "error": {
                        "code": -32000,
                        "message": format!(
                            "backend disconnected during wait_for_terminal \
                             (job {job_id} still running on {})",
                            entry.dcc_type
                        ),
                        "data": {
                            "job_id": job_id,
                            "instance_id": entry.instance_id.to_string(),
                            "dcc_type": entry.dcc_type,
                        }
                    }
                });
                return (
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                    true,
                );
            }
            // Per-iteration timeout — fall through to check deadline.
            Err(_) => break,
        }
    }

    // If we have a terminal event, build the final envelope by merging
    // the backend's job-update payload into the pending envelope.
    let envelope = match latest {
        Some(update) => merge_job_update_into_envelope(pending_envelope.clone(), &update, false),
        None => {
            // Timed out before any update arrived. Tag the pending
            // envelope so the client can distinguish "still running"
            // from "completed with empty output".
            gs.subscriber.forget_job_bus(job_id);
            merge_job_update_into_envelope(pending_envelope.clone(), &Value::Null, true)
        }
    };

    let mut final_envelope = envelope;
    inject_instance_metadata(&mut final_envelope, &entry.instance_id, &entry.dcc_type);
    envelope_to_text_result(&final_envelope)
}

/// Compose a terminal-state `CallToolResult` by layering:
/// 1. The backend's original `{pending, job_id}` envelope (preserves
///    `_meta.dcc.jobId`, `parentJobId`).
/// 2. The `$/dcc.jobUpdated` payload's `status`, `result` (if present),
///    and `error` (if present).
/// 3. Gateway flags — `_meta.dcc.timed_out` when we couldn't wait any
///    longer.
///
/// The output is a JSON object shaped like a `CallToolResult` so the
/// caller can reuse [`envelope_to_text_result`].
fn merge_job_update_into_envelope(mut pending: Value, update: &Value, timed_out: bool) -> Value {
    let params = update.get("params");
    let status = params
        .and_then(|p| p.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let error_text = params
        .and_then(|p| p.get("error"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let result_value = params.and_then(|p| p.get("result")).cloned();

    // Build structuredContent payload: reuse the pending object, then
    // overwrite status + result.
    let mut sc = pending
        .get("structuredContent")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if !status.is_empty() {
        sc.insert("status".to_string(), Value::String(status.to_string()));
    }
    if let Some(r) = result_value {
        sc.insert("result".to_string(), r);
    }
    if let Some(ref e) = error_text {
        sc.insert("error".to_string(), Value::String(e.clone()));
    }

    // Merge _meta.
    let mut meta = sc
        .get("_meta")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let mut dcc_meta = meta
        .get("dcc")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if !status.is_empty() {
        dcc_meta.insert("status".to_string(), Value::String(status.to_string()));
    }
    if timed_out {
        dcc_meta.insert("timed_out".to_string(), Value::Bool(true));
    }
    meta.insert("dcc".to_string(), Value::Object(dcc_meta));
    sc.insert("_meta".to_string(), Value::Object(meta));

    // Build a human-readable text body so the CallToolResult still
    // has a non-empty `content`.
    let text = if timed_out {
        format!("wait_for_terminal: timeout — job still running (status={status})")
    } else if let Some(err) = error_text.as_deref() {
        format!("Job {}: {err}", status)
    } else {
        format!(
            "Job {status} — {}",
            sc.get("result")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(no structured result)".to_string())
        )
    };

    let is_error = matches!(status, "failed" | "cancelled" | "interrupted") || timed_out;
    if let Some(obj) = pending.as_object_mut() {
        obj.insert("structuredContent".to_string(), Value::Object(sc));
        obj.insert("isError".to_string(), Value::Bool(is_error));
        obj.insert(
            "content".to_string(),
            json!([{ "type": "text", "text": text }]),
        );
    }
    pending
}

/// Extract the `job_id` from a backend `tools/call` result envelope, if
/// the backend enqueued an async job. Returns `None` when the tool ran
/// synchronously.
pub(crate) fn extract_job_id(result: &Value) -> Option<String> {
    if let Some(s) = result
        .get("structuredContent")
        .and_then(|c| c.get("job_id"))
        .and_then(Value::as_str)
    {
        return Some(s.to_owned());
    }
    if let Some(s) = result
        .get("_meta")
        .and_then(|m| m.get("dcc"))
        .and_then(|d| d.get("jobId"))
        .and_then(Value::as_str)
    {
        return Some(s.to_owned());
    }
    None
}

// ── Skill-management dispatch ──────────────────────────────────────────────

/// Dispatch a skill-management tool across backends.
///
/// Two patterns:
/// * Fan-out, aggregate (`list_skills`, `find_skills`, `search_skills`,
///   `get_skill_info`): call every matching backend, merge results with
///   `_instance_id` / `_dcc_type` annotations so agents can disambiguate.
/// * Target-instance (`load_skill`, `unload_skill`): require `instance_id` /
///   `dcc` in the arguments; if a single backend is live these default
///   automatically.
async fn skill_mgmt_dispatch(gs: &GatewayState, tool: &str, args: &Value) -> (String, bool) {
    let dcc_filter = args.get("dcc").and_then(Value::as_str);
    let target_instance = args.get("instance_id").and_then(Value::as_str);

    match tool {
        "load_skill" | "unload_skill" => {
            match resolve_target(gs, target_instance, dcc_filter).await {
                Ok(entry) => {
                    // Strip gateway-only routing keys before forwarding.
                    let mut forward_args = args.clone();
                    if let Some(obj) = forward_args.as_object_mut() {
                        obj.remove("instance_id");
                    }
                    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                    match forward_tools_call(
                        &gs.http_client,
                        &url,
                        tool,
                        Some(forward_args),
                        None,
                        None,
                        gs.backend_timeout,
                    )
                    .await
                    {
                        Ok(mut result) => {
                            inject_instance_metadata(
                                &mut result,
                                &entry.instance_id,
                                &entry.dcc_type,
                            );
                            let is_error = result
                                .get("isError")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let text = result
                                .get("content")
                                .and_then(Value::as_array)
                                .and_then(|arr| arr.first())
                                .and_then(|c| c.get("text"))
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(|| {
                                    serde_json::to_string_pretty(&result).unwrap_or_default()
                                });
                            (text, is_error)
                        }
                        Err(e) => (format!("Backend call failed: {e}"), true),
                    }
                }
                Err(msg) => (msg, true),
            }
        }
        _ => {
            // Fan-out aggregation.
            let targets = targets_for_fanout(gs, dcc_filter).await;
            if targets.is_empty() {
                return (
                    "No live DCC instances. Start dcc-mcp-server on the DCC you want to use."
                        .to_string(),
                    true,
                );
            }

            let client = &gs.http_client;
            let backend_timeout = gs.backend_timeout;
            let params = json!({"name": tool, "arguments": args});
            let futs = targets.iter().map(|entry| {
                let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                let params = params.clone();
                async move {
                    let res = super::backend_client::call_backend(
                        client,
                        &url,
                        "tools/call",
                        Some(params),
                        None,
                        backend_timeout,
                    )
                    .await;
                    (entry.instance_id, entry.dcc_type.clone(), res)
                }
            });
            let results = join_all(futs).await;

            let merged: Vec<Value> = results
                .into_iter()
                .map(|(iid, dcc, res)| match res {
                    Ok(v) => {
                        // Extract the actual text payload from the backend
                        // CallToolResult so the merged response is readable
                        // without double-unwrapping.
                        let text = v
                            .get("content")
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.first())
                            .and_then(|c| c.get("text"))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                serde_json::to_string_pretty(&v).unwrap_or_default()
                            });
                        json!({
                            "instance_id": iid.to_string(),
                            "instance_short": instance_short(&iid),
                            "dcc_type": dcc,
                            "result": text,
                        })
                    }
                    Err(e) => json!({
                        "instance_id": iid.to_string(),
                        "instance_short": instance_short(&iid),
                        "dcc_type": dcc,
                        "error": e,
                    }),
                })
                .collect();

            (
                serde_json::to_string_pretty(&json!({"instances": merged})).unwrap_or_default(),
                false,
            )
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn live_backends(gs: &GatewayState) -> Vec<ServiceEntry> {
    let reg = gs.registry.read().await;
    gs.live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE)
        .collect()
}

async fn targets_for_fanout(gs: &GatewayState, dcc_filter: Option<&str>) -> Vec<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect()
}

async fn find_instance_by_prefix(gs: &GatewayState, prefix: &str) -> Option<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .find(|e| instance_short(&e.instance_id) == prefix)
}

async fn resolve_target(
    gs: &GatewayState,
    instance_id: Option<&str>,
    dcc_filter: Option<&str>,
) -> Result<ServiceEntry, String> {
    let candidates = live_backends(gs).await;

    // Exact or prefix match on instance_id.
    if let Some(iid) = instance_id {
        if let Some(e) = candidates.iter().find(|e| {
            let full = e.instance_id.to_string();
            full == iid || full.starts_with(iid) || instance_short(&e.instance_id) == iid
        }) {
            return Ok(e.clone());
        }
        return Err(format!("No live instance matches instance_id='{iid}'"));
    }

    // DCC-filtered auto-select when unambiguous.
    let filtered: Vec<&ServiceEntry> = candidates
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect();

    match filtered.len() {
        0 => Err(match dcc_filter {
            Some(d) => format!("No live '{d}' instance."),
            None => "No live DCC instances.".to_string(),
        }),
        1 => Ok(filtered[0].clone()),
        _ => Err(format!(
            "Ambiguous target — {} instances live. Pass `instance_id` (or use `dcc` filter if only one of that type).",
            filtered.len()
        )),
    }
}

fn to_text_result(res: Result<String, String>) -> (String, bool) {
    match res {
        Ok(text) => (text, false),
        Err(msg) => (msg, true),
    }
}

fn inject_instance_metadata(value: &mut Value, iid: &Uuid, dcc_type: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
        obj.insert(
            "_instance_short".to_string(),
            Value::String(instance_short(iid)),
        );
        obj.insert("_dcc_type".to_string(), Value::String(dcc_type.to_string()));
    }
}

// ── Tools-list fingerprint (for SSE tools/list_changed aggregation) ─────────

/// Compute a fingerprint of the aggregated tool list across every live backend.
///
/// The fingerprint is a stable, sorted concatenation of `{instance_id}:{tool_name}`
/// across every live backend.  When this string changes between two polls, we
/// know at least one backend's tool list mutated (skill loaded / unloaded) and
/// we can push a single `notifications/tools/list_changed` to all connected
/// gateway SSE clients.
///
/// Deliberately excludes tool descriptions / schemas: we only want to detect
/// set-level add/remove changes, not metadata edits.
pub async fn compute_tools_fingerprint(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
) -> String {
    let instances: Vec<ServiceEntry> = {
        let reg = registry.read().await;
        reg.list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(stale_timeout)
                    && e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE
                    && !matches!(
                        e.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::ShuttingDown
                            | dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                    )
            })
            .collect()
    };

    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let tools = fetch_tools(http_client, &url, backend_timeout).await;
        (entry.instance_id, tools)
    });
    let results = join_all(futs).await;

    let mut parts: Vec<String> = results
        .into_iter()
        .flat_map(|(iid, tools)| {
            tools
                .into_iter()
                .map(move |t| format!("{iid}:{}", t.name))
                .collect::<Vec<_>>()
        })
        .collect();
    parts.sort_unstable();
    parts.join("|")
}

// ── Skill-management tool schemas ──────────────────────────────────────────

/// JSON-Schema definitions for the six skill-management tools the gateway
/// exposes (matching the per-DCC server schemas but with gateway-specific
/// routing parameters like `instance_id` and `dcc`).
fn skill_management_tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "list_skills",
            "description": "List all skills across every live DCC instance. Returns a per-instance breakdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["all", "loaded", "unloaded", "error"], "default": "all"},
                    "dcc":    {"type": "string", "description": "Restrict to one DCC type (maya, blender, …)"}
                }
            }
        }),
        json!({
            "name": "find_skills",
            "description": "DEPRECATED — use `search_skills`. Compat alias that forwards to `search_skills` \
                            on every live DCC instance. Scheduled for removal in v0.17.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "tags":  {"type": "array", "items": {"type": "string"}},
                    "dcc":   {"type": "string"}
                }
            }
        }),
        json!({
            "name": "search_skills",
            "description": "Unified skill discovery across every live DCC instance. Matches `query` against \
                            name/description/search_hint/tool names and filters by `tags`, `dcc`, `scope`. \
                            Call with no arguments to browse by trust scope (Admin > System > User > Repo).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "tags":  {"type": "array", "items": {"type": "string"}},
                    "dcc":   {"type": "string"},
                    "scope": {"type": "string", "enum": ["repo", "user", "system", "admin"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }
            }
        }),
        json!({
            "name": "get_skill_info",
            "description": "Get detailed skill info (tools, scripts, dependencies) from each instance that has it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name": {"type": "string"},
                    "dcc":        {"type": "string"}
                },
                "required": ["skill_name"]
            }
        }),
        json!({
            "name": "load_skill",
            "description": "Load a skill on a specific DCC instance. When multiple instances are live, \
                            pass `instance_id` (or the short prefix from list_dcc_instances). With a single \
                            live instance the routing is automatic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name":  {"type": "string"},
                    "skill_names": {"type": "array", "items": {"type": "string"}},
                    "instance_id": {"type": "string", "description": "Target instance (full UUID or short prefix)"},
                    "dcc":         {"type": "string", "description": "DCC type when only one instance of that type is live"}
                },
                "required": ["skill_name"]
            }
        }),
        json!({
            "name": "unload_skill",
            "description": "Unload a skill on a specific DCC instance. Same routing rules as load_skill.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name":  {"type": "string"},
                    "instance_id": {"type": "string"},
                    "dcc":         {"type": "string"}
                },
                "required": ["skill_name"]
            }
        }),
    ]
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_management_tool_defs_cover_all_six_tools() {
        let defs = skill_management_tool_defs();
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        for expected in [
            "list_skills",
            "find_skills",
            "search_skills",
            "get_skill_info",
            "load_skill",
            "unload_skill",
        ] {
            assert!(names.contains(&expected), "missing tool def {expected}");
        }
        assert_eq!(defs.len(), 6, "expected exactly 6 skill-management tools");
    }

    #[test]
    fn skill_management_tool_defs_all_declare_input_schema() {
        for def in skill_management_tool_defs() {
            let schema = def.get("inputSchema").expect("inputSchema present");
            assert_eq!(
                schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "schema for {} is not an object",
                def.get("name").unwrap()
            );
        }
    }

    #[test]
    fn inject_instance_metadata_adds_annotations_to_object() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let mut value = json!({"existing": "field"});
        inject_instance_metadata(&mut value, &id, "maya");

        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("existing").unwrap(), &json!("field"));
        assert_eq!(obj.get("_instance_id").unwrap(), &json!(id.to_string()));
        assert_eq!(obj.get("_instance_short").unwrap(), &json!("abcdef01"));
        assert_eq!(obj.get("_dcc_type").unwrap(), &json!("maya"));
    }

    #[test]
    fn inject_instance_metadata_is_noop_for_non_objects() {
        let id = Uuid::new_v4();
        // Arrays and scalars cannot receive annotations — the helper must
        // silently skip them rather than panic.
        let mut arr = json!([1, 2, 3]);
        inject_instance_metadata(&mut arr, &id, "blender");
        assert_eq!(arr, json!([1, 2, 3]));

        let mut s = json!("scalar");
        inject_instance_metadata(&mut s, &id, "blender");
        assert_eq!(s, json!("scalar"));
    }

    #[test]
    fn to_text_result_maps_ok_to_success() {
        let (text, is_error) = to_text_result(Ok("payload".to_string()));
        assert_eq!(text, "payload");
        assert!(!is_error);
    }

    #[test]
    fn to_text_result_maps_err_to_error() {
        let (text, is_error) = to_text_result(Err("boom".to_string()));
        assert_eq!(text, "boom");
        assert!(is_error);
    }

    // ── #320: extract_job_id covers both sync (None) and async (#318) envelopes.

    #[test]
    fn extract_job_id_reads_structured_content_first() {
        let v = json!({
            "content": [],
            "structuredContent": {"job_id": "job-42", "status": "pending"},
            "isError": false,
        });
        assert_eq!(extract_job_id(&v).as_deref(), Some("job-42"));
    }

    #[test]
    fn extract_job_id_falls_back_to_meta_dcc_jobid() {
        let v = json!({
            "content": [],
            "_meta": {"dcc": {"jobId": "job-99", "parentJobId": null}},
            "isError": false,
        });
        assert_eq!(extract_job_id(&v).as_deref(), Some("job-99"));
    }

    #[test]
    fn extract_job_id_returns_none_for_sync_reply() {
        let v = json!({"content": [{"type": "text", "text": "ok"}], "isError": false});
        assert!(extract_job_id(&v).is_none());
    }

    // ── #321: async opt-in detection + envelope merging ────────────────

    #[test]
    fn meta_signals_async_dispatch_picks_up_async_flag() {
        let meta = json!({"dcc": {"async": true}});
        assert!(meta_signals_async_dispatch(Some(&meta)));
    }

    #[test]
    fn meta_signals_async_dispatch_picks_up_progress_token() {
        let meta = json!({"progressToken": "tok"});
        assert!(meta_signals_async_dispatch(Some(&meta)));
    }

    #[test]
    fn meta_signals_async_dispatch_is_false_for_sync_requests() {
        assert!(!meta_signals_async_dispatch(None));
        let meta = json!({"dcc": {"parentJobId": "abc"}});
        assert!(!meta_signals_async_dispatch(Some(&meta)));
    }

    #[test]
    fn meta_wants_wait_for_terminal_reads_dcc_flag() {
        let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});
        assert!(meta_wants_wait_for_terminal(Some(&meta)));

        let meta = json!({"dcc": {"async": true}});
        assert!(!meta_wants_wait_for_terminal(Some(&meta)));
    }

    #[test]
    fn strip_gateway_meta_flags_removes_wait_for_terminal_only() {
        let meta = json!({"dcc": {"async": true, "wait_for_terminal": true, "parentJobId": "p"}});
        let stripped = strip_gateway_meta_flags(meta);
        assert_eq!(stripped["dcc"]["async"], true);
        assert_eq!(stripped["dcc"]["parentJobId"], "p");
        assert!(stripped["dcc"].get("wait_for_terminal").is_none());
    }

    #[test]
    fn merge_job_update_into_envelope_completed_sets_status_and_result() {
        let pending = json!({
            "content": [{"type": "text", "text": "Job x queued"}],
            "structuredContent": {"job_id": "x", "status": "pending", "_meta": {"dcc": {"jobId": "x"}}},
            "isError": false,
        });
        let update = json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "x", "status": "completed", "result": {"rows": 42}}
        });
        let merged = merge_job_update_into_envelope(pending, &update, false);
        assert_eq!(merged["structuredContent"]["status"], "completed");
        assert_eq!(merged["structuredContent"]["result"]["rows"], 42);
        assert_eq!(
            merged["structuredContent"]["_meta"]["dcc"]["status"],
            "completed"
        );
        assert_eq!(merged["isError"], false);
    }

    #[test]
    fn merge_job_update_into_envelope_failed_marks_is_error() {
        let pending = json!({
            "content": [{"type": "text", "text": "Job x queued"}],
            "structuredContent": {"job_id": "x", "status": "pending"},
            "isError": false,
        });
        let update = json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "x", "status": "failed", "error": "boom"}
        });
        let merged = merge_job_update_into_envelope(pending, &update, false);
        assert_eq!(merged["structuredContent"]["status"], "failed");
        assert_eq!(merged["structuredContent"]["error"], "boom");
        assert_eq!(merged["isError"], true);
    }

    #[test]
    fn merge_job_update_into_envelope_timeout_sets_timed_out_flag() {
        let pending = json!({
            "content": [{"type": "text", "text": "Job x queued"}],
            "structuredContent": {"job_id": "x", "status": "pending"},
            "isError": false,
        });
        let merged = merge_job_update_into_envelope(pending, &Value::Null, true);
        assert_eq!(
            merged["structuredContent"]["_meta"]["dcc"]["timed_out"],
            true
        );
        assert_eq!(merged["isError"], true);
    }
}
