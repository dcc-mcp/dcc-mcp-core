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
    state.bump_registry_generation(); // #438
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
    state.bump_registry_generation(); // #438
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

/// Handle ``search_tools`` — free-text search across every registered tool
/// and, by default, the metadata of unloaded skills the catalog knows about.
///
/// Behaviour (issue #677):
///
/// * **Stubs are filtered by default.** Progressive-loading discovery names
///   like `__skill__<name>` and `__group__<name>` never appear as regular
///   tool hits unless the caller opts in with `include_stubs=true`. This
///   matches what [`super::super::gateway::capability::builder::should_skip`]
///   already does on the MCP-gateway side so both paths behave the same.
/// * **Unloaded skills are searchable.** When `include_unloaded_skills` is
///   unset or `true`, the handler also runs the query through
///   [`SkillCatalog::search_skills`] (BM25-lite over name, description,
///   `search-hint`, tags and `tools.yaml` names) and returns each unloaded
///   hit as a **skill candidate** with `requires_load_skill: true` and a
///   ready-to-send `load_hint` describing the `load_skill` call needed to
///   expose the underlying tools.
/// * **Schema property names are indexed.** The haystack for loaded tools
///   includes the top-level property keys of `input_schema`, so a query
///   like `radius` matches `create_sphere({radius}) ` even if no tag,
///   description or tool name contains the word.
///
/// Output envelope (stable, additive — old fields kept):
///
/// ```json
/// {
///   "total": N,
///   "query": "...",
///   "tools": [...],              // loaded tools, kind = "tool"
///   "skill_candidates": [...]    // unloaded skills, kind = "skill_candidate"
/// }
/// ```
pub async fn handle_search_tools(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();

    let query_raw = args
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if query_raw.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: query"))?,
        ));
    }
    let query = query_raw.to_lowercase();

    let dcc = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);
    let include_disabled = args
        .and_then(|a| a.get("include_disabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_stubs = args
        .and_then(|a| a.get("include_stubs"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_unloaded_skills = args
        .and_then(|a| a.get("include_unloaded_skills"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let limit = args
        .and_then(|a| a.get("limit"))
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(25);

    // ── 1. Loaded-tool hits ───────────────────────────────────────────
    let mut tool_hits: Vec<serde_json::Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !include_disabled && !meta.enabled {
            continue;
        }
        if !include_stubs && is_progressive_stub(&meta.name) {
            continue;
        }
        // Pull top-level input-schema property keys into the haystack so
        // queries that name a parameter (`radius`, `force`, ...) still hit
        // the action that declares them.
        let schema_props = schema_property_names(&meta.input_schema);
        let haystack = format!(
            "{} {} {} {} {}",
            meta.name,
            meta.description,
            meta.category,
            meta.tags.join(" "),
            schema_props.join(" "),
        )
        .to_lowercase();
        if !haystack.contains(&query) {
            continue;
        }
        let mut hit = serde_json::json!({
            "kind": "tool",
            "name": meta.name,
            "description": meta.description,
            "category": meta.category,
            "group": meta.group,
            "enabled": meta.enabled,
            "dcc": meta.dcc,
        });
        if let Some(skill) = &meta.skill_name {
            hit["skill_name"] = Value::String(skill.clone());
        }
        tool_hits.push(hit);
        if tool_hits.len() >= limit {
            break;
        }
    }

    // ── 1b. Progressive-loading stubs (debug opt-in) ───────────────────
    //
    // Stubs are *not* stored in the ActionRegistry — they are synthesised
    // on demand by `tools/list` for unloaded skills and inactive tool
    // groups. When `include_stubs=true`, mirror that synthesis here so
    // operators can verify the progressive-loading surface end-to-end
    // from a single `search_tools` call.
    if include_stubs && tool_hits.len() < limit {
        // Skill stubs: every non-loaded skill whose metadata matches the
        // query gets surfaced as `__skill__<name>`.
        for summary in state.catalog.list_skills(Some("unloaded")) {
            if let Some(filter) = dcc
                && !summary.dcc.eq_ignore_ascii_case(filter)
            {
                continue;
            }
            let haystack = format!(
                "{} {} {} {} {}",
                summary.name,
                summary.description,
                summary.search_hint,
                summary.tags.join(" "),
                summary.tool_names.join(" "),
            )
            .to_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
            tool_hits.push(serde_json::json!({
                "kind": "tool",
                "name": format!("__skill__{}", summary.name),
                "description": format!(
                    "[stub] unloaded skill `{}` — call load_skill(\"{}\") to expose its {} tool(s)",
                    summary.name, summary.name, summary.tool_count,
                ),
                "category": "stub",
                "group": "",
                "enabled": false,
                "dcc": summary.dcc,
                "skill_name": summary.name,
            }));
            if tool_hits.len() >= limit {
                break;
            }
        }

        // Group stubs: every declared-but-inactive tool group gets surfaced
        // as `__group__<name>`. Names can repeat across skills, so we
        // de-duplicate by group name.
        if tool_hits.len() < limit {
            let mut seen_groups: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for (skill, group, active) in state.catalog.list_groups() {
                if active {
                    continue;
                }
                if !seen_groups.insert(group.clone()) {
                    continue;
                }
                let haystack = format!("__group__{} {} {}", group, group, skill).to_lowercase();
                if !haystack.contains(&query) {
                    continue;
                }
                tool_hits.push(serde_json::json!({
                    "kind": "tool",
                    "name": format!("__group__{}", group),
                    "description": format!(
                        "[stub] inactive tool group `{}` — call activate_tool_group(group=\"{}\") to expose its members",
                        group, group,
                    ),
                    "category": "stub",
                    "group": group,
                    "enabled": false,
                    "dcc": "",
                    "skill_name": skill,
                }));
                if tool_hits.len() >= limit {
                    break;
                }
            }
        }
    }

    // ── 2. Unloaded-skill candidates ──────────────────────────────────
    let mut skill_candidates: Vec<serde_json::Value> = Vec::new();
    if include_unloaded_skills {
        let candidates = state
            .catalog
            .search_skills(Some(query_raw), &[], dcc, None, Some(limit));
        for summary in candidates {
            if summary.loaded {
                continue; // already surfaced through registry hits above
            }
            let detail = state.catalog.get_skill_info(&summary.name);
            let matching_tools = detail
                .as_ref()
                .map(|d| {
                    d.tools
                        .iter()
                        .filter(|t| {
                            t.name.to_lowercase().contains(&query)
                                || t.description.to_lowercase().contains(&query)
                        })
                        .map(|t| t.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            skill_candidates.push(serde_json::json!({
                "kind": "skill_candidate",
                "skill_name": summary.name,
                "description": summary.description,
                "tags": summary.tags,
                "dcc": summary.dcc,
                "scope": summary.scope,
                "tool_count": summary.tool_count,
                "matching_tools": matching_tools,
                "requires_load_skill": true,
                "load_hint": {
                    "tool": "load_skill",
                    "arguments": { "skill_name": summary.name },
                },
            }));
        }
    }

    let total = tool_hits.len() + skill_candidates.len();
    let result = serde_json::json!({
        "total": total,
        "query": query,
        "tools": tool_hits,
        "skill_candidates": skill_candidates,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&result)?))?,
    ))
}

/// `true` when `name` matches a progressive-loading discovery stub.
///
/// These names — `__skill__<name>`, `__group__<name>`, or a dotted form
/// `<ns>.__skill__<name>` — describe *how to load* a capability, not a
/// capability you can invoke, so `search_tools` hides them by default.
fn is_progressive_stub(name: &str) -> bool {
    name.starts_with("__skill__")
        || name.starts_with("__group__")
        || name.contains(".__skill__")
        || name.contains(".__group__")
}

/// Collect top-level property names from a JSON Schema object.
///
/// Returns an empty vector for schemas without a `properties` map. Used by
/// `handle_search_tools` to index parameter names (e.g. `radius`, `force`)
/// so they become matchable by keyword queries.
fn schema_property_names(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
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
    if include_result
        && job.status.is_terminal()
        && let Some(ref r) = job.result
    {
        envelope.insert("result".into(), r.clone());
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
