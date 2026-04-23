use super::*;

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
    let params: CallToolParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid tools/call params".to_string()))?;

    let tool_name = params.name.clone();

    // Route core discovery tools
    match tool_name.as_str() {
        "list_roots" => return handle_list_roots(state, req, session_id).await,
        "find_skills" => return handle_find_skills(state, req, &params).await,
        "list_skills" => return handle_list_skills(state, req, &params).await,
        "get_skill_info" => return handle_get_skill_info(state, req, &params).await,
        "load_skill" => return handle_load_skill(state, req, &params, session_id).await,
        "unload_skill" => return handle_unload_skill(state, req, &params, session_id).await,
        "search_skills" => return handle_search_skills(state, req, &params).await,
        "activate_tool_group" => {
            return handle_activate_tool_group(state, req, &params, session_id).await;
        }
        "deactivate_tool_group" => {
            return handle_deactivate_tool_group(state, req, &params, session_id).await;
        }
        "search_tools" => return handle_search_tools(state, req, &params).await,
        // #319 — built-in job polling tool. Always available, regardless of
        // which skills are loaded or whether any jobs exist.
        "jobs.get_status" => return handle_jobs_get_status(state, req, &params).await,
        // #328 — built-in TTL pruning for tracked jobs.
        "jobs.cleanup" => return handle_jobs_cleanup(state, req, &params).await,
        // #254 — lazy-actions fast-path (opt-in).
        "list_actions" if state.lazy_actions => {
            return handle_list_actions(state, req, &params).await;
        }
        "describe_action" if state.lazy_actions => {
            return handle_describe_action(state, req, &params, session_id).await;
        }
        "call_action" if state.lazy_actions => {
            return handle_call_action(state, req, &params, session_id).await;
        }
        _ => {}
    }

    // Skill stub: __skill__<name> — guide model to call load_skill first
    if let Some(skill_name) = tool_name.strip_prefix("__skill__") {
        let envelope = DccMcpError::new(
            "gateway",
            "SKILL_NOT_LOADED",
            format!("Skill '{skill_name}' is not loaded."),
        )
        .with_hint(format!(
            "Call load_skill with skill_name=\"{skill_name}\" to register its tools, \
             then call the specific tool you need."
        ));
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // Group stub: __group__<group_name> — guide model to call activate_tool_group.
    if let Some(group_name) = tool_name.strip_prefix("__group__") {
        let envelope = DccMcpError::new(
            "gateway",
            "GROUP_NOT_ACTIVATED",
            format!("Tool group '{group_name}' is inactive."),
        )
        .with_hint(format!(
            "Call activate_tool_group with group=\"{group_name}\" to enable its tools, \
             then re-list with tools/list."
        ));
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // Resolve action params (default to empty object)
    let call_params = params.arguments.unwrap_or(json!({}));

    // Tool name resolution (#238 + #307):
    //   1. Exact registry hit (canonical `skill__action` form).
    //   2. `<skill>.<action>` shape — the legacy prefixed form. Accepted for
    //      one release even when `bare_tool_names` is on; emits a one-shot
    //      warning so operators find remaining hard-coded clients.
    //   3. Bare action name — the preferred #307 form when unique, or
    //      legacy fallback when the client predates #238.
    let resolved_name: String = if state.registry.get_action(&tool_name, None).is_some() {
        tool_name.clone()
    } else if let Some((skill_part, bare_tool)) = decode_skill_tool_name(&tool_name) {
        let matched = state
            .registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
        if let Some(m) = matched {
            if state.bare_tool_names {
                crate::gateway::namespace::warn_legacy_prefixed_once(&tool_name);
            }
            m.name
        } else {
            tool_name.clone()
        }
    } else {
        let lm = state.registry.list_actions(None).into_iter().find(|m| {
            m.skill_name
                .as_deref()
                .map(|sn| extract_bare_tool_name(sn, &m.name) == tool_name.as_str())
                .unwrap_or(false)
        });
        if let Some(ref matched) = lm {
            // When bare names are the blessed form (#307) this path is the
            // happy path — stay silent. Only warn when the server was
            // explicitly told to keep the prefixed form as the primary shape,
            // which means a bare call is the legacy escape hatch.
            if !state.bare_tool_names {
                let canonical =
                    skill_tool_name(matched.skill_name.as_deref().unwrap_or(""), &matched.name)
                        .unwrap_or_else(|| matched.name.clone());
                tracing::warn!(bare_name=%tool_name, "Deprecated bare name -- use {canonical}.");
            }
            matched.name.clone()
        } else {
            tool_name.clone()
        }
    };

    // Check action exists in registry before dispatch
    let action_meta_snapshot = state.registry.get_action(&resolved_name, None);
    if action_meta_snapshot.is_none() {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown tool: {tool_name}"),
        )
        .with_hint(
            "Use tools/list to see available tools, or load a skill first with load_skill."
                .to_string(),
        );
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // ── Issue #354 — capability gate ──
    //
    // Every tool may declare `required_capabilities` in its sibling
    // `tools.yaml`. If the hosting DCC adapter did not advertise every
    // requirement via `McpHttpConfig::declared_capabilities`, short-circuit
    // the call with the `-32001 capability_missing` JSON-RPC error so
    // clients can react (skip the step, branch to `else`, fail fast).
    if let Some(meta) = action_meta_snapshot.as_ref() {
        let missing = missing_capabilities(
            &meta.required_capabilities,
            state.declared_capabilities.as_ref(),
        );
        if !missing.is_empty() {
            let msg = format!(
                "tool {:?} requires capabilities not advertised by this DCC: {}",
                resolved_name,
                missing.join(", ")
            );
            return Ok(JsonRpcResponse::error_with_data(
                req.id.clone(),
                crate::protocol::error_codes::CAPABILITY_MISSING,
                msg,
                Some(serde_json::json!({
                    "tool": resolved_name,
                    "required_capabilities": meta.required_capabilities,
                    "declared_capabilities": state.declared_capabilities.as_ref(),
                    "missing_capabilities": missing,
                })),
            ));
        }
    }

    // ── Async dispatch path (#318) ───────────────────────────────────────
    //
    // Opt-in conditions — any of these routes the call through `JobManager`
    // and returns immediately with `{job_id, status: "pending"}`:
    //
    // 1. `_meta.dcc.async == true` (explicit client opt-in).
    // 2. `_meta.progressToken` is set (MCP 2025-03-26 long-running hint).
    // 3. The tool declares `execution: async` in its `ActionMeta` (#317).
    // 4. The tool declares a non-zero `timeout_hint_secs` (#317) — the
    //    skill author signalled "expect this to take a while".
    //
    // Otherwise dispatch is synchronous (unchanged path below).
    let meta_dcc = params.meta.as_ref().and_then(|m| m.dcc.as_ref());
    let async_opt_in = meta_dcc.is_some_and(|d| d.r#async);
    let has_progress_token = params
        .meta
        .as_ref()
        .and_then(|m| m.progress_token.as_ref())
        .is_some();
    let action_meta_for_async = action_meta_snapshot.as_ref();
    let action_declares_async = action_meta_for_async
        .map(|m| {
            matches!(m.execution, dcc_mcp_models::ExecutionMode::Async)
                || m.timeout_hint_secs.unwrap_or(0) > 0
        })
        .unwrap_or(false);
    let should_dispatch_async = async_opt_in || has_progress_token || action_declares_async;
    if should_dispatch_async {
        let parent_job_id = meta_dcc.and_then(|d| d.parent_job_id.clone());
        let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
        // #332 — inspect the tool's thread_affinity. Main-affined tools must
        // execute on the DCC main thread via DeferredExecutor even along the
        // async path; Any-affined tools execute on a Tokio worker.
        let thread_affinity = action_meta_for_async
            .map(|m| m.thread_affinity)
            .unwrap_or_default();
        return dispatch_async_job(
            state,
            req,
            resolved_name,
            call_params,
            parent_job_id,
            session_id,
            progress_token,
            thread_affinity,
        )
        .await;
    }

    // ── Register in-flight entry (#240 progress + #241 cancellation) ─────
    let req_id_str: Option<String> = req.id.as_ref().map(|id| match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    });

    if let Some(sid) = session_id {
        notify_message(
            &state.sessions,
            sid,
            SessionLogMessage {
                level: SessionLogLevel::Debug,
                logger: "dcc_mcp_http.tools".to_string(),
                data: json!({
                    "event": "tools_call_received",
                    "tool_name": tool_name.clone(),
                    "resolved_name": resolved_name.clone(),
                }),
                request_id: req_id_str.clone(),
            },
        );
    }

    let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
    let cancel_token = CancelToken::new();
    let progress_reporter = ProgressReporter::new(
        progress_token.clone(),
        session_id.map(str::to_owned),
        state.sessions.clone(),
        req_id_str.clone().unwrap_or_default(),
    );

    // ── Job lifecycle tracking (#316 + #326) ─────────────────────────────
    // Create a Pending→Running→terminal job whenever either (a) the caller
    // supplied a `progressToken` (channel A will fire) or (b) the session
    // opted into `$/dcc.jobUpdated` via `enable_job_notifications`.
    let job_tracking_session = session_id.map(str::to_owned);
    let track_job = job_tracking_session.is_some()
        && (progress_token.is_some() || state.job_notifier.job_updates_enabled());
    let tracked_job_id: Option<String> = if track_job {
        let sid = job_tracking_session.as_deref().unwrap();
        state.job_notifier.subscribe_session(sid);
        let handle = state.jobs.create(tool_name.clone());
        let id = handle.read().id.clone();
        state
            .job_notifier
            .register_job(&id, sid, progress_token.clone());
        state.jobs.start(&id);
        Some(id)
    } else {
        None
    };

    if let Some(ref rid) = req_id_str {
        let entry = InFlightEntry::new(cancel_token.clone(), progress_reporter.clone());
        state.in_flight.insert(rid.clone(), entry);
        tracing::debug!(
            request_id = %rid,
            has_progress_token = progress_token.is_some(),
            "registered in-flight request"
        );
    }

    // ── Pre-dispatch early-cancel check ───────────────────────────────────
    if let Some(ref rid) = req_id_str {
        let already_cancelled = state
            .cancelled_requests
            .get(rid)
            .is_some_and(|ts| ts.elapsed() < CANCELLED_REQUEST_TTL);
        if already_cancelled {
            state.in_flight.remove(rid);
            state.cancelled_requests.remove(rid);
            tracing::info!(request_id = %rid, "request cancelled before dispatch");
            let envelope = DccMcpError::new(
                "registry",
                "CANCELLED",
                format!("Request {rid} was cancelled before dispatch."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    }

    // Dispatch — cancel token is checked before entering the action.
    let cancel_token_for_dispatch = cancel_token.clone();
    let dispatch_outcome = if let Some(exec) = &state.executor {
        // DCC main-thread path
        let dispatcher = state.dispatcher.clone();
        let name = resolved_name.clone();
        let p = call_params.clone();
        let ct = cancel_token_for_dispatch;
        exec.execute(Box::new(move || {
            if ct.is_cancelled() {
                return serde_json::to_string(&json!({"__dispatch_error": "CANCELLED"}))
                    .unwrap_or_default();
            }
            match dispatcher.dispatch(&name, p) {
                Ok(r) => serde_json::to_string(&r.output).unwrap_or_else(|_| "null".to_string()),
                Err(e) => serde_json::to_string(&json!({"__dispatch_error": e.to_string()}))
                    .unwrap_or_default(),
            }
        }))
        .await
        .map(|json_str| {
            let v: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            if let Some(err) = v.get("__dispatch_error") {
                Err(err.as_str().unwrap_or("dispatch error").to_string())
            } else {
                Ok(v)
            }
        })
        .unwrap_or_else(|e| Err(e.to_string()))
    } else {
        // Non-DCC path: spawn_blocking with cooperative cancel monitor.
        let dispatcher = state.dispatcher.clone();
        let name = resolved_name.clone();
        let p = call_params.clone();
        let ct_for_block = cancel_token_for_dispatch.clone();
        let dispatch_fut = tokio::task::spawn_blocking(move || {
            if ct_for_block.is_cancelled() {
                return Err("CANCELLED".to_string());
            }
            dispatcher
                .dispatch(&name, p)
                .map(|r| r.output)
                .map_err(|e| e.to_string())
        });
        tokio::select! {
            result = dispatch_fut => { result.map_err(|e| e.to_string()).and_then(|r| r) }
            _ = async {
                let deadline = tokio::time::Instant::now() + crate::inflight::CANCEL_GRACE_PERIOD;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    if cancel_token_for_dispatch.is_cancelled() || tokio::time::Instant::now() >= deadline { break; }
                }
            } => { Err("CANCELLED".to_string()) }
        }
    };

    if let Some(ref rid) = req_id_str {
        state.in_flight.remove(rid);
    }

    // ── Drive the tracked job to its terminal state (#326) ──────────────
    if let Some(ref jid) = tracked_job_id {
        match &dispatch_outcome {
            Ok(v) => {
                state.jobs.complete(jid, v.clone());
            }
            Err(msg) if msg == "CANCELLED" => {
                state.jobs.cancel(jid);
            }
            Err(msg) => {
                state.jobs.fail(jid, msg.clone());
            }
        }
    }

    let mut call_result = match dispatch_outcome {
        Ok(output) => {
            let text = match &output {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
            };
            let mut content = vec![protocol::ToolContent::Text { text }];

            // #243/#242 — both features are gated on 2025-06-18 sessions.
            //   * resource_link: surface DCC artifact files without copying bytes
            //   * structuredContent: hand back machine-readable payloads so the
            //     agent skips the text→JSON re-parse step
            let is_2025_06_18 = session_id
                .and_then(|sid| state.sessions.get_protocol_version(sid))
                .as_deref()
                == Some("2025-06-18");

            if is_2025_06_18 {
                content.extend(crate::resource_link::extract_resource_links(&output));
            }

            // #242 — ``structuredContent`` carries the dispatch output verbatim
            // when it is a JSON object or array. Strings and nulls go through
            // ``content[].text`` only, matching the 2025-03-26 convention.
            // Older sessions never see the field (serde skips None).
            let structured_content =
                if is_2025_06_18 && matches!(&output, Value::Object(_) | Value::Array(_)) {
                    Some(output.clone())
                } else {
                    None
                };

            CallToolResult {
                content,
                structured_content,
                is_error: false,
                meta: None,
            }
        }
        Err(err_msg) if err_msg == "CANCELLED" => {
            let rid = req_id_str.as_deref().unwrap_or("unknown");
            tracing::info!(request_id = %rid, "tool call cancelled cooperatively");
            if let Some(ref r) = req_id_str {
                state.cancelled_requests.remove(r);
            }
            let envelope = DccMcpError::new(
                "registry",
                "CANCELLED",
                format!("Request {rid} was cancelled by the client."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
        Err(err_msg) => {
            if let Some(sid) = session_id {
                notify_message(
                    &state.sessions,
                    sid,
                    SessionLogMessage {
                        level: SessionLogLevel::Error,
                        logger: "dcc_mcp_http.tools".to_string(),
                        data: json!({
                            "event": "tools_call_failed",
                            "tool_name": tool_name.clone(),
                            "error": err_msg.clone(),
                        }),
                        request_id: req_id_str.clone(),
                    },
                );
            }

            let mut envelope = if err_msg.contains("no handler registered") {
                DccMcpError::new(
                    "instance",
                    "NO_HANDLER",
                    format!("Tool '{tool_name}' is registered but has no handler."),
                )
                .with_hint("Register a handler via ActionDispatcher.register_handler().")
            } else {
                DccMcpError::new("instance", "EXECUTION_FAILED", &err_msg)
            };

            if let (Some(sid), Some(rid)) = (session_id, req_id_str.as_deref()) {
                let log_tail = state.sessions.tail_logs_for_request(sid, rid, 20);
                if !log_tail.is_empty() {
                    envelope = envelope.with_details(json!({ "log_tail": log_tail }));
                }
            }
            CallToolResult {
                content: vec![protocol::ToolContent::Text {
                    text: envelope.to_json(),
                }],
                structured_content: None,
                is_error: true,
                meta: None,
            }
        }
    };

    if let Some(ref rid) = req_id_str {
        let cancelled = state
            .cancelled_requests
            .remove(rid)
            .is_some_and(|(_, recorded_at)| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
        if cancelled {
            tracing::info!(request_id = %rid, "Suppressing result — request was cancelled");
            let envelope = DccMcpError::new(
                "gateway",
                "REQUEST_CANCELLED",
                format!("Request {rid} was cancelled by the client."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    }

    // Issue #342 — attach `_meta["dcc.next_tools"]` with the matching
    // on-success / on-failure list when the tool declared one. The slot
    // is asymmetric on purpose: success results never expose on-failure
    // suggestions and vice versa. Absent → no key, never an empty dict.
    if let Some(action_meta) = state.registry.get_action(&resolved_name, None) {
        attach_next_tools_meta(&mut call_result, &action_meta.next_tools);
    }

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(call_result)?,
    ))
}

/// Async job dispatch path for `tools/call` (issue #318).
///
/// Creates a [`crate::job::Job`] via `state.jobs`, spawns the actual tool
/// execution on Tokio, and returns immediately with a spec-compliant
/// `CallToolResult` envelope:
///
/// ```json
/// {
///   "content": [{"type": "text", "text": "Job <id> queued"}],
///   "structuredContent": {"job_id": "<uuid>", "status": "pending", "parent_job_id": "<uuid>|null"},
///   "isError": false,
///   "_meta": {"dcc": {"jobId": "<uuid>", "parentJobId": "<uuid>|null"}, "status": "pending"}
/// }
/// ```
///
/// Parent-job cascade: when `parent_job_id` resolves to a tracked job, the
/// child's `CancellationToken` is derived from the parent's via
/// [`tokio_util::sync::CancellationToken::child_token`]. Cancelling the
/// parent therefore cancels every descendant within one cooperative
/// checkpoint.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_async_job(
    state: &AppState,
    req: &JsonRpcRequest,
    resolved_name: String,
    call_params: Value,
    parent_job_id: Option<String>,
    session_id: Option<&str>,
    progress_token: Option<Value>,
    thread_affinity: dcc_mcp_models::ThreadAffinity,
) -> Result<JsonRpcResponse, HttpError> {
    let job_handle = state
        .jobs
        .create_with_parent(resolved_name.clone(), parent_job_id.clone());
    let (job_id, cancel_token) = {
        let j = job_handle.read();
        (j.id.clone(), j.cancel_token.clone())
    };

    // ── Wire job lifecycle notifications (#326) ──────────────────────────
    // Map job_id → (session_id, progress_token) so JobNotifier can fan out
    // both `notifications/progress` (if progress_token was supplied) and
    // `notifications/$/dcc.jobUpdated` on every status transition.
    if let Some(sid) = session_id {
        state.job_notifier.subscribe_session(sid);
        state
            .job_notifier
            .register_job(&job_id, sid, progress_token.clone());
    }

    tracing::info!(
        job_id = %job_id,
        tool = %resolved_name,
        parent_job_id = ?parent_job_id,
        affinity = %thread_affinity,
        "async job dispatched"
    );

    // Spawn the actual execution. The task owns clones of everything it
    // needs; the request task returns immediately with the pending envelope.
    let jobs = Arc::clone(&state.jobs);
    let dispatcher = Arc::clone(&state.dispatcher);
    let executor = state.executor.clone();
    let spawn_job_id = job_id.clone();
    let spawn_name = resolved_name.clone();
    let spawn_params = call_params;
    let use_main_thread = matches!(thread_affinity, dcc_mcp_models::ThreadAffinity::Main);
    if use_main_thread && executor.is_none() {
        tracing::warn!(
            tool = %spawn_name,
            "tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }
    tokio::spawn(async move {
        // Pending → Running. If the job was cancelled before pick-up, skip.
        if cancel_token.is_cancelled() {
            tracing::debug!(job_id = %spawn_job_id, "job cancelled before execution");
            return;
        }
        if jobs.start(&spawn_job_id).is_none() {
            tracing::debug!(job_id = %spawn_job_id, "job could not enter Running state");
            return;
        }

        // #332 — pick the execution lane:
        //   * `Main` + executor available  → DeferredExecutor::submit_deferred
        //     (guarantees the handler runs on the DCC main thread)
        //   * `Main` + no executor         → Tokio worker (already warned above)
        //   * `Any`                        → Tokio worker
        let route_to_main = use_main_thread && executor.is_some();
        let exec_result: Result<Value, String> = if route_to_main {
            let exec = executor.as_ref().unwrap();
            let disp = Arc::clone(&dispatcher);
            let name = spawn_name.clone();
            let p = spawn_params.clone();
            let rx = exec.submit_deferred(
                &spawn_name,
                cancel_token.clone(),
                Box::new(move || match disp.dispatch(&name, p) {
                    Ok(r) => serde_json::to_string(&r.output).unwrap_or_else(|_| "null".into()),
                    Err(e) => serde_json::to_string(&json!({"__dispatch_error": e.to_string()}))
                        .unwrap_or_default(),
                }),
            );
            tokio::select! {
                out = rx => match out {
                    Ok(json_str) => {
                        let v: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
                        if let Some(err) = v.get("__dispatch_error") {
                            Err(err.as_str().unwrap_or("dispatch error").to_string())
                        } else {
                            Ok(v)
                        }
                    }
                    // oneshot dropped without sending → cancelled or executor down.
                    Err(_) => Err("CANCELLED".to_string()),
                },
                _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
            }
        } else {
            // `Any` affinity (or `Main` fallback): offload to a blocking
            // worker with cooperative cancel via `tokio::select!`.
            let disp = Arc::clone(&dispatcher);
            let name = spawn_name.clone();
            let p = spawn_params.clone();
            let ct = cancel_token.clone();
            let fut = tokio::task::spawn_blocking(move || {
                if ct.is_cancelled() {
                    return Err("CANCELLED".to_string());
                }
                disp.dispatch(&name, p)
                    .map(|r| r.output)
                    .map_err(|e| e.to_string())
            });
            tokio::select! {
                r = fut => r.map_err(|e| e.to_string()).and_then(|x| x),
                _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
            }
        };

        match exec_result {
            Ok(v) => {
                if jobs.complete(&spawn_job_id, v).is_none() {
                    tracing::debug!(
                        job_id = %spawn_job_id,
                        "job.complete rejected — likely cancelled concurrently"
                    );
                }
            }
            Err(msg) if msg == "CANCELLED" => {
                // `cancel_token` firing already transitioned the job via
                // JobManager::cancel if that path was taken. If the job is
                // still Running (e.g. the token fired via parent cascade
                // without a direct `cancel()` call), mark it cancelled now.
                if jobs
                    .get(&spawn_job_id)
                    .map(|h| h.read().status)
                    .is_some_and(|s| !s.is_terminal())
                {
                    jobs.cancel(&spawn_job_id);
                }
            }
            Err(msg) => {
                jobs.fail(&spawn_job_id, msg);
            }
        }
    });

    // Build the pending envelope.
    let structured = json!({
        "job_id": job_id,
        "status": "pending",
        "parent_job_id": parent_job_id,
    });
    let mut meta = serde_json::Map::new();
    meta.insert("status".to_string(), json!("pending"));
    let mut dcc_meta = serde_json::Map::new();
    dcc_meta.insert("jobId".to_string(), json!(job_id));
    dcc_meta.insert(
        "parentJobId".to_string(),
        parent_job_id
            .as_ref()
            .map(|p| json!(p))
            .unwrap_or(Value::Null),
    );
    meta.insert("dcc".to_string(), Value::Object(dcc_meta));

    // The CallToolResult shape doesn't carry a `_meta` field today; embed it
    // into `structured_content` so clients that read either surface find it.
    // This matches the "structuredContent carries job metadata" convention
    // spelled out in #318 while remaining spec-compliant (extra keys allowed).
    let structured_with_meta = {
        let mut s = structured.as_object().cloned().unwrap_or_default();
        s.insert("_meta".to_string(), Value::Object(meta));
        Value::Object(s)
    };

    let envelope = CallToolResult {
        content: vec![protocol::ToolContent::Text {
            text: format!("Job {job_id} queued"),
        }],
        structured_content: Some(structured_with_meta),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(envelope)?,
    ))
}

/// Populate `CallToolResult._meta["dcc.next_tools"]` per issue #342.
///
/// The key is only emitted when the relevant list (on-success for a
/// success result, on-failure for an error result) is non-empty. Other
/// existing `_meta` entries are preserved; callers are expected to own
/// their own vendor namespace inside the same map.
pub fn attach_next_tools_meta(result: &mut CallToolResult, next_tools: &dcc_mcp_models::NextTools) {
    let list = if result.is_error {
        &next_tools.on_failure
    } else {
        &next_tools.on_success
    };
    if list.is_empty() {
        return;
    }
    let key = if result.is_error {
        "on_failure"
    } else {
        "on_success"
    };
    let mut nt_map = serde_json::Map::new();
    nt_map.insert(
        key.to_string(),
        Value::Array(list.iter().map(|n| Value::String(n.clone())).collect()),
    );
    let meta = result.meta.get_or_insert_with(serde_json::Map::new);
    meta.insert("dcc.next_tools".to_string(), Value::Object(nt_map));
}

pub async fn handle_list_roots(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "list_roots requires Mcp-Session-Id header",
            ))?,
        ));
    };
    let roots = state.sessions.get_client_roots(sid);
    let payload = json!({
        "supports_roots": state.sessions.supports_roots(sid),
        "count": roots.len(),
        "roots": roots,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string_pretty(
            &payload,
        )?))?,
    ))
}
