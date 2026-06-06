use super::*;
use dcc_mcp_gateway_core::policy::GatewayPolicyOperation;

use super::super::http_registration::entry_mcp_url;

/// Dispatch a skill-management tool across backends.
///
/// Two patterns:
/// * Fan-out, aggregate (`list_skills`, `search_skills`,
///   `get_skill_info`): call every matching backend, merge results with
///   `_instance_id` / `_dcc_type` annotations so agents can disambiguate.
/// * Target-instance (`load_skill`, `unload_skill`): require `instance_id` /
///   `dcc` in the arguments; if a single backend is live these default
///   automatically.
fn normalize_skill_mgmt_args(args: &Value) -> Value {
    let mut out = args.clone();
    if let Some(obj) = out.as_object_mut()
        && !obj.contains_key("dcc")
        && let Some(dcc) = obj.get("dcc_type").and_then(Value::as_str)
    {
        obj.insert("dcc".into(), json!(dcc));
    }
    out
}

pub(crate) async fn skill_mgmt_dispatch(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
) -> (String, bool) {
    let args = normalize_skill_mgmt_args(args);
    let dcc_filter = args.get("dcc").and_then(Value::as_str);
    let target_instance = args.get("instance_id").and_then(Value::as_str);

    match tool {
        "load_skill" | "unload_skill" | "activate_tool_group" | "deactivate_tool_group" => {
            match resolve_target(gs, target_instance, dcc_filter).await {
                Ok(entry) => {
                    let search_id = crate::gateway::search_telemetry::search_id_from_payload(&args);
                    let skill_names = requested_skill_names(&args);
                    if let Err(denial) = gs.policy.enforce_skill_operation(
                        GatewayPolicyOperation::LoadSkill,
                        Some(&entry.dcc_type),
                        skill_names.iter().map(String::as_str),
                    ) {
                        return (policy_error_text(denial), true);
                    }
                    // Strip gateway-only routing keys before forwarding.
                    let mut forward_args = args.clone();
                    if let Some(obj) = forward_args.as_object_mut() {
                        obj.remove("instance_id");
                        obj.remove("meta");
                        obj.remove("_meta");
                        obj.remove("target_tool_slug");
                        if tool == "load_skill" && !obj.contains_key("activate_groups") {
                            obj.insert("activate_groups".to_string(), Value::Bool(true));
                        }
                        if let Some(group_name) = obj.get("group_name").cloned()
                            && !obj.contains_key("group")
                        {
                            obj.insert("group".to_string(), group_name);
                        }
                    }
                    let url = entry_mcp_url(&entry);
                    let params = json!({"name": tool, "arguments": forward_args});
                    match call_backend(
                        &gs.http_client,
                        &url,
                        "tools/call",
                        Some(params),
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
                            if !is_error {
                                crate::gateway::capability_service::refresh_all_live_backends(
                                    gs,
                                    crate::gateway::capability::RefreshReason::ToolsListChanged,
                                )
                                .await;
                                if gs.events_tx.receiver_count() > 0 {
                                    let notif = serde_json::to_string(&json!({
                                        "jsonrpc": "2.0",
                                        "method": "notifications/tools/list_changed",
                                        "params": {}
                                    }))
                                    .unwrap_or_default();
                                    let _ = gs.events_tx.send(notif);
                                }
                            }
                            let text = if !is_error && tool == "load_skill" {
                                decorate_load_skill_success(
                                    gs,
                                    &entry,
                                    &args,
                                    &forward_args,
                                    &text,
                                    search_id.as_deref(),
                                )
                                .await
                            } else {
                                text
                            };
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
            let mut targets = targets_for_fanout(gs, dcc_filter).await;
            if targets.is_empty() {
                return (
                    "No live DCC instances. Start dcc-mcp-server (or your DCC adapter) so the gateway can fan out skill tools. \
Standalone `dcc-mcp-server` without `--app` registers as `dcc_type` from DCC_MCP_STANDALONE_REGISTRY_DCC_TYPE (default `python`); \
`unknown` is hidden from fan-out unless gateway `allow_unknown_tools` is enabled."
                        .to_string(),
                    true,
                );
            }
            targets.retain(|entry| gs.policy.allows_dcc(&entry.dcc_type));
            if targets.is_empty() {
                return (
                    serde_json::to_string_pretty(&json!({
                        "skills": [],
                        "total": 0,
                        "instances": [],
                    }))
                    .unwrap_or_default(),
                    false,
                );
            }
            if tool == "get_skill_info" {
                let skill_names = requested_skill_names(&args);
                if let Err(denial) = gs.policy.enforce_skill_operation(
                    GatewayPolicyOperation::Describe,
                    dcc_filter,
                    skill_names.iter().map(String::as_str),
                ) {
                    return (policy_error_text(denial), true);
                }
            }

            let client = &gs.http_client;
            let backend_timeout = gs.backend_timeout;
            let params = json!({"name": tool, "arguments": args});
            let futs = targets.iter().map(|entry| {
                let url = entry_mcp_url(entry);
                let params = params.clone();
                async move {
                    let res = call_backend(
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

            if tool == "list_skills" {
                return flatten_skill_list_results(results, &args, true, &gs.policy);
            }
            if tool == "search_skills" {
                return flatten_skill_list_results(results, &args, false, &gs.policy);
            }

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

fn flatten_skill_list_results(
    results: Vec<(Uuid, String, Result<Value, String>)>,
    args: &Value,
    apply_projection: bool,
    policy: &crate::gateway::GatewayPolicy,
) -> (String, bool) {
    let mut skills: Vec<Value> = Vec::new();
    let mut instances: Vec<Value> = Vec::new();
    let mut ok_count = 0usize;

    for (iid, dcc, res) in results {
        match res {
            Ok(value) => {
                ok_count += 1;
                let text = call_tool_text(&value)
                    .map(str::to_owned)
                    .unwrap_or_else(|| serde_json::to_string_pretty(&value).unwrap_or_default());

                match serde_json::from_str::<Value>(&text) {
                    Ok(parsed) => {
                        let before = skills.len();
                        if let Some(items) = parsed.get("skills").and_then(Value::as_array) {
                            for item in items {
                                let mut skill = item.clone();
                                inject_instance_metadata(&mut skill, &iid, &dcc);
                                if skill_allowed_by_policy(policy, &skill) {
                                    skills.push(skill);
                                }
                            }
                        }
                        let skill_count = skills.len() - before;
                        instances.push(json!({
                            "instance_id": iid.to_string(),
                            "instance_short": instance_short(&iid),
                            "dcc_type": dcc,
                            "skill_count": skill_count,
                            "total": parsed.get("total").cloned().unwrap_or(json!(skill_count)),
                        }));
                    }
                    Err(_) => {
                        instances.push(json!({
                            "instance_id": iid.to_string(),
                            "instance_short": instance_short(&iid),
                            "dcc_type": dcc,
                            "skill_count": 0,
                            "message": text,
                        }));
                    }
                }
            }
            Err(error) => {
                instances.push(json!({
                    "instance_id": iid.to_string(),
                    "instance_short": instance_short(&iid),
                    "dcc_type": dcc,
                    "error": error,
                }));
            }
        }
    }

    let total = skills.len();
    let mut payload = json!({
        "skills": skills,
        "total": total,
        "instances": instances,
    });
    if apply_projection {
        payload =
            dcc_mcp_skills::catalog::list_projection::project_list_skills_payload(payload, args);
    }
    (
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
        ok_count == 0,
    )
}

fn requested_skill_names(args: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = args.get("skill_name").and_then(Value::as_str) {
        names.push(name.to_string());
    }
    if let Some(items) = args.get("skill_names").and_then(Value::as_array) {
        names.extend(items.iter().filter_map(Value::as_str).map(str::to_string));
    }
    names
}

fn skill_allowed_by_policy(policy: &crate::gateway::GatewayPolicy, skill: &Value) -> bool {
    let dcc_allowed = skill
        .get("_dcc_type")
        .and_then(Value::as_str)
        .is_none_or(|dcc| policy.allows_dcc(dcc));
    let name = skill
        .get("name")
        .or_else(|| skill.get("skill_name"))
        .or_else(|| skill.get("skill"))
        .and_then(Value::as_str);
    dcc_allowed && policy.allows_skill(name)
}

fn policy_error_text(denial: crate::gateway::GatewayPolicyDenial) -> String {
    let err = crate::gateway::capability_service::policy_denied_error(denial);
    serde_json::to_string_pretty(&crate::gateway::capability_service::service_error_to_json(
        &err,
    ))
    .unwrap_or_else(|_| "policy-denied".to_string())
}

fn call_tool_text(value: &Value) -> Option<&str> {
    value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
}

/// JSON-Schema definitions for legacy skill-management tools still routed
/// via `tools/call` aliases (not published on `tools/list` after RFC #998).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn skill_management_tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "list_skills",
            "description": "List all skills across every live DCC instance. Returns a per-instance breakdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["all", "loaded", "unloaded", "pending_deps", "error"], "default": "all"},
                    "dcc":    {"type": "string", "description": "Restrict to one DCC type (maya, blender, …)"},
                    "dcc_type": {"type": "string", "description": "Alias for dcc (REST callers)."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                    "offset": {"type": "integer", "minimum": 0, "default": 0},
                    "fields": {"type": "array", "items": {"type": "string"}, "description": "Strict per-skill field allow-list; omit for compact mode."}
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
                    "skill_name":      {"type": "string"},
                    "skill_names":     {"type": "array", "items": {"type": "string"}},
                    "activate_groups": {"type": "boolean", "default": true, "description": "Gateway default activates all declared tool groups. Set false for lazy loading when you only want default-active/core groups."},
                    "instance_id":     {"type": "string", "description": "Target instance (full UUID or short prefix)"},
                    "dcc":             {"type": "string", "description": "DCC type when only one instance of that type is live"}
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
        json!({
            "name": "activate_tool_group",
            "description": "Activate a progressive tool group on a specific DCC instance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group_name":  {"type": "string"},
                    "group":       {"type": "string", "description": "Alias of group_name"},
                    "skill_name":  {"type": "string", "description": "Optional disambiguation for clients"},
                    "instance_id": {"type": "string"},
                    "dcc":         {"type": "string"}
                },
                "required": ["group_name"]
            }
        }),
        json!({
            "name": "deactivate_tool_group",
            "description": "Deactivate a progressive tool group on a specific DCC instance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group_name":  {"type": "string"},
                    "group":       {"type": "string", "description": "Alias of group_name"},
                    "skill_name":  {"type": "string", "description": "Optional disambiguation for clients"},
                    "instance_id": {"type": "string"},
                    "dcc":         {"type": "string"}
                },
                "required": ["group_name"]
            }
        }),
    ]
}

async fn decorate_load_skill_success(
    gs: &GatewayState,
    entry: &ServiceEntry,
    request_args: &Value,
    forwarded_args: &Value,
    text: &str,
    search_id: Option<&str>,
) -> String {
    let mut payload = serde_json::from_str::<Value>(text).unwrap_or_else(|_| {
        json!({
            "message": text,
        })
    });

    if !payload.is_object() {
        payload = json!({ "result": payload });
    }

    let Some(obj) = payload.as_object_mut() else {
        return text.to_string();
    };

    let requested_skill = forwarded_args
        .get("skill_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            forwarded_args
                .get("skill_names")
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find_map(Value::as_str))
                .map(str::to_string)
        })
        .or_else(|| {
            obj.get("skill_name")
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    obj.entry("loaded".to_string()).or_insert(Value::Bool(true));
    if let Some(skill_name) = &requested_skill {
        obj.entry("skill_name".to_string())
            .or_insert_with(|| Value::String(skill_name.clone()));
    }
    obj.insert(
        "dcc_type".to_string(),
        Value::String(entry.dcc_type.clone()),
    );
    obj.insert(
        "instance_id".to_string(),
        Value::String(entry.instance_id.to_string()),
    );
    obj.insert(
        "instance_short".to_string(),
        Value::String(instance_short(&entry.instance_id)),
    );

    let tool_slugs = new_tool_slugs_for_skill(gs, entry.instance_id, requested_skill.as_deref());
    let target_tool_slug = request_args
        .get("target_tool_slug")
        .and_then(Value::as_str)
        .filter(|slug| tool_slugs.iter().any(|candidate| candidate == *slug))
        .map(str::to_string);
    obj.insert("new_tool_slugs".to_string(), json!(tool_slugs));
    obj.insert(
        "index_generation".to_string(),
        Value::String(crate::gateway::capability_service::index_generation(
            &gs.capability_index,
        )),
    );

    if !obj.contains_key("activated_groups") {
        let active_groups = obj
            .get("active_groups")
            .cloned()
            .unwrap_or_else(|| json!([]));
        obj.insert("activated_groups".to_string(), active_groups);
    }

    let selected_tool_slug = target_tool_slug.or_else(|| {
        obj.get("new_tool_slugs")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let compact_schema =
        inline_compact_schema_for_correlated_load(gs, selected_tool_slug.as_deref(), search_id)
            .await;
    if let Some(schema) = compact_schema.as_ref() {
        obj.insert("compact_schema".to_string(), schema.clone());
    }

    let next_step = suggested_post_load_next_step(
        gs,
        requested_skill.as_deref(),
        &entry.dcc_type,
        entry.instance_id,
        selected_tool_slug.as_deref(),
        search_id,
        obj.get("index_generation").and_then(Value::as_str),
        compact_schema.as_ref(),
    );
    obj.insert("next_step".to_string(), next_step);

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| text.to_string())
}

fn new_tool_slugs_for_skill(
    gs: &GatewayState,
    instance_id: Uuid,
    skill_name: Option<&str>,
) -> Vec<String> {
    let mut slugs: Vec<String> = gs
        .capability_index
        .snapshot()
        .records
        .iter()
        .filter(|record| record.instance_id == instance_id && record.loaded)
        .filter(|record| {
            skill_name.is_none_or(|skill| {
                record
                    .skill_name
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(skill))
            })
        })
        .map(|record| record.tool_slug.clone())
        .collect();
    slugs.sort();
    slugs
}

#[allow(clippy::too_many_arguments)]
fn suggested_post_load_next_step(
    gs: &GatewayState,
    skill_name: Option<&str>,
    dcc_type: &str,
    instance_id: Uuid,
    first_tool_slug: Option<&str>,
    search_id: Option<&str>,
    index_generation: Option<&str>,
    compact_schema: Option<&Value>,
) -> Value {
    if let Some(tool_slug) = first_tool_slug {
        if compact_schema.is_some() {
            let mut arguments = json!({ "tool_slug": tool_slug, "arguments": {} });
            attach_search_meta(&mut arguments, search_id, index_generation);
            return json!({
                "action": "call",
                "arguments": arguments.clone(),
                "mcp": {
                    "tool": "call",
                    "arguments": arguments.clone(),
                    "_meta": arguments.get("meta").cloned().unwrap_or(Value::Null),
                },
                "rest": {
                    "method": "POST",
                    "path": "/v1/call",
                    "body": arguments,
                },
                "schema_source": "load_skill.compact_schema",
            });
        }

        if let Ok(record) =
            crate::gateway::capability_service::describe_service(&gs.capability_index, tool_slug)
            && !record.has_schema
        {
            let mut arguments = json!({ "tool_slug": tool_slug, "arguments": {} });
            attach_search_meta(&mut arguments, search_id, index_generation);
            return json!({
                "action": "call",
                "arguments": arguments.clone(),
                "mcp": {
                    "tool": "call",
                    "arguments": arguments.clone(),
                    "_meta": arguments.get("meta").cloned().unwrap_or(Value::Null),
                },
                "rest": {
                    "method": "POST",
                    "path": "/v1/call",
                    "body": arguments,
                },
            });
        }

        let mut arguments = json!({ "tool_slug": tool_slug });
        attach_search_meta(&mut arguments, search_id, index_generation);
        return json!({
            "action": "describe",
            "arguments": arguments.clone(),
            "mcp": {
                "tool": "describe",
                "arguments": arguments.clone(),
                "_meta": arguments.get("meta").cloned().unwrap_or(Value::Null),
            },
            "rest": {
                "method": "POST",
                "path": "/v1/describe",
                "body": arguments,
            },
        });
    }

    let mut arguments = json!({
        "query": skill_name.unwrap_or_default(),
        "skill_hint": skill_name.unwrap_or_default(),
        "dcc_type": dcc_type,
        "instance_id": instance_id.to_string(),
        "loaded_only": true,
    });
    attach_search_meta(&mut arguments, search_id, index_generation);
    json!({
        "action": "search",
        "arguments": arguments.clone(),
        "mcp": {
            "tool": "search",
            "arguments": arguments.clone(),
            "_meta": arguments.get("meta").cloned().unwrap_or(Value::Null),
        },
        "rest": {
            "method": "POST",
            "path": "/v1/search",
            "body": arguments,
        },
    })
}

async fn inline_compact_schema_for_correlated_load(
    gs: &GatewayState,
    tool_slug: Option<&str>,
    search_id: Option<&str>,
) -> Option<Value> {
    let tool_slug = tool_slug?;
    search_id?;
    let record =
        crate::gateway::capability_service::describe_service(&gs.capability_index, tool_slug)
            .ok()?;
    if !record.has_schema {
        return Some(compact_schema_payload(
            tool_slug,
            &record,
            &json!({"type": "object"}),
        ));
    }
    let (record, tool) = crate::gateway::capability_service::describe_tool_full(gs, tool_slug)
        .await
        .ok()?;
    Some(compact_schema_payload(
        tool_slug,
        &record,
        &tool.input_schema,
    ))
}

fn compact_schema_payload(
    tool_slug: &str,
    record: &crate::gateway::capability::CapabilityRecord,
    input_schema: &Value,
) -> Value {
    let required = input_schema
        .get("required")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let properties = input_schema
        .get("properties")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let property_keys: Vec<String> = properties
        .as_object()
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default();
    json!({
        "tool_slug": tool_slug,
        "has_schema": record.has_schema,
        "required": required,
        "property_keys": property_keys,
        "properties": properties,
    })
}

fn attach_search_meta(
    arguments: &mut Value,
    search_id: Option<&str>,
    index_generation: Option<&str>,
) {
    let Some(search_id) = search_id else {
        return;
    };
    let mut meta = json!({
        "search_id": search_id,
        "ranker_version": crate::gateway::search_telemetry::RANKER_VERSION,
    });
    if let Some(generation) = index_generation.filter(|value| !value.is_empty()) {
        meta["index_generation"] = json!(generation);
    }
    if let Some(obj) = arguments.as_object_mut() {
        obj.insert("meta".to_string(), meta);
    }
}
