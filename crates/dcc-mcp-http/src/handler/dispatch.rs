//! JSON-RPC method dispatch + `initialize` / `tools/list` implementations.
//!
//! `dispatch_request` is the central router: every client request
//! passing through `POST /mcp` ends up here, is routed to the matching
//! `handle_*` function (most of which live in [`crate::handlers`]),
//! and the resulting [`JsonRpcResponse`] is returned back up to the
//! axum layer in [`crate::handler::handle_post`].

use serde_json::{Value, json};

use super::state::AppState;
use crate::error::HttpError;
use crate::handlers::{
    action_meta_to_mcp_tool, build_core_tools, build_group_stub, build_lazy_action_tools,
    build_skill_stub, handle_elicitation_create, handle_logging_set_level, handle_prompts_get,
    handle_prompts_list, handle_resources_list, handle_resources_read, handle_resources_subscribe,
    handle_resources_unsubscribe, handle_tools_call, refresh_roots_cache_for_session,
};
use crate::protocol::{
    DELTA_TOOLS_UPDATE_CAP, ElicitationCapability, InitializeResult, JsonRpcRequest,
    JsonRpcResponse, LOGGING_SET_LEVEL_METHOD, ListToolsResult, LoggingCapability, McpTool,
    PromptsCapability, ResourcesCapability, ServerCapabilities, ServerInfo, TOOLS_LIST_PAGE_SIZE,
    ToolsCapability, decode_cursor, encode_cursor, negotiate_protocol_version,
};

pub(crate) async fn dispatch_request(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Refresh session TTL on every request so active sessions are not evicted.
    if let Some(id) = session_id {
        state.sessions.touch(id);
    }
    match req.method.as_str() {
        "initialize" => handle_initialize(state, req, session_id).await,
        "notifications/initialized" => Ok(JsonRpcResponse::success(req.id.clone(), json!({}))),
        LOGGING_SET_LEVEL_METHOD => handle_logging_set_level(state, req, session_id).await,
        "tools/list" => handle_tools_list(state, req, session_id).await,
        "tools/call" => handle_tools_call(state, req, session_id).await,
        "resources/list" if state.enable_resources => handle_resources_list(state, req).await,
        "resources/read" if state.enable_resources => handle_resources_read(state, req).await,
        "resources/subscribe" if state.enable_resources => {
            handle_resources_subscribe(state, req, session_id).await
        }
        "resources/unsubscribe" if state.enable_resources => {
            handle_resources_unsubscribe(state, req, session_id).await
        }
        "prompts/list" if state.enable_prompts => handle_prompts_list(state, req).await,
        "prompts/get" if state.enable_prompts => handle_prompts_get(state, req).await,
        "elicitation/create" => handle_elicitation_create(state, req, session_id).await,
        "ping" => Ok(JsonRpcResponse::success(req.id.clone(), json!({}))),
        other => Ok(JsonRpcResponse::method_not_found(req.id.clone(), other)),
    }
}

pub(crate) async fn handle_initialize(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Create or mark session as initialized
    let sid = if let Some(id) = session_id {
        state.sessions.mark_initialized(id);
        id.to_owned()
    } else {
        let id = state.sessions.create();
        state.sessions.mark_initialized(&id);
        id
    };

    // Negotiate protocol version: honour client's preference if we support it,
    // otherwise fall back to our latest supported version.
    let client_version = req
        .params
        .as_ref()
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str());
    let negotiated = negotiate_protocol_version(client_version);

    // Store the negotiated version on the session for later handlers.
    state.sessions.set_protocol_version(&sid, negotiated);

    // Negotiate vendored delta-tools capability.
    let client_wants_delta = req
        .params
        .as_ref()
        .and_then(|p| p.get("capabilities"))
        .and_then(|c| c.get("experimental"))
        .and_then(|e| e.get(DELTA_TOOLS_UPDATE_CAP))
        .and_then(|d| d.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    state
        .sessions
        .set_supports_delta_tools(&sid, client_wants_delta);

    // Negotiate MCP roots capability (2025-03-26+).
    let client_supports_roots = req
        .params
        .as_ref()
        .and_then(|p| p.get("capabilities"))
        .and_then(|c| c.get("roots"))
        .is_some();
    state
        .sessions
        .set_supports_roots(&sid, client_supports_roots);
    if client_supports_roots {
        let sessions = state.sessions.clone();
        let sid_owned = sid.clone();
        tokio::spawn(async move {
            let _ = refresh_roots_cache_for_session(&sessions, &sid_owned).await;
        });
    }

    let experimental_caps = if client_wants_delta {
        Some(json!({ DELTA_TOOLS_UPDATE_CAP: { "enabled": true } }))
    } else {
        None
    };

    let elicitation_cap = if negotiated == "2025-06-18" {
        Some(ElicitationCapability::default())
    } else {
        None
    };

    let resources_cap = if state.enable_resources {
        Some(ResourcesCapability {
            subscribe: true,
            list_changed: true,
        })
    } else {
        None
    };

    let prompts_cap = if state.enable_prompts {
        Some(PromptsCapability { list_changed: true })
    } else {
        None
    };

    let result = InitializeResult {
        protocol_version: negotiated.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            resources: resources_cap,
            prompts: prompts_cap,
            logging: Some(LoggingCapability::default()),
            elicitation: elicitation_cap,
            experimental: experimental_caps,
        },
        server_info: ServerInfo {
            name: state.server_name.clone(),
            version: state.server_version.clone(),
        },
        instructions: Some(
            "Search skills with search_skills(query), load with load_skill(name). See get_skill_info or tools/list for details."
                .to_string(),
        ),
    };

    let mut resp = JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result)?);
    // Attach session ID via a custom field — the real header is set in the layer
    // We store it in the response id metadata for the server layer to pick up.
    // The actual Mcp-Session-Id header is injected by handle_post after this.
    // We attach it as __session_id for the outer layer.
    if let Some(obj) = resp.result.as_mut().and_then(|v| v.as_object_mut()) {
        obj.insert("__session_id".to_string(), Value::String(sid));
    }
    Ok(resp)
}

pub(crate) async fn handle_tools_list(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Check whether the client requested a forced refresh (issue #438).
    let force_refresh = req
        .params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get("dcc"))
        .and_then(|d| d.get("refresh"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let current_gen = state.current_registry_generation();

    // ── Fast path: return cached snapshot if available and still valid ──
    if state.enable_tool_cache && !force_refresh {
        if let Some(sid) = session_id {
            if let Some(snapshot) = state.sessions.get_tool_list_snapshot(sid) {
                if snapshot.generation == current_gen {
                    // Cache hit — apply cursor pagination on the cached list
                    // and return without rebuilding.
                    let cursor: usize = req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("cursor"))
                        .and_then(|v| v.as_str())
                        .and_then(decode_cursor)
                        .unwrap_or(0);
                    let total = snapshot.total;
                    let page_end = (cursor + TOOLS_LIST_PAGE_SIZE).min(total);
                    let page: Vec<McpTool> = if cursor < total {
                        snapshot.tools[cursor..page_end].to_vec()
                    } else {
                        Vec::new()
                    };
                    let next_cursor = if page_end < total {
                        Some(encode_cursor(page_end))
                    } else {
                        None
                    };
                    let result = ListToolsResult {
                        tools: page,
                        next_cursor,
                    };
                    return Ok(JsonRpcResponse::success(
                        req.id.clone(),
                        serde_json::to_value(result)?,
                    ));
                }
            }
        }
    }

    // ── Slow path: rebuild the full tool list from the registry ──

    // 1. Core discovery tools — always fully visible (static, cached once per process)
    let core = build_core_tools();
    let mut tools: Vec<McpTool> = Vec::with_capacity(core.len() + 16);
    tools.extend_from_slice(core);

    // 1b. Optional lazy-actions fast-path (#254) — three extra meta-tools that
    //     let agents drive an arbitrarily large action catalog without paging
    //     through every skill's full schema. Opt-in via
    //     `McpHttpConfig::lazy_actions`.
    if state.lazy_actions {
        tools.extend(build_lazy_action_tools());
    }

    // #242 — ``outputSchema`` is only valid on 2025-06-18 sessions. On
    // 2025-03-26 we strip it so compliant clients never see a field they
    // cannot process.
    let include_output_schema = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");

    // 2. Loaded skill tools — full definitions from ActionRegistry.
    //    Tools in inactive groups are collapsed into one ``__group__<name>``
    //    stub per group to keep ``tools/list`` compact (progressive exposure).
    let actions = state.registry.list_actions(None);

    // #307 — decide which actions can publish under their **bare name** on
    // this instance. `bare_eligible` contains `(skill, action)` tuples for
    // every action whose bare name is unique across loaded skills.
    let bare_eligible: std::collections::HashSet<(String, String)> = if state.bare_tool_names {
        let inputs: Vec<crate::gateway::namespace::BareNameInput<'_>> = actions
            .iter()
            .filter(|m| m.enabled)
            .filter_map(|m| {
                m.skill_name
                    .as_deref()
                    .map(|sn| crate::gateway::namespace::BareNameInput {
                        skill_name: sn,
                        action_name: m.name.as_str(),
                    })
            })
            .collect();
        crate::gateway::namespace::resolve_bare_names(&inputs)
    } else {
        std::collections::HashSet::new()
    };

    let mut inactive_groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for meta in &actions {
        if meta.enabled {
            tools.push(action_meta_to_mcp_tool(
                meta,
                include_output_schema,
                &bare_eligible,
                state.declared_capabilities.as_ref(),
            ));
        } else if !meta.group.is_empty() {
            inactive_groups
                .entry(meta.group.clone())
                .or_default()
                .push(meta.name.clone());
        }
    }
    for (group, names) in &inactive_groups {
        tools.push(build_group_stub(group, names));
    }

    // 3. Unloaded skills — one lightweight stub per skill.
    //    The stub lets the model see what skills exist and what tools they expose
    //    without flooding the context with full input schemas.
    //    Format: name="__skill__<skill_name>", description summarises tools,
    //    input_schema is a minimal passthrough (use load_skill to get full tools).
    let unloaded = state.catalog.list_skills(Some("unloaded"));
    for summary in &unloaded {
        tools.push(build_skill_stub(summary));
    }

    let total = tools.len();

    // ── Store the snapshot for future cache hits (issue #438) ──
    if state.enable_tool_cache {
        if let Some(sid) = session_id {
            let snapshot = crate::session::ToolListSnapshot {
                tools: tools.clone(),
                generation: current_gen,
                total,
            };
            state.sessions.set_tool_list_snapshot(sid, snapshot);
        }
    }

    // 4. Session-scoped dynamic tools (issue #462).
    //    These are intentionally excluded from the tool-list cache because
    //    they are session-specific and may change independently of the
    //    registry generation counter.
    if let Some(sid) = session_id {
        let dynamic = state.sessions.dynamic_tools_for_list(sid);
        tools.extend(dynamic);
    }

    // Recalculate total after dynamic tools are appended (they are not cached).
    let total = tools.len();

    // Cursor pagination
    let cursor: usize = req
        .params
        .as_ref()
        .and_then(|p| p.get("cursor"))
        .and_then(|v| v.as_str())
        .and_then(decode_cursor)
        .unwrap_or(0);
    let page_end = (cursor + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<McpTool> = if cursor < total {
        tools.drain(cursor..page_end).collect()
    } else {
        Vec::new()
    };
    let next_cursor = if page_end < total {
        Some(encode_cursor(page_end))
    } else {
        None
    };
    let result = ListToolsResult {
        tools: page,
        next_cursor,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}
