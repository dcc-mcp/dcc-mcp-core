//! Skill lifecycle and group MCP tool handlers.

use serde_json::{Value, json};

use crate::mcp_tool_catalog::parse_scope_label;
use crate::rmcp_registry_context::RegistryContext;
use crate::server_state::ServerState;
use dcc_mcp_jsonrpc::CallToolResult;

use super::super::helpers::notify_tools_changed;
pub(in crate::rmcp_tool_call_dispatch) fn handle_list_roots(
    state: &ServerState,
    session_id: Option<&str>,
) -> CallToolResult {
    let Some(session) = session_id else {
        return CallToolResult::error("list_roots requires Mcp-Session-Id header");
    };
    let roots = state.sessions.get_client_roots(session);
    let payload = json!({
        "supports_roots": state.sessions.supports_roots(session),
        "count": roots.len(),
        "roots": roots,
    });
    CallToolResult::text(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_list_skills(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let status = arguments.get("status").and_then(Value::as_str);
    let results = state.catalog.list_skills(status);
    let payload =
        dcc_mcp_skills::catalog::list_projection::build_list_skills_response(results, arguments);
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    CallToolResult::text(text)
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_get_skill_info(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if skill_name.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name");
    }
    match state.catalog.get_skill_info(skill_name) {
        Some(info) => {
            let text = serde_json::to_string_pretty(&info).unwrap_or_default();
            CallToolResult::text(text)
        }
        None => CallToolResult::error(format!("Skill '{skill_name}' not found")),
    }
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_load_skill(
    state: &ServerState,
    ctx: &RegistryContext,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let skill_names: Vec<String> = arguments
        .get("skill_names")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if skill_name.is_empty() && skill_names.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name or skill_names");
    }

    let activate_groups = arguments
        .get("activate_groups")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut requested: Vec<String> = Vec::new();
    if !skill_name.is_empty() {
        requested.push(skill_name.to_string());
    }
    for name in &skill_names {
        if !requested.contains(name) {
            requested.push(name.clone());
        }
    }

    let mut all_registered_tools: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut newly_loaded: Vec<String> = Vec::new();
    let mut already_loaded: Vec<String> = Vec::new();

    for name in &requested {
        let was_loaded = state.catalog.is_loaded(name);
        match state.catalog.load_skill_with_options(name, activate_groups) {
            Ok(tools) => {
                all_registered_tools.extend(tools);
                if was_loaded {
                    already_loaded.push(name.clone());
                } else {
                    newly_loaded.push(name.clone());
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }

    if !newly_loaded.is_empty() {
        state.bump_registry_generation();
        if let Some(sid) = session_id {
            let added = all_registered_tools.clone();
            let removed: Vec<String> = newly_loaded
                .iter()
                .map(|n| format!("__skill__{n}"))
                .collect();
            notify_tools_changed(&state.sessions, sid, &added, &removed);
        }
        (ctx.on_skill_catalog_mutated)();
    }

    let mut tool_schemas: Vec<Value> = Vec::new();
    for name in newly_loaded.iter().chain(already_loaded.iter()) {
        for meta in state.catalog.registry().list_actions_by_skill(name) {
            tool_schemas.push(json!({
                "name":          meta.name,
                "description":   meta.description,
                "inputSchema":   meta.input_schema,
                "outputSchema":  meta.output_schema,
                "skill_name":    meta.skill_name,
            }));
        }
    }

    let loaded_ok = !all_registered_tools.is_empty();
    let partial = loaded_ok && !errors.is_empty();
    let available_groups = group_state_payloads(&state.catalog, &requested);
    let activated_groups: Vec<String> = available_groups
        .iter()
        .filter(|group| {
            group
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|group| {
            group
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let mut body = json!({
        "loaded":           loaded_ok,
        "partial":          partial,
        "registered_tools": all_registered_tools,
        "tool_count":       all_registered_tools.len(),
        "newly_loaded":     newly_loaded,
        "already_loaded":   already_loaded,
        "available_groups":  available_groups,
        "activated_groups":  activated_groups,
        "tools":            tool_schemas,
    });
    if !errors.is_empty()
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert("errors".to_string(), json!(errors));
    }

    let text = serde_json::to_string_pretty(&body).unwrap_or_default();
    if loaded_ok {
        CallToolResult::text(text)
    } else {
        CallToolResult::error(text)
    }
}

fn group_state_payloads(
    catalog: &dcc_mcp_skills::SkillCatalog,
    skill_names: &[String],
) -> Vec<Value> {
    let active_by_skill_group: std::collections::HashMap<(String, String), bool> = catalog
        .list_groups()
        .into_iter()
        .map(|(skill, group, active)| ((skill, group), active))
        .collect();
    let mut groups = Vec::new();
    for skill_name in skill_names {
        let Some(detail) = catalog.get_skill_info(skill_name) else {
            continue;
        };
        for group in detail.groups {
            if group.name.is_empty() {
                continue;
            }
            let active = active_by_skill_group
                .get(&(detail.name.clone(), group.name.clone()))
                .copied()
                .unwrap_or(false);
            groups.push(json!({
                "name": group.name,
                "description": group.description,
                "tools": group.tools,
                "default_active": group.default_active,
                "active": active,
            }));
        }
    }
    groups
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_unload_skill(
    state: &ServerState,
    ctx: &RegistryContext,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if skill_name.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name");
    }

    match state.catalog.unload_skill(skill_name) {
        Ok(count) => {
            state.bump_registry_generation();
            if let Some(sid) = session_id {
                let removed: Vec<String> = state
                    .registry
                    .list_actions_by_skill(skill_name)
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let added = vec![format!("__skill__{skill_name}")];
                notify_tools_changed(&state.sessions, sid, &added, &removed);
            }
            (ctx.on_skill_catalog_mutated)();
            let text = serde_json::to_string_pretty(&json!({
                "unloaded": true,
                "tools_removed": count
            }))
            .unwrap_or_default();
            CallToolResult::text(text)
        }
        Err(e) => CallToolResult::error(e),
    }
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_search_skills(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let tags_owned: Vec<String> = arguments
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let tags: Vec<&str> = tags_owned.iter().map(String::as_str).collect();

    let dcc_filter = arguments.get("dcc").and_then(Value::as_str);

    let scope_filter = match arguments.get("scope").and_then(Value::as_str) {
        None => None,
        Some(s) => match parse_scope_label(s) {
            Ok(sc) => Some(sc),
            Err(msg) => return CallToolResult::error(msg),
        },
    };

    let limit = arguments
        .get("limit")
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
        return CallToolResult::text(text);
    }

    let compact_skills: Vec<Value> = matches
        .iter()
        .map(|s| {
            json!({
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

    let result = json!({
        "total": matches.len(),
        "query": query,
        "skills": compact_skills
    });

    CallToolResult::text(serde_json::to_string(&result).unwrap_or_default())
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_activate_tool_group(
    state: &ServerState,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let group = arguments
        .get("group")
        .or_else(|| arguments.get("group_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return CallToolResult::error("Missing required parameter: group or group_name");
    }
    let changed = state.catalog.activate_group(group);
    state.bump_registry_generation();
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
    CallToolResult::text(
        json!({
            "success": true,
            "group": group,
            "changed": changed,
            "active_groups": state.catalog.active_groups(),
        })
        .to_string(),
    )
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_deactivate_tool_group(
    state: &ServerState,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let group = arguments
        .get("group")
        .or_else(|| arguments.get("group_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return CallToolResult::error("Missing required parameter: group or group_name");
    }
    let changed = state.catalog.deactivate_group(group);
    state.bump_registry_generation();
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
    CallToolResult::text(
        json!({
            "success": true,
            "group": group,
            "changed": changed,
            "active_groups": state.catalog.active_groups(),
        })
        .to_string(),
    )
}
