use super::*;

pub async fn handle_activate_tool_group(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let group = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("group"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: group"))?,
        ));
    }

    let changed = state.catalog.activate_group(group);
    if let Some(sid) = session_id {
        let added: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let removed = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    let payload = json!({
        "success": true,
        "group": group,
        "changed": changed,
        "active_groups": state.catalog.active_groups(),
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(payload.to_string()))?,
    ))
}

/// Handle ``deactivate_tool_group`` — mirror of [`handle_activate_tool_group`].
pub async fn handle_deactivate_tool_group(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let group = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("group"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: group"))?,
        ));
    }

    let changed = state.catalog.deactivate_group(group);
    if let Some(sid) = session_id {
        let removed: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let added = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    let payload = json!({
        "success": true,
        "group": group,
        "changed": changed,
        "active_groups": state.catalog.active_groups(),
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(payload.to_string()))?,
    ))
}

/// Handle ``search_tools`` — free-text search across every registered tool.
pub async fn handle_search_tools(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let query = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    if query.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: query"))?,
        ));
    }
    let dcc = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("dcc"))
        .and_then(Value::as_str);
    let include_disabled = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("include_disabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut matches: Vec<serde_json::Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !include_disabled && !meta.enabled {
            continue;
        }
        let haystack = format!(
            "{} {} {} {}",
            meta.name,
            meta.description,
            meta.category,
            meta.tags.join(" ")
        )
        .to_lowercase();
        if haystack.contains(&query) {
            matches.push(serde_json::json!({
                "name": meta.name,
                "description": meta.description,
                "category": meta.category,
                "group": meta.group,
                "enabled": meta.enabled,
                "dcc": meta.dcc,
            }));
        }
    }
    let result = serde_json::json!({
        "total": matches.len(),
        "query": query,
        "tools": matches,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&result)?))?,
    ))
}

// ── Built-in `jobs.get_status` (#319) ─────────────────────────────────────

/// Handle ``jobs.get_status`` — poll a tracked job's lifecycle state.
///
/// Returns the standard status envelope — ``{job_id, parent_job_id, tool,
/// status, created_at, started_at, completed_at, progress, error, result}``
/// — mirroring the field names emitted on the ``$/dcc.jobUpdated`` SSE
/// channel (#326) so clients can mix polling and streaming freely.
///
/// Semantics:
///
/// * Missing / empty ``job_id`` → ``isError=true`` with a human-readable
///   message (still a valid ``CallToolResult``, never a JSON-RPC error).
/// * Unknown ``job_id`` → ``isError=true`` naming the bad id.
/// * ``include_result=false`` or job not terminal → ``result`` is omitted.
/// * ``include_logs=true`` is accepted for forward compatibility —
///   ``JobManager`` does not currently capture per-job stdout/stderr, so
///   the flag is a no-op and a ``tracing::debug!`` breadcrumb is emitted.
pub async fn handle_jobs_get_status(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let job_id = args
        .and_then(|a| a.get("job_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if job_id.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: job_id".to_string(),
            ))?,
        ));
    }
    let include_logs = args
        .and_then(|a| a.get("include_logs"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_result = args
        .and_then(|a| a.get("include_result"))
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if include_logs {
        // #319: accepted for forward-compat. JobManager does not capture
        // stdout/stderr today; document the reality instead of silently
        // pretending to honour the flag.
        tracing::debug!(
            job_id = %job_id,
            "jobs.get_status received include_logs=true — no-op, JobManager does not capture logs"
        );
    }

    let Some(entry) = state.jobs.get(job_id) else {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!(
                "No job found with id '{job_id}'"
            )))?,
        ));
    };
    let job = entry.read();

    // Build the envelope. Field order / names mirror `$/dcc.jobUpdated`
    // (see `notifications.rs`) so polling clients see the same shape as
    // streaming subscribers.
    let (started_at, completed_at) = compute_job_timestamps(&job);
    let mut envelope = serde_json::Map::new();
    envelope.insert("job_id".into(), Value::String(job.id.clone()));
    envelope.insert(
        "parent_job_id".into(),
        match &job.parent_job_id {
            Some(p) => Value::String(p.clone()),
            None => Value::Null,
        },
    );
    envelope.insert("tool".into(), Value::String(job.tool_name.clone()));
    envelope.insert(
        "status".into(),
        serde_json::to_value(job.status).unwrap_or(Value::Null),
    );
    envelope.insert(
        "created_at".into(),
        Value::String(job.created_at.to_rfc3339()),
    );
    envelope.insert(
        "started_at".into(),
        started_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "completed_at".into(),
        completed_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "updated_at".into(),
        Value::String(job.updated_at.to_rfc3339()),
    );
    envelope.insert(
        "progress".into(),
        serde_json::to_value(&job.progress).unwrap_or(Value::Null),
    );
    envelope.insert(
        "error".into(),
        match &job.error {
            Some(e) => Value::String(e.clone()),
            None => Value::Null,
        },
    );
    if include_result && job.status.is_terminal() {
        if let Some(ref r) = job.result {
            envelope.insert("result".into(), r.clone());
        }
    }
    drop(job);

    let envelope_value = Value::Object(envelope);
    let text = serde_json::to_string(&envelope_value)?;
    let tool_result = CallToolResult {
        content: vec![crate::protocol::ToolContent::Text { text }],
        structured_content: Some(envelope_value),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(tool_result)?,
    ))
}

// ── Built-in `jobs.cleanup` (#328) ────────────────────────────────────────

/// Handle ``jobs.cleanup`` — TTL prune terminal jobs from JobManager
/// and any attached storage backend (issue #328).
///
/// Semantics:
/// * `older_than_hours` defaults to 24. Values of 0 prune every
///   terminal row that already exists (useful for tests).
/// * Non-terminal (pending/running) rows are never touched.
/// * Returns a ``{removed: <count>}`` envelope both as text and
///   `structuredContent`.
pub async fn handle_jobs_cleanup(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let older_than_hours = args
        .and_then(|a| a.get("older_than_hours"))
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let removed = state.jobs.cleanup_older_than_hours(older_than_hours);
    let envelope = serde_json::json!({
        "removed": removed,
        "older_than_hours": older_than_hours,
    });
    let text = serde_json::to_string(&envelope)?;
    let tool_result = CallToolResult {
        content: vec![crate::protocol::ToolContent::Text { text }],
        structured_content: Some(envelope),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(tool_result)?,
    ))
}

/// Derive ``started_at`` and ``completed_at`` from a [`Job`] snapshot.
///
/// `JobManager` does not store these explicitly — it keeps only
/// `created_at` + `updated_at` + current `status`. For the public
/// envelope (#319 / #326) we reconstruct them:
/// * `started_at` is `updated_at` once the job has left `Pending`.
/// * `completed_at` is `updated_at` once the job is terminal.
pub fn compute_job_timestamps(
    job: &crate::job::Job,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    use crate::job::JobStatus;
    let started_at = match job.status {
        JobStatus::Pending => None,
        _ => Some(job.updated_at),
    };
    let completed_at = if job.status.is_terminal() {
        Some(job.updated_at)
    } else {
        None
    };
    (started_at, completed_at)
}

// ── Lazy-actions fast-path (#254) ─────────────────────────────────────────

/// Handle ``list_actions`` — compact action catalog without JSON schemas.
///
/// Returns one JSON object per enabled action, containing **only** the
/// three fields needed for an agent to decide whether to follow up with
/// ``describe_action`` / ``call_action``:
///
/// ```text
/// {"id": <full tool name>, "summary": <description>, "tags": [...]}
/// ```
///
/// Deliberately omits the input/output schemas — surfacing them here
