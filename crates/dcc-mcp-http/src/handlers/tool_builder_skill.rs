use super::*;

pub fn build_lazy_action_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "list_actions".to_string(),
            description: "Returns every enabled action as a compact {id, summary, tags} record with no JSON schemas attached.\n\n\
                          When to use: The entry point of the lazy-actions fast-path (lazy_actions=true). Use it to enumerate candidates cheaply when the full tools/list would blow the token budget.\n\n\
                          How to use:\n\
                          - Filter with dcc and/or skill to narrow the list before fetching schemas.\n\
                          - Follow up with describe_action(id=...) for one action, then call_action(id=..., args=...) to invoke."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender)."
                    },
                    "skill": {
                        "type": "string",
                        "description": "Skill-name filter to limit results to one skill."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Actions".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "describe_action".to_string(),
            description: "Returns the full JSON input schema and metadata for a single action, identical to what tools/list would surface for it.\n\n\
                          When to use: Step 2 of the lazy-actions flow — after list_actions has narrowed the candidate, fetch the schema for exactly one action before calling it.\n\n\
                          How to use:\n\
                          - Pass id exactly as reported by list_actions; unknown ids return an error.\n\
                          - Follow up with call_action(id=..., args=...) using the returned schema."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Action id as reported by list_actions."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Describe Action".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "call_action".to_string(),
            description: "Generic dispatcher that invokes any action by id with the given arguments, using the same code path as a native tools/call.\n\n\
                          When to use: Step 3 of the lazy-actions flow, or whenever you want to avoid inflating tools/list with every action. Semantically identical to calling the action's native tool name directly.\n\n\
                          How to use:\n\
                          - Make sure args matches the schema from describe_action; invalid args are rejected.\n\
                          - Side effects are those of the underlying action — check its ToolAnnotations first."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Action id (e.g. create_sphere or maya-geometry.create_sphere)."
                    },
                    "args": {
                        "type": "object",
                        "description": "Arguments matching the action's input_schema."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Call Action".to_string()),
                // `call_action` itself has no side effects beyond those of
                // the underlying action — so we inherit nothing and signal
                // the open-world hint so clients treat it defensively.
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(false),
                open_world_hint: Some(true),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
    ]
}

/// Convert an ActionMeta to an McpTool, respecting annotations from the skill.
///
/// `include_output_schema` controls whether the action's declared
/// [`ActionMeta::output_schema`] is surfaced as the MCP `outputSchema` field
/// (introduced in 2025-06-18). On older sessions this must be `false` so the
/// field is never serialised.
pub fn action_meta_to_mcp_tool(
    meta: &dcc_mcp_actions::registry::ActionMeta,
    include_output_schema: bool,
    bare_eligible: &std::collections::HashSet<(String, String)>,
    declared_capabilities: &[String],
) -> McpTool {
    let input_schema = if meta.input_schema.is_null() {
        json!({"type": "object"})
    } else {
        meta.input_schema.clone()
    };

    // Only surface a non-null schema. An explicit `null` from the action is
    // equivalent to "unspecified" and must not leak as `outputSchema: null`
    // (which some clients treat as a hard rejection).
    let output_schema = if include_output_schema && !meta.output_schema.is_null() {
        Some(meta.output_schema.clone())
    } else {
        None
    };

    // #307 — prefer the bare action name when the caller has deemed it
    // unique within this instance. Core tools and actions without a skill
    // still publish under their canonical form.
    let mcp_name = meta
        .skill_name
        .as_deref()
        .map(|sn| {
            let key = (sn.to_string(), meta.name.clone());
            if bare_eligible.contains(&key) {
                crate::gateway::namespace::extract_bare_tool_name(sn, &meta.name).to_string()
            } else {
                skill_tool_name(sn, &meta.name).unwrap_or_else(|| meta.name.clone())
            }
        })
        .unwrap_or_else(|| meta.name.clone());
    // Build the MCP `annotations` object from the skill-author declaration
    // (issue #344). Only hints that were explicitly declared appear in
    // the output — tools without any spec-standard annotations omit the
    // `annotations` field entirely instead of emitting an empty object.
    // `deferred_hint` is intentionally *not* placed inside the spec
    // annotations map — it rides in `_meta["dcc.deferred_hint"]` (set by
    // `build_tool_meta`), which keeps us MCP 2025-03-26 compliant.
    let declared = &meta.annotations;
    let annotations = if declared.is_spec_empty() {
        None
    } else {
        Some(McpToolAnnotations {
            title: declared.title.clone(),
            read_only_hint: declared.read_only_hint,
            destructive_hint: declared.destructive_hint,
            idempotent_hint: declared.idempotent_hint,
            open_world_hint: declared.open_world_hint,
            deferred_hint: None,
        })
    };

    McpTool {
        name: mcp_name,
        description: meta.description.clone(),
        input_schema,
        output_schema,
        annotations,
        meta: build_tool_meta(meta, declared_capabilities),
    }
}

/// Build the MCP `_meta` map for a tool definition (issues #317, #344).
///
/// Emits dcc-mcp-core-specific hints under a vendor-scoped `dcc.*` key so
/// future additions don't collide with spec-defined fields:
///
/// * `dcc.timeoutHintSecs` — when the skill author declared
///   `timeout_hint_secs` (issue #317).
/// * `dcc.deferred_hint` — when the tool is deferred. This is a
///   dcc-mcp-core extension (not part of MCP 2025-03-26), so it rides in
///   `_meta` instead of the spec `annotations` map (issue #344). The
///   value is `true` when either the skill author declared
///   `deferred_hint: true` in `tools.yaml` **or** the author declared
///   `execution: async` (which implies deferred).
///
/// Returns `None` when there is nothing to emit.
pub fn build_tool_meta(
    meta: &dcc_mcp_actions::registry::ActionMeta,
    declared_capabilities: &[String],
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let deferred = meta
        .annotations
        .deferred_hint
        .unwrap_or_else(|| meta.execution.is_deferred());

    let has_timeout = meta.timeout_hint_secs.is_some();
    // Issue #354 — surface any capabilities the tool requires that the
    // hosting DCC adapter did not declare, so clients can filter these
    // out before asking the user to invoke them.
    let missing = missing_capabilities(&meta.required_capabilities, declared_capabilities);
    let has_required_caps = !meta.required_capabilities.is_empty();
    if !has_timeout && !deferred && !has_required_caps {
        return None;
    }

    let mut dcc_meta = serde_json::Map::new();
    if let Some(t) = meta.timeout_hint_secs {
        dcc_meta.insert("timeoutHintSecs".to_string(), serde_json::json!(t));
    }
    if deferred {
        dcc_meta.insert("deferred_hint".to_string(), serde_json::json!(true));
    }
    if has_required_caps {
        dcc_meta.insert(
            "required_capabilities".to_string(),
            serde_json::json!(meta.required_capabilities),
        );
        if !missing.is_empty() {
            dcc_meta.insert(
                "missing_capabilities".to_string(),
                serde_json::json!(missing),
            );
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("dcc".to_string(), serde_json::Value::Object(dcc_meta));
    Some(out)
}

/// Return the subset of `required` capabilities that is not present in
/// `declared`. Preserves declaration order; drops empty tags.
pub(crate) fn missing_capabilities(required: &[String], declared: &[String]) -> Vec<String> {
    if required.is_empty() {
        return Vec::new();
    }
    let set: std::collections::HashSet<&str> = declared.iter().map(String::as_str).collect();
    required
        .iter()
        .filter(|c| !c.is_empty() && !set.contains(c.as_str()))
        .cloned()
        .collect()
}

/// Build a lightweight stub McpTool for an unloaded skill.
///
/// The stub is surfaced in `tools/list` so the model knows the skill exists
/// and what tools it contains — without emitting full input schemas.
/// When called, the stub responds with a hint to call `load_skill` first.
///
/// Name format: `__skill__<skill_name>`
pub fn build_skill_stub(summary: &SkillSummary) -> McpTool {
    // When an explicit search-hint was provided in SKILL.md, surface it in the
    // stub description so the agent can match skills by keyword without an
    // extra round-trip.  The hint is considered explicit when it differs from
    // the description (the catalog falls back to description when no hint is
    // set).  When no explicit hint exists, keep the compact tool-name preview.
    let has_explicit_hint =
        !summary.search_hint.is_empty() && summary.search_hint != summary.description;

    let description = if has_explicit_hint {
        format!(
            "[{}] {} tools • keywords: {} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, summary.search_hint, summary.name
        )
    } else {
        const PREVIEW_LIMIT: usize = 5;
        let preview = if summary.tool_names.is_empty() {
            String::new()
        } else if summary.tool_names.len() <= PREVIEW_LIMIT {
            format!(" ({})", summary.tool_names.join(", "))
        } else {
            format!(
                " ({}, …+{} more)",
                summary.tool_names[..PREVIEW_LIMIT].join(", "),
                summary.tool_names.len() - PREVIEW_LIMIT
            )
        };

        format!(
            "[{}] {} tools{} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, preview, summary.name
        )
    };

    McpTool {
        name: format!("__skill__{}", summary.name),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        output_schema: None,
        // Skill stubs are not callable tools: they exist solely to hint the agent
        // to call `load_skill` first. Full annotation blocks add ~40-60 tokens
        // per stub × 64 skills = measurable `tools/list` bloat with zero routing
        // value for the model. (#235)
        annotations: None,
        meta: None,
    }
}

/// Handle `search_skills` — unified skill discovery tool (issue #340).
///
/// Input:
///   - `query`  (str, optional)     — substring match on name/description/search_hint/tool names
///   - `tags`   (list[str], optional) — every tag must match (AND)
///   - `dcc`    (str, optional)       — filter by DCC binding
///   - `scope`  (str, optional)       — `"repo" | "user" | "system" | "admin"`
///   - `limit`  (int, optional)       — cap results (default 20, max 100)
///
/// When all inputs are empty/None, returns the top `limit` skills sorted by
/// scope precedence (Admin > System > User > Repo) then name. This is the
/// "what skills are available?" discovery entry point for agents.
pub async fn handle_search_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    let args = params.arguments.as_ref();

    let query = args
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let tags_owned: Vec<String> = args
        .and_then(|a| a.get("tags"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let tags: Vec<&str> = tags_owned.iter().map(String::as_str).collect();

    let dcc_filter = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);

    let scope_filter = match args.and_then(|a| a.get("scope")).and_then(Value::as_str) {
        None => None,
        Some(s) => match parse_scope_label(s) {
            Ok(sc) => Some(sc),
            Err(msg) => {
                return Ok(JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(CallToolResult::error(msg))?,
                ));
            }
        },
    };

    let limit = args
        .and_then(|a| a.get("limit"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let query_opt = if query.is_empty() { None } else { Some(query) };
    let matches =
        state
            .catalog
            .search_skills(query_opt, &tags, dcc_filter, scope_filter, Some(limit));

    if matches.is_empty() {
        let text = if query.is_empty()
            && tags.is_empty()
            && dcc_filter.is_none()
            && scope_filter.is_none()
        {
            "No skills discovered. Drop SKILL.md files into the scan paths and rescan.".to_string()
        } else if query.is_empty() {
            "No skills match the given filters.".to_string()
        } else {
            format!("No skills found matching '{query}'.")
        };
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::text(text))?,
        ));
    }

    // RTK-inspired: ultra-compact JSON format to reduce token consumption.
    // Keep the historical keys (`name`, `tools`, `loaded`, `dcc`) and add
    // `scope` / `description` / `tags` / `search_hint` so the union covers
    // what find_skills used to return.
    let compact_skills: Vec<serde_json::Value> = matches
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "tools": s.tool_count,
                "loaded": s.loaded,
                "dcc": s.dcc,
                "scope": s.scope,
                "tags": s.tags,
                "search_hint": s.search_hint,
            })
        })
        .collect();

    let result = serde_json::json!({
        "total": matches.len(),
        "query": query,
        "skills": compact_skills
    });

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&result)?))?,
    ))
}

/// Parse the `scope` argument string into a [`SkillScope`].
pub fn parse_scope_label(s: &str) -> Result<SkillScope, String> {
    match s.to_ascii_lowercase().as_str() {
        "repo" => Ok(SkillScope::Repo),
        "user" => Ok(SkillScope::User),
        "system" => Ok(SkillScope::System),
        "admin" => Ok(SkillScope::Admin),
        other => Err(format!(
            "Invalid scope {other:?}: expected 'repo' | 'user' | 'system' | 'admin'"
        )),
    }
}

/// Build a compact stub that replaces all tools of an inactive group in
/// ``tools/list``. Collapses the group into one entry the agent can reason
/// about without paying the schema cost for every member tool.
pub fn build_group_stub(group: &str, tool_names: &[String]) -> McpTool {
    const PREVIEW_LIMIT: usize = 5;
    let preview = if tool_names.len() <= PREVIEW_LIMIT {
        format!(" [{}]", tool_names.join(", "))
    } else {
        format!(
            " [{}, … +{} more]",
            tool_names[..PREVIEW_LIMIT].join(", "),
            tool_names.len() - PREVIEW_LIMIT
        )
    };
    let description = format!(
        "Inactive group '{group}' • {} tools{preview} • Call activate_tool_group(\"{group}\")",
        tool_names.len(),
    );
    McpTool {
        name: format!("__group__{group}"),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        output_schema: None,
        // Same rationale as `build_skill_stub`: group stubs are not callable
        // tools, so their annotations are pure protocol noise. (#235)
        annotations: None,
        meta: None,
    }
}

/// Handle ``activate_tool_group`` — flips every action in the named group
