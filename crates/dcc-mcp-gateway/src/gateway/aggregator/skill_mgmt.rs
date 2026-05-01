use super::*;

/// Dispatch a skill-management tool across backends.
///
/// Two patterns:
/// * Fan-out, aggregate (`list_skills`, `search_skills`,
///   `get_skill_info`): call every matching backend, merge results with
///   `_instance_id` / `_dcc_type` annotations so agents can disambiguate.
/// * Target-instance (`load_skill`, `unload_skill`): require `instance_id` /
///   `dcc` in the arguments; if a single backend is live these default
///   automatically.
pub(crate) async fn skill_mgmt_dispatch(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
) -> (String, bool) {
    let dcc_filter = args.get("dcc").and_then(Value::as_str);
    let target_instance = args.get("instance_id").and_then(Value::as_str);

    match tool {
        "load_skill" | "unload_skill" => {
            match resolve_target(gs, target_instance, dcc_filter).await {
                Ok(entry) => {
                    // Strip gateway-only routing keys before forwarding.
                    let mut forward_args = args.clone();
                    if let Some(obj) = forward_args.as_object_mut() {
                        obj.remove("instance_id");
                    }
                    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                    match forward_tools_call(
                        &gs.http_client,
                        &url,
                        tool,
                        Some(forward_args),
                        None,
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
            let targets = targets_for_fanout(gs, dcc_filter).await;
            if targets.is_empty() {
                return (
                    "No live DCC instances. Start dcc-mcp-server on the DCC you want to use."
                        .to_string(),
                    true,
                );
            }

            let client = &gs.http_client;
            let backend_timeout = gs.backend_timeout;
            let params = json!({"name": tool, "arguments": args});
            let futs = targets.iter().map(|entry| {
                let url = format!("http://{}:{}/mcp", entry.host, entry.port);
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

            if matches!(tool, "list_skills" | "search_skills") {
                return flatten_skill_list_results(results);
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
                                skills.push(skill);
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
    let payload = json!({
        "skills": skills,
        "total": total,
        "instances": instances,
    });
    (
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
        ok_count == 0,
    )
}

fn call_tool_text(value: &Value) -> Option<&str> {
    value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
}

/// JSON-Schema definitions for the six skill-management tools the gateway
/// exposes (matching the per-DCC server schemas but with gateway-specific
/// routing parameters like `instance_id` and `dcc`).
pub(crate) fn skill_management_tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "list_skills",
            "description": "List all skills across every live DCC instance. Returns a per-instance breakdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["all", "loaded", "unloaded", "error"], "default": "all"},
                    "dcc":    {"type": "string", "description": "Restrict to one DCC type (maya, blender, …)"}
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
                    "skill_name":  {"type": "string"},
                    "skill_names": {"type": "array", "items": {"type": "string"}},
                    "instance_id": {"type": "string", "description": "Target instance (full UUID or short prefix)"},
                    "dcc":         {"type": "string", "description": "DCC type when only one instance of that type is live"}
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
    ]
}
