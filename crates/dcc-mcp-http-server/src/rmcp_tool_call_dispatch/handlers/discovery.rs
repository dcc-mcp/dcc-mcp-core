//! Tool discovery MCP handlers (search_tools, lazy list_actions).

use serde_json::{Value, json};

use dcc_mcp_jsonrpc::CallToolResult;

use crate::server_state::ServerState;

pub(in crate::rmcp_tool_call_dispatch) fn is_progressive_stub(name: &str) -> bool {
    crate::mcp_tool_catalog::is_progressive_tool_stub(name)
}

pub(in crate::rmcp_tool_call_dispatch) fn schema_property_names(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_search_tools(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let query_raw = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if query_raw.is_empty() {
        return CallToolResult::error("Missing required parameter: query");
    }
    let query = query_raw.to_lowercase();

    let dcc = arguments.get("dcc").and_then(Value::as_str);
    let include_disabled = arguments
        .get("include_disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_stubs = arguments
        .get("include_stubs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_unloaded_skills = arguments
        .get("include_unloaded_skills")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(25);

    let mut tool_hits: Vec<Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !include_disabled && !meta.enabled {
            continue;
        }
        if !include_stubs && is_progressive_stub(&meta.name) {
            continue;
        }
        let schema_props = schema_property_names(&meta.input_schema);
        let haystack = format!(
            "{} {} {} {} {}",
            meta.name,
            meta.description,
            meta.category,
            meta.tags.join(" "),
            schema_props.join(" ")
        )
        .to_lowercase();
        if !haystack.contains(&query) {
            continue;
        }
        let mut hit = json!({
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

    if include_stubs && tool_hits.len() < limit {
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
                summary.tool_names.join(" ")
            )
            .to_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
            tool_hits.push(json!({
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
                tool_hits.push(json!({
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

    let mut skill_candidates: Vec<Value> = Vec::new();
    if include_unloaded_skills {
        let candidates = state
            .catalog
            .search_skills(Some(query_raw), &[], dcc, None, Some(limit));
        for summary in candidates {
            if summary.loaded {
                continue;
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
            skill_candidates.push(json!({
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
    let result = json!({
        "total": total,
        "query": query,
        "tools": tool_hits,
        "skill_candidates": skill_candidates,
    });
    CallToolResult::text(serde_json::to_string(&result).unwrap_or_default())
}
