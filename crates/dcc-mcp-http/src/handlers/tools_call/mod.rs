use super::*;

mod async_impl;
mod resolve_impl;
mod result_impl;
mod sync_impl;

pub(crate) use async_impl::dispatch_async_job;
pub use result_impl::attach_next_tools_meta;
pub use result_impl::handle_list_roots;

use async_impl::async_dispatch_config;
use resolve_impl::{ToolCallResolution, resolve_tool_call};
use sync_impl::dispatch_sync_tool_call;

/// Shared routing decision used by both sync and async `tools/call` paths
/// (issue #716).
///
/// Returns `true` when the call should be dispatched on the DCC main thread
/// (via the wired [`crate::executor::DccExecutorHandle`]), `false` when it
/// should run on a Tokio worker via `spawn_blocking`.
///
/// Routing is driven by **action metadata**, not by whether an executor
/// happens to be wired:
///
/// * `ThreadAffinity::Main` + executor present → main thread
/// * `ThreadAffinity::Main` + no executor       → worker (with a warning
///   logged by the caller; scene API calls would be unsafe here)
/// * `ThreadAffinity::Any`                      → worker, even when an
///   executor is wired — the UI dispatcher is a single-slot queue and must
///   not be reserved for tools that don't need the main thread.
///
/// Before this helper existed, the sync path branched on
/// `executor.is_some()` alone, so every embedded-DCC backend routed
/// `affinity: any` tools through the UI dispatcher where they would fight
/// `affinity: main` tools for the same slot.
pub(crate) fn use_main_thread_route(
    thread_affinity: dcc_mcp_models::ThreadAffinity,
    executor_present: bool,
) -> bool {
    matches!(thread_affinity, dcc_mcp_models::ThreadAffinity::Main) && executor_present
}

pub async fn handle_tools_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Observe tool-call duration / status when the Prometheus exporter
    // is enabled (issue #331). We extract the tool name eagerly so we
    // can still record a row for malformed params.
    #[cfg(feature = "prometheus")]
    let prom_start = std::time::Instant::now();
    #[cfg(feature = "prometheus")]
    let prom_tool_name: Option<String> = req
        .params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    let result = handle_tools_call_inner(state, req, session_id).await;

    #[cfg(feature = "prometheus")]
    if let Some(exporter) = state.prometheus.as_ref() {
        let tool = prom_tool_name.as_deref().unwrap_or("<unknown>");
        let status = match &result {
            Ok(resp) => {
                // A JSON-RPC success response with `result.isError == true`
                // is a tool-level error (MCP convention). Distinguish so
                // counters match what operators see in traces.
                if resp
                    .result
                    .as_ref()
                    .and_then(|r| r.get("isError"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "error"
                } else {
                    "success"
                }
            }
            Err(_) => "error",
        };
        exporter.record_tool_call(tool, status, prom_start.elapsed());
    }

    result
}

pub async fn handle_tools_call_inner(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let resolution = resolve_tool_call(state, req, session_id).await?;
    let resolved = match resolution {
        ToolCallResolution::Response(response) => return Ok(response),
        ToolCallResolution::Dispatch(resolved) => *resolved,
    };

    // Issue #714 — readiness gate. Runs *after* `resolve_tool_call` so
    // discovery/introspection tools (`list_skills`, `search_skills`,
    // `load_skill`, `list_dynamic_tools`, `jobs.get_status`, …) — which
    // take the early-return `ToolCallResolution::Response` branch —
    // bypass the gate. Only DCC-touching actions reach this point, so a
    // red probe reliably refuses work that would otherwise queue on
    // `DeferredExecutor` / `QueueDispatcher` while the DCC is still
    // booting.
    if let Some(response) = readiness_gate(state, req, &resolved.tool_name) {
        return Ok(response);
    }

    if let Some(async_cfg) = async_dispatch_config(&resolved.params, &resolved.action_meta) {
        return dispatch_async_job(
            state,
            req,
            resolved.resolved_name,
            resolved.call_params,
            async_cfg.parent_job_id,
            session_id,
            async_cfg.progress_token,
            async_cfg.thread_affinity,
        )
        .await;
    }

    dispatch_sync_tool_call(state, req, session_id, resolved).await
}

/// Refuse DCC-touching `tools/call` dispatches when the shared
/// [`ReadinessProbe`](dcc_mcp_skill_rest::ReadinessProbe) is red
/// (issue #714).
///
/// Returns `None` when the probe reports ready, so the caller can
/// continue with the normal dispatch path. When the probe is red,
/// returns a `JsonRpcResponse` carrying error code
/// [`BACKEND_NOT_READY`](crate::protocol::error_codes::BACKEND_NOT_READY)
/// with a structured `data` payload echoing the three-state report so
/// clients can back off with context.
fn readiness_gate(
    state: &AppState,
    req: &JsonRpcRequest,
    tool_name: &str,
) -> Option<JsonRpcResponse> {
    let report = state.readiness.report();
    if report.is_ready() {
        return None;
    }
    tracing::warn!(
        tool = tool_name,
        readiness.process = report.process,
        readiness.dispatcher = report.dispatcher,
        readiness.dcc = report.dcc,
        "tools/call refused: backend not ready (issue #714)"
    );
    Some(JsonRpcResponse::error_with_data(
        req.id.clone(),
        crate::protocol::error_codes::BACKEND_NOT_READY,
        format!(
            "Backend is not ready yet: process={}, dispatcher={}, dcc={}. \
             Refusing to queue `tools/call` for `{tool_name}` — retry once \
             `/v1/readyz` reports ready.",
            report.process, report.dispatcher, report.dcc
        ),
        Some(serde_json::json!({
            "tool": tool_name,
            "readiness": {
                "process": report.process,
                "dispatcher": report.dispatcher,
                "dcc": report.dcc,
            },
        })),
    ))
}
