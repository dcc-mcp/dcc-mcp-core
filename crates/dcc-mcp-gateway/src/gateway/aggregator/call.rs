use super::*;

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
        "acquire_dcc_instance" => return to_text_result(tool_acquire_instance(gs, args).await),
        "release_dcc_instance" => return to_text_result(tool_release_instance(gs, args).await),
        "diagnostics__process_status" => {
            return to_text_result(tool_diagnostics_process_status(gs, args).await);
        }
        "diagnostics__audit_log" => {
            return to_text_result(tool_diagnostics_audit_log(gs, args).await);
        }
        "diagnostics__tool_metrics" => {
            return to_text_result(tool_diagnostics_tool_metrics(gs, args).await);
        }
        // ── #655 dynamic-capability MCP wrappers ────────────────────
        "search_tools" => return to_text_result(tool_search_tools(gs, args).await),
        "describe_tool" => return to_text_result(tool_describe_tool(gs, args).await),
        "call_tool" => return tool_call_tool(gs, args, meta).await,
        _ => {}
    }

    // ── Skill-management tools ──────────────────────────────────────────
    if matches!(
        tool,
        "list_skills" | "search_skills" | "get_skill_info" | "load_skill" | "unload_skill"
    ) {
        return skill_mgmt_dispatch(gs, tool, args).await;
    }

    if !gs.tool_exposure.publishes_backend_tools() {
        return (
            format!(
                "Tool '{tool}' is not available as a direct gateway MCP tool in {} mode. Use `search_tools`, `describe_tool`, and `call_tool` instead.",
                gs.tool_exposure.as_str(),
            ),
            true,
        );
    }

    // ── Backend tool routing ────────────────────────────────────────────
    // Preferred gateway names come in two Cursor-safe / SEP-986 forms
    // (both accepted by `decode_tool_name`), but with a single live
    // backend a bare tool name is unambiguous and easier for clients
    // to call (#583).
    let (entry, original) = match decode_tool_name(tool) {
        Some((prefix, original)) => {
            let Some(entry) = find_instance_by_prefix(gs, &prefix).await else {
                return (
                    format!("No live DCC instance matches prefix '{prefix}' in tool '{tool}'."),
                    true,
                );
            };
            (entry, original)
        }
        None => {
            let instances = live_backends(gs).await;
            if instances.len() == 1 {
                (instances.into_iter().next().unwrap(), tool.to_string())
            } else {
                // Detect the common "skill__toolname" internal-format mistake and emit
                // a targeted hint instead of the generic "Unknown tool" message.
                let hint = if tool.contains("__") {
                    format!(
                        "Unknown tool: '{tool}'. \
                         '{tool}' looks like an internal action name (double-underscore format). \
                         Gateway tools are published as 'i_{{id8}}__{{escaped_name}}' (Cursor-safe) \
                         or '{{id8}}.{{bare_name}}' during the compatibility window. \
                         In slim / rest mode, use `search_tools`, `describe_tool`, and `call_tool` \
                         to discover and invoke backend capabilities dynamically."
                    )
                } else {
                    format!(
                        "Unknown tool: '{tool}'. \
                         Call tools/list (or search_skills) to discover available tool names. \
                         Gateway tools use the form 'i_{{id8}}__{{escaped_tool}}' (Cursor-safe); \
                         in slim / rest mode, use `search_tools`, `describe_tool`, and `call_tool`."
                    )
                };
                return (hint, true);
            }
        }
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
        &original,
        Some(args.clone()),
        forwarded_meta,
        request_id.clone(),
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
                // #322: record the full JobRoute so `notifications/cancelled`
                // can later forward to this backend. `parent_job_id` comes
                // from the inbound `_meta.dcc.parentJobId` so workflow
                // cancellations can cascade across backends.
                let parent = meta
                    .and_then(|m| m.get("dcc"))
                    .and_then(|d| d.get("parentJobId"))
                    .and_then(|p| p.as_str());
                if let Err(e) = gs
                    .subscriber
                    .bind_job_route(jid, sid, &url, &original, parent)
                {
                    tracing::warn!(
                        session = %sid,
                        job = %jid,
                        backend = %url,
                        error = %e,
                        "gateway: refusing to bind route — per-session cap reached"
                    );
                    let payload = json!({
                        "error": {
                            "code": -32005,
                            "message": format!("{e}"),
                            "data": {
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
                // Correlate the originating JSON-RPC requestId with the
                // dispatched job_id so a later `notifications/cancelled`
                // can resolve to the JobRoute.
                if let Some(ref rid) = request_id {
                    gs.subscriber.bind_request_to_job(rid, jid);
                }
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
