//! Local FileRegistry + direct MCP control path for `dcc-mcp-cli`.
//!
//! Remote profiles use the gateway REST surface. The built-in `local` profile
//! resolves live instances from the shared FileRegistry, then talks to the
//! selected DCC instance's MCP HTTP endpoint directly.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context;
use dcc_mcp_jsonrpc::{JsonRpcRequestBuilder, MCP_PROTOCOL_VERSION};
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::{Map, Value, json};

use crate::application::local_instance;
use crate::domain::rest::{
    Endpoint, ReloadSkillsRequest, SearchRequest, StopInstanceRequest, WaitReadyRequest,
};
use crate::infra::http::HttpGateway;

const MCP_ACCEPT: &str = "application/json, text/event-stream";
const DEFAULT_REQUIRED_READINESS_FIELDS: &[&str] =
    &["process", "dcc", "skill_catalog", "dispatcher"];

pub async fn search_local(registry_dir: PathBuf, request: SearchRequest) -> anyhow::Result<Value> {
    let entries = local_instance::select_routable_entries(
        &registry_dir,
        request.dcc_type.as_deref(),
        request.instance_id.as_deref(),
    )?;
    let gateway = HttpGateway::default();
    let mut hits = Vec::new();
    let mut total_matches = 0_usize;
    let mut truncated = false;
    let query = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let limit = effective_search_limit(query.as_deref(), request.limit);

    for entry in &entries {
        let discovery_mcp_url = local_instance::discovery_mcp_url(entry);
        let payload = if let Some(query) = query.as_deref() {
            let search_result = mcp_call_tool(
                &gateway,
                &discovery_mcp_url,
                "search_tools",
                json!({
                    "query": query,
                    "dcc": entry.dcc_type,
                    "limit": limit,
                    "include_disabled": true,
                }),
                None,
            )
            .await
            .with_context(|| {
                format!(
                    "searching local {} instance {}",
                    entry.dcc_type,
                    local_instance::instance_short(entry)
                )
            })?;
            call_result_payload(&search_result).unwrap_or(search_result)
        } else {
            let tools = list_mcp_tools(&gateway, &discovery_mcp_url)
                .await
                .with_context(|| {
                    format!(
                        "listing local {} instance {} tools",
                        entry.dcc_type,
                        local_instance::instance_short(entry)
                    )
                })?;
            json!({ "tools": tools })
        };
        let previous = hits.len();
        extend_tool_hits(&mut hits, entry, &payload);
        extend_skill_hits(&mut hits, entry, &payload);
        let payload_returned = hits.len() - previous;
        let payload_matches = payload
            .get("total_matches")
            .or_else(|| payload.get("total"))
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(payload_returned)
            .max(payload_returned);
        total_matches += payload_matches;
        truncated |= payload.get("truncated").and_then(Value::as_bool) == Some(true)
            || payload_returned < payload_matches;
    }

    hits.sort_by(|left, right| {
        let left_key = left
            .get("slug")
            .and_then(Value::as_str)
            .or_else(|| left.get("skill_name").and_then(Value::as_str));
        let right_key = right
            .get("slug")
            .and_then(Value::as_str)
            .or_else(|| right.get("skill_name").and_then(Value::as_str));
        left_key.cmp(&right_key).then_with(|| {
            left.get("instance_id")
                .and_then(Value::as_str)
                .cmp(&right.get("instance_id").and_then(Value::as_str))
        })
    });
    if hits.len() > limit {
        hits.truncate(limit);
        truncated = true;
    }
    let returned = hits.len();

    Ok(json!({
        "total": returned,
        "total_matches": total_matches,
        "returned": returned,
        "truncated": truncated || returned < total_matches,
        "hits": hits,
        "source": "local_mcp",
        "registry_dir": registry_dir,
        "query": request.query,
    }))
}

fn effective_search_limit(query: Option<&str>, requested: Option<usize>) -> usize {
    match requested {
        Some(limit) => limit.clamp(1, 100),
        None if query.is_none() => usize::MAX,
        None => 25,
    }
}

pub async fn describe_local(registry_dir: PathBuf, tool_slug: String) -> anyhow::Result<Value> {
    let route = resolve_tool_route(&registry_dir, &tool_slug, None, None)?;
    let gateway = HttpGateway::default();
    let discovery_mcp_url = local_instance::discovery_mcp_url(&route.entry);
    let endpoint = Endpoint::from_mcp_url(&discovery_mcp_url);
    let tool = match gateway
        .post_json(
            &endpoint.path("/v1/describe"),
            &json!({"tool_slug": route.backend_tool, "include_schema": true}),
        )
        .await
    {
        Ok(described) => json!({
            "name": route.backend_tool,
            "description": described.get("description").cloned().unwrap_or(Value::Null),
            "inputSchema": described.get("input_schema").cloned().unwrap_or_else(|| json!({"type": "object"})),
            "annotations": described.get("annotations").cloned().unwrap_or_else(|| json!({})),
            "_meta": described.get("metadata").cloned().unwrap_or(Value::Null),
        }),
        Err(describe_error) => list_mcp_tools(&gateway, &discovery_mcp_url)
            .await
            .with_context(|| {
                format!(
                    "local describe failed for '{}' and tools/list fallback also failed: {describe_error}",
                    route.backend_tool
                )
            })?
            .into_iter()
            .find(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name == route.backend_tool)
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "tool '{}' was not found after local describe failed: {describe_error}",
                    route.backend_tool
                )
            })?,
    };

    Ok(json!({
        "record": route.record(),
        "tool": tool,
        "instance": local_instance::instance_summary(&route.entry),
        "source": "local_mcp",
    }))
}

pub async fn load_skill_local(registry_dir: PathBuf, body: Value) -> anyhow::Result<Value> {
    let LocalLoadSkillRequest {
        dcc_type,
        instance_id,
        target_tool_slug,
        mut backend_body,
    } = split_load_skill_request(body)?;
    let entry = local_instance::select_one_routable_entry(
        &registry_dir,
        dcc_type.as_deref(),
        instance_id.as_deref(),
    )?;
    let gateway = HttpGateway::default();
    let mcp_url = local_instance::mcp_url(&entry);
    let tool_group = backend_body
        .get("tool_group")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(str::to_string);
    let skill_name = backend_body
        .get("skill_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    if tool_group.is_some()
        && backend_body.get("activate_groups").is_none()
        && let Some(object) = backend_body.as_object_mut()
    {
        object.insert("activate_groups".to_string(), Value::Bool(false));
    }
    let result = mcp_call_tool(&gateway, &mcp_url, "load_skill", backend_body, None)
        .await
        .with_context(|| {
            format!(
                "loading skill on local {} instance {}",
                entry.dcc_type,
                local_instance::instance_short(&entry)
            )
        })?;
    if !call_result_succeeded(&result) {
        anyhow::bail!("loading skill failed: {result}");
    }
    let mut payload = call_result_payload(&result).unwrap_or(result);
    if payload.get("loaded").and_then(Value::as_bool) == Some(false) {
        anyhow::bail!("loading skill failed: {payload}");
    }
    if let Some(group) = tool_group.as_deref()
        && !payload
            .get("activated_groups")
            .and_then(Value::as_array)
            .is_some_and(|groups| groups.iter().any(|value| value.as_str() == Some(group)))
    {
        let mut arguments = json!({"group_name": group});
        if let Some(skill_name) = skill_name.as_deref()
            && let Some(object) = arguments.as_object_mut()
        {
            object.insert("skill_name".to_string(), json!(skill_name));
        }
        let activation = mcp_call_tool(&gateway, &mcp_url, "activate_tool_group", arguments, None)
            .await
            .with_context(|| {
                format!(
                    "activating tool group '{group}' on local {} instance {}",
                    entry.dcc_type,
                    local_instance::instance_short(&entry)
                )
            })?;
        if !call_result_succeeded(&activation) {
            anyhow::bail!("activating tool group '{group}' failed: {activation}");
        }
        if let Some(groups) = payload
            .as_object_mut()
            .map(|object| {
                object
                    .entry("activated_groups")
                    .or_insert_with(|| json!([]))
            })
            .and_then(Value::as_array_mut)
        {
            groups.push(json!(group));
        }
    }
    attach_local_context(&mut payload, &entry, None, "local_mcp");
    let target_tool_slug = target_tool_slug.or_else(|| {
        payload
            .get("registered_tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find(|name| !name.trim().is_empty())
            .or_else(|| {
                payload
                    .get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .find(|name| !name.trim().is_empty())
            })
            .map(|name| local_instance::local_tool_slug(&entry, name))
    });
    if let Some(target_tool_slug) = target_tool_slug {
        attach_local_post_load_hint(
            &mut payload,
            &entry,
            &target_tool_slug,
            skill_name.as_deref(),
        );
    }
    Ok(payload)
}

struct LocalLoadSkillRequest {
    dcc_type: Option<String>,
    instance_id: Option<String>,
    target_tool_slug: Option<String>,
    backend_body: Value,
}

fn split_load_skill_request(body: Value) -> anyhow::Result<LocalLoadSkillRequest> {
    let Value::Object(mut object) = body else {
        anyhow::bail!("load-skill local request body must be a JSON object");
    };

    let dcc_type = object
        .get("dcc_type")
        .or_else(|| object.get("dcc"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let instance_id = object
        .get("instance_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let target_tool_slug = object
        .get("target_tool_slug")
        .and_then(Value::as_str)
        .map(str::to_string);

    object.remove("dcc_type");
    object.remove("dcc");
    object.remove("instance_id");
    object.remove("target_tool_slug");
    object.remove("meta");
    object.remove("_meta");
    if target_tool_slug.is_some()
        && ["tool_group", "group", "group_name"].iter().any(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|group| !group.trim().is_empty())
        })
    {
        object.insert("strict_tool_group".to_string(), Value::Bool(true));
    }

    Ok(LocalLoadSkillRequest {
        dcc_type,
        instance_id,
        target_tool_slug,
        backend_body: Value::Object(object),
    })
}

fn attach_local_post_load_hint(
    payload: &mut Value,
    entry: &ServiceEntry,
    target_tool_slug: &str,
    requested_skill_name: Option<&str>,
) {
    if payload.get("loaded").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let requested_backend_tool = parse_local_tool_slug(target_tool_slug).backend_tool;
    let backend_tool =
        resolve_loaded_backend_tool(payload, &requested_backend_tool, requested_skill_name);
    let target_tool_slug = local_instance::local_tool_slug(entry, &backend_tool);
    let selected_tool = payload
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name == backend_tool)
            })
        });
    let input_schema = selected_tool
        .and_then(|tool| tool.get("inputSchema").or_else(|| tool.get("input_schema")))
        .filter(|schema| schema.is_object())
        .cloned();
    let annotations = selected_tool
        .and_then(|tool| tool.get("annotations"))
        .cloned()
        .unwrap_or(Value::Null);
    let metadata = selected_tool
        .and_then(|tool| tool.get("metadata"))
        .cloned()
        .unwrap_or(Value::Null);

    let Some(object) = payload.as_object_mut() else {
        return;
    };
    let next_step = if let Some(input_schema) = input_schema {
        let complete_for_call = simple_object_schema(&input_schema);
        let safe_to_call = complete_for_call && has_safety_hints(&annotations);
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
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default();
        object.insert(
            "compact_schema".to_string(),
            json!({
                "tool_slug": &target_tool_slug,
                "has_schema": schema_has_constraints(&input_schema),
                "complete_for_call": complete_for_call,
                "required": required,
                "property_keys": property_keys,
                "properties": properties,
                "annotations": annotations,
                "metadata": metadata,
            }),
        );
        if safe_to_call {
            json!({
                "action": "call",
                "arguments": {"tool_slug": &target_tool_slug, "arguments": {}},
                "schema_source": "load_skill.compact_schema",
            })
        } else {
            json!({
                "action": "describe",
                "arguments": {"tool_slug": &target_tool_slug},
            })
        }
    } else {
        json!({
            "action": "describe",
            "arguments": {"tool_slug": &target_tool_slug},
        })
    };
    object.insert("next_step".to_string(), next_step);
}

fn resolve_loaded_backend_tool(
    payload: &Value,
    requested_backend_tool: &str,
    requested_skill_name: Option<&str>,
) -> String {
    let mut candidates: Vec<&str> = payload
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .chain(
            payload
                .get("registered_tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .collect();
    candidates.sort_unstable();
    candidates.dedup();

    if candidates.contains(&requested_backend_tool) {
        return requested_backend_tool.to_string();
    }

    let skill_name = requested_skill_name.or_else(|| {
        payload
            .get("skill_name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
    });
    let Some(skill_name) = skill_name else {
        return requested_backend_tool.to_string();
    };
    let canonical_prefix = format!("{}__", skill_name.replace('-', "_"));
    let requested_action = requested_backend_tool
        .strip_prefix(&canonical_prefix)
        .unwrap_or(requested_backend_tool)
        .replace('-', "_");
    let mut matches = candidates.into_iter().filter(|candidate| {
        candidate
            .strip_prefix(&canonical_prefix)
            .is_some_and(|action| action == requested_action)
    });
    let Some(canonical) = matches.next() else {
        return requested_backend_tool.to_string();
    };
    if matches.next().is_some() {
        return requested_backend_tool.to_string();
    }
    canonical.to_string()
}

pub async fn call_local(
    registry_dir: PathBuf,
    tool_slug: String,
    dcc_type: Option<String>,
    instance_id: Option<String>,
    arguments: Value,
    meta: Option<Value>,
    timeout: Duration,
) -> anyhow::Result<Value> {
    let route = resolve_tool_route(
        &registry_dir,
        &tool_slug,
        dcc_type.as_deref(),
        instance_id.as_deref(),
    )?;
    enforce_active_instance_lease(&route.entry, meta.as_ref())?;
    let gateway = HttpGateway::with_timeout(timeout);
    let dispatch_mcp_url = local_dispatch_mcp_url(&route.entry, &route.backend_tool);
    let mut call = mcp_call_tool_correlated(
        &gateway,
        &dispatch_mcp_url,
        &route.backend_tool,
        arguments.clone(),
        meta.clone(),
    )
    .await
    .with_context(|| format!("calling local tool {}", route.tool_slug))?;
    let mut result = call.result;

    if should_retry_local_call_via_discovery(&result) {
        let discovery_mcp_url = local_instance::discovery_mcp_url(&route.entry);
        if discovery_mcp_url != dispatch_mcp_url {
            call = mcp_call_tool_correlated(
                &gateway,
                &discovery_mcp_url,
                &route.backend_tool,
                arguments.clone(),
                meta,
            )
            .await
            .with_context(|| {
                format!(
                    "calling local discovery tool {} after sidecar returned unknown-action",
                    route.tool_slug
                )
            })?;
            result = call.result;
        }
    }

    Ok(json!({
        "success": !result.get("isError").and_then(Value::as_bool).unwrap_or(false),
        "tool_slug": route.tool_slug,
        "backend_tool": route.backend_tool,
        "dcc_type": route.entry.dcc_type,
        "instance_id": route.entry.instance_id.to_string(),
        "instance_short": local_instance::instance_short(&route.entry),
        "request_id": call.request_id,
        "arguments": arguments,
        "result": result,
        "source": "local_mcp",
    }))
}

fn should_retry_local_call_via_discovery(result: &Value) -> bool {
    (result.get("success").and_then(Value::as_bool) == Some(false)
        && result.get("error").and_then(Value::as_str) == Some("unknown-action"))
        || (result.get("isError").and_then(Value::as_bool) == Some(true)
            && result
                .pointer("/structuredContent/error")
                .and_then(Value::as_str)
                == Some("unknown-action"))
}

fn local_dispatch_mcp_url(entry: &ServiceEntry, backend_tool: &str) -> String {
    if backend_tool.starts_with("ui_control__") {
        return local_instance::discovery_mcp_url(entry);
    }
    local_instance::mcp_url(entry)
}

fn enforce_active_instance_lease(entry: &ServiceEntry, meta: Option<&Value>) -> anyhow::Result<()> {
    let request_owner = meta
        .and_then(|value| value.get("lease_owner"))
        .and_then(Value::as_str);
    entry
        .check_lease_owner(request_owner, SystemTime::now())
        .map_err(|error| {
            anyhow::anyhow!(
                "{}: {error} for instance {}",
                error.kind(),
                entry.instance_id
            )
        })
}

pub async fn wait_ready_local(
    registry_dir: PathBuf,
    request: WaitReadyRequest,
) -> anyhow::Result<Value> {
    let required = normalize_required_fields(request.required);
    let entry = local_instance::select_one_entry(
        &registry_dir,
        request.dcc_type.as_deref(),
        request.instance_id.as_deref(),
    )?;
    let gateway = HttpGateway::with_timeout(request.interval.max(Duration::from_secs(1)));
    let readyz_url = local_instance::readyz_url(&entry);
    let started = tokio::time::Instant::now();
    let mut attempts = 0_u64;
    let mut last = json!({
        "ready": false,
        "required": required,
        "instance": local_instance::instance_summary(&entry),
        "readiness": null,
        "missing": DEFAULT_REQUIRED_READINESS_FIELDS,
        "source": "local_mcp",
    });

    loop {
        attempts += 1;
        match gateway.get_json(&readyz_url).await {
            Ok(value) => {
                let mut readiness = normalize_readiness_report(&value).unwrap_or(value);
                if required.iter().any(|field| field == "skill_catalog")
                    && readiness.get("skill_catalog").and_then(Value::as_bool) != Some(true)
                {
                    let catalog_observable = discovery_catalog_observable(&gateway, &entry).await;
                    readiness = reconcile_catalog_readiness(readiness, catalog_observable);
                }
                let missing = missing_required_fields(Some(&readiness), &required);
                let ready = missing.is_empty();
                last = json!({
                    "ready": ready,
                    "required": required,
                    "attempts": attempts,
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "instance": local_instance::instance_summary(&entry),
                    "readiness": readiness,
                    "readiness_source": "direct",
                    "missing": missing,
                    "source": "local_mcp",
                });
                if ready {
                    return Ok(last);
                }
            }
            Err(err) => {
                last = json!({
                    "ready": false,
                    "required": required,
                    "attempts": attempts,
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "instance": local_instance::instance_summary(&entry),
                    "readiness": null,
                    "missing": required,
                    "error": err.to_string(),
                    "source": "local_mcp",
                });
            }
        }

        if started.elapsed() >= request.timeout {
            return Ok(last);
        }
        tokio::time::sleep(request.interval.max(Duration::from_secs(1))).await;
    }
}

pub async fn reload_skills_local(
    registry_dir: PathBuf,
    request: ReloadSkillsRequest,
) -> anyhow::Result<Value> {
    let entries = local_instance::select_routable_entries(
        &registry_dir,
        request.dcc_type.as_deref(),
        request.instance_id.as_deref(),
    )?;
    if entries.is_empty() {
        anyhow::bail!("no live local DCC instance matched the request");
    }

    let gateway = HttpGateway::default();
    let mut results = Vec::new();
    for entry in entries {
        let result = mcp_call_tool(
            &gateway,
            &local_instance::mcp_url(&entry),
            "dcc_admin__reload_skills",
            json!({}),
            None,
        )
        .await
        .with_context(|| {
            format!(
                "reloading skills on local {} instance {}",
                entry.dcc_type,
                local_instance::instance_short(&entry)
            )
        })?;
        let mut payload = call_result_payload(&result).unwrap_or(result);
        attach_local_context(
            &mut payload,
            &entry,
            Some("dcc_admin__reload_skills"),
            "local_mcp",
        );
        results.push(payload);
    }

    let reloaded = results.iter().all(reload_result_succeeded);

    Ok(json!({
        "ok": reloaded,
        "reloaded": reloaded,
        "count": results.len(),
        "results": results,
        "source": "local_mcp",
        "registry_dir": registry_dir,
    }))
}

pub(crate) fn reload_result_succeeded(value: &Value) -> bool {
    call_result_succeeded(value)
        && value
            .get("reloaded")
            .and_then(Value::as_bool)
            .unwrap_or(true)
}

pub(crate) fn call_result_succeeded(value: &Value) -> bool {
    call_result_succeeded_inner(value, 0)
}

fn call_result_succeeded_inner(value: &Value, depth: u8) -> bool {
    if depth > 4 {
        return true;
    }
    if value.get("success").and_then(Value::as_bool) == Some(false)
        || value.get("ok").and_then(Value::as_bool) == Some(false)
        || value.get("isError").and_then(Value::as_bool) == Some(true)
        || value.get("error").is_some_and(|error| !error.is_null())
    {
        return false;
    }
    [
        "result",
        "output",
        "structuredContent",
        "structured_content",
        "results",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .all(|nested| call_result_succeeded_inner(nested, depth + 1))
        && match value {
            Value::Array(items) => items
                .iter()
                .all(|item| call_result_succeeded_inner(item, depth + 1)),
            Value::Object(_) => value
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                .all(|payload| call_result_succeeded_inner(&payload, depth + 1)),
            _ => true,
        }
}

pub async fn stop_instance_local(
    registry_dir: PathBuf,
    request: StopInstanceRequest,
) -> anyhow::Result<Value> {
    let entry = local_instance::select_one_entry(
        &registry_dir,
        Some(&request.dcc_type),
        Some(&request.instance_id),
    )?;
    guard_metadata(
        &entry,
        "owner",
        request.expected_owner.as_deref(),
        &[
            "owner",
            "test_owner",
            "dcc_mcp_owner",
            "dcc_mcp_test_owner",
            "dcc_mcp.owner",
        ],
    )?;
    guard_metadata(
        &entry,
        "session",
        request.expected_session.as_deref(),
        &[
            "session",
            "test_session",
            "dcc_mcp_session",
            "dcc_mcp_test_session",
            "dcc_mcp.session",
        ],
    )?;

    let Some(stop_url) = metadata_value(
        &entry,
        &[
            "safe_stop_url",
            "dcc_mcp_safe_stop_url",
            "dcc_mcp.safe_stop_url",
            "stop_url",
        ],
    ) else {
        anyhow::bail!("instance does not advertise safe_stop_url metadata; refusing to stop it");
    };
    let method = metadata_value(
        &entry,
        &[
            "safe_stop_method",
            "dcc_mcp_safe_stop_method",
            "dcc_mcp.safe_stop_method",
        ],
    )
    .unwrap_or("POST");
    if !method.eq_ignore_ascii_case("POST") {
        anyhow::bail!("unsupported safe_stop_method '{method}'; only POST is supported");
    }

    let gateway = HttpGateway::default();
    let response = gateway
        .post_json(
            stop_url,
            &json!({
                "instance_id": entry.instance_id.to_string(),
                "dcc_type": entry.dcc_type,
                "owner": metadata_value(&entry, &["owner", "test_owner", "dcc_mcp_owner", "dcc_mcp_test_owner", "dcc_mcp.owner"]),
                "session": metadata_value(&entry, &["session", "test_session", "dcc_mcp_session", "dcc_mcp_test_session", "dcc_mcp.session"]),
            }),
        )
        .await
        .with_context(|| format!("posting safe-stop request to {stop_url}"))?;

    Ok(json!({
        "ok": true,
        "stopping": true,
        "instance_id": entry.instance_id.to_string(),
        "dcc_type": entry.dcc_type,
        "safe_stop_url": stop_url,
        "response": response,
        "source": "local_mcp",
    }))
}

#[derive(Debug)]
struct ToolRoute {
    entry: ServiceEntry,
    backend_tool: String,
    tool_slug: String,
}

impl ToolRoute {
    fn record(&self) -> Value {
        json!({
            "tool_slug": self.tool_slug,
            "backend_tool": self.backend_tool,
            "dcc": self.entry.dcc_type,
            "dcc_type": self.entry.dcc_type,
            "instance_id": self.entry.instance_id.to_string(),
            "instance_short": local_instance::instance_short(&self.entry),
            "mcp_url": local_instance::mcp_url(&self.entry),
            "source": "local_mcp",
        })
    }
}

fn resolve_tool_route(
    registry_dir: &Path,
    tool_slug: &str,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
) -> anyhow::Result<ToolRoute> {
    let parsed = parse_local_tool_slug(tool_slug);
    let dcc = dcc_type.or(parsed.dcc_type.as_deref());
    let instance = instance_id.or(parsed.instance_hint.as_deref());
    let entry = local_instance::select_one_routable_entry(registry_dir, dcc, instance)?;
    let backend_tool = parsed.backend_tool;
    Ok(ToolRoute {
        tool_slug: local_instance::local_tool_slug(&entry, &backend_tool),
        entry,
        backend_tool,
    })
}

struct ParsedToolSlug {
    dcc_type: Option<String>,
    instance_hint: Option<String>,
    backend_tool: String,
}

fn parse_local_tool_slug(tool_slug: &str) -> ParsedToolSlug {
    let mut parts = tool_slug.splitn(3, '.');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();
    match (second, third) {
        (Some(instance), Some(tool)) => ParsedToolSlug {
            dcc_type: Some(first.to_string()),
            instance_hint: Some(instance.to_string()),
            backend_tool: tool.to_string(),
        },
        _ => ParsedToolSlug {
            dcc_type: None,
            instance_hint: None,
            backend_tool: tool_slug.to_string(),
        },
    }
}

async fn list_mcp_tools(gateway: &HttpGateway, mcp_url: &str) -> anyhow::Result<Vec<Value>> {
    let mut cursor: Option<String> = None;
    let mut tools = Vec::new();
    for _ in 0..16 {
        let mut params = Map::new();
        if let Some(value) = cursor.take() {
            params.insert("cursor".to_string(), Value::String(value));
        }
        let response = mcp_request(gateway, mcp_url, "tools/list", Value::Object(params)).await?;
        let result = response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("MCP tools/list response did not contain result"))?;
        tools.extend(
            result
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .cloned(),
        );
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(tools)
}

async fn mcp_call_tool(
    gateway: &HttpGateway,
    mcp_url: &str,
    name: &str,
    arguments: Value,
    meta: Option<Value>,
) -> anyhow::Result<Value> {
    Ok(
        mcp_call_tool_correlated(gateway, mcp_url, name, arguments, meta)
            .await?
            .result,
    )
}

struct CorrelatedMcpToolCall {
    request_id: String,
    result: Value,
}

async fn mcp_call_tool_correlated(
    gateway: &HttpGateway,
    mcp_url: &str,
    name: &str,
    arguments: Value,
    meta: Option<Value>,
) -> anyhow::Result<CorrelatedMcpToolCall> {
    let mut params = Map::new();
    params.insert("name".to_string(), Value::String(name.to_string()));
    params.insert("arguments".to_string(), arguments);
    if let Some(meta) = meta {
        params.insert("_meta".to_string(), meta);
    }
    let response = mcp_request(gateway, mcp_url, "tools/call", Value::Object(params)).await?;
    let result = response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("MCP tools/call response did not contain result"))?;
    let request_id = response
        .get("id")
        .and_then(Value::as_str)
        .expect("mcp_request validates the generated string id")
        .to_string();
    Ok(CorrelatedMcpToolCall { request_id, result })
}

async fn mcp_request(
    gateway: &HttpGateway,
    mcp_url: &str,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    let request_id = next_local_mcp_request_id();
    let body = JsonRpcRequestBuilder::new(request_id.clone(), method)
        .with_params(params)
        .to_value();
    let response = gateway
        .post_json_with_headers(
            mcp_url,
            &body,
            &[
                ("Mcp-Protocol-Version", MCP_PROTOCOL_VERSION),
                ("Accept", MCP_ACCEPT),
            ],
        )
        .await
        .with_context(|| format!("MCP {method} request failed (request_id={request_id})"))?;
    let expected_id = Value::String(request_id.clone());
    if response.get("id") != Some(&expected_id) {
        anyhow::bail!(
            "transport desync: expected JSON-RPC response id {request_id:?}, got {}",
            response
                .get("id")
                .map_or_else(|| "<missing>".to_string(), Value::to_string),
        );
    }
    if let Some(error) = response.get("error") {
        anyhow::bail!("MCP {method} failed (request_id={request_id}): {error}");
    }
    Ok(response)
}

fn next_local_mcp_request_id() -> String {
    format!("dcc-mcp-cli-local-{}", uuid::Uuid::new_v4())
}

pub(crate) fn call_result_payload(result: &Value) -> Option<Value> {
    if let Some(value) = result.get("structuredContent") {
        return Some(value.clone());
    }
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
}

fn extend_tool_hits(hits: &mut Vec<Value>, entry: &ServiceEntry, payload: &Value) {
    for tool in payload
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let tool_slug = local_instance::local_tool_slug(entry, name);
        let has_schema = tool
            .get("has_schema")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let enabled = tool.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let group = tool
            .get("group")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|group| !group.is_empty());
        let skill_name = tool
            .get("skill_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|skill| !skill.is_empty());
        let annotations = tool.get("annotations").cloned().unwrap_or(Value::Null);
        let next_step = if !enabled {
            match (skill_name, group) {
                (Some(skill_name), Some(group)) => json!({
                    "action": "load_skill",
                    "arguments": {
                        "skill_name": skill_name,
                        "dcc": entry.dcc_type,
                        "dcc_type": entry.dcc_type,
                        "instance_id": entry.instance_id.to_string(),
                        "tool_group": group,
                        "group_action": "activate",
                        "target_tool_slug": tool_slug,
                    }
                }),
                _ => json!({"action": "describe", "arguments": {"tool_slug": tool_slug}}),
            }
        } else if has_schema || !has_safety_hints(&annotations) {
            json!({"action": "describe", "arguments": {"tool_slug": tool_slug}})
        } else {
            json!({
                "action": "call",
                "arguments": {"tool_slug": tool_slug, "arguments": {}}
            })
        };
        hits.push(json!({
            "kind": "tool",
            "slug": tool_slug,
            "backend_tool": name,
            "instance_id": entry.instance_id.to_string(),
            "instance_short": local_instance::instance_short(entry),
            "dcc": entry.dcc_type,
            "dcc_type": entry.dcc_type,
            "summary": tool.get("description").cloned().unwrap_or(Value::Null),
            "has_schema": has_schema,
            "enabled": enabled,
            "skill_name": skill_name,
            "tool_group": group,
            "annotations": annotations,
            "metadata": tool.get("metadata").cloned().unwrap_or(Value::Null),
            "loaded": true,
            "next_step": next_step,
            "scope": "local",
            "source": "local_mcp",
            "mcp_url": local_instance::mcp_url(entry),
        }));
    }
}

fn has_safety_hints(annotations: &Value) -> bool {
    annotations.as_object().is_some_and(|annotations| {
        [
            ("readOnlyHint", "read_only_hint"),
            ("destructiveHint", "destructive_hint"),
            ("idempotentHint", "idempotent_hint"),
            ("openWorldHint", "open_world_hint"),
        ]
        .iter()
        .all(|(camel, snake)| {
            annotations
                .get(*camel)
                .or_else(|| annotations.get(*snake))
                .and_then(Value::as_bool)
                .is_some()
        })
    })
}

fn simple_object_schema(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) != Some("object")
        || object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "type"
                    | "properties"
                    | "required"
                    | "title"
                    | "description"
                    | "$schema"
                    | "additionalProperties"
            )
        })
    {
        return false;
    }
    !schema_contains_complex_keyword(schema)
}

fn schema_has_constraints(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return !schema.is_null();
    };
    object.iter().any(|(key, value)| match key.as_str() {
        "type" => value.as_str() != Some("object"),
        "properties" => value
            .as_object()
            .is_none_or(|properties| !properties.is_empty()),
        "required" => value.as_array().is_none_or(|required| !required.is_empty()),
        "title" | "description" | "$schema" => false,
        _ => true,
    })
}

fn schema_contains_complex_keyword(schema: &Value) -> bool {
    const COMPLEX: &[&str] = &[
        "$ref",
        "$dynamicRef",
        "$defs",
        "definitions",
        "oneOf",
        "anyOf",
        "allOf",
        "not",
        "if",
        "then",
        "else",
        "unevaluatedProperties",
        "patternProperties",
        "propertyNames",
        "dependentRequired",
        "dependentSchemas",
        "dependencies",
    ];
    match schema {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "additionalProperties" && value != &Value::Bool(false))
                || COMPLEX.contains(&key.as_str())
                || schema_contains_complex_keyword(value)
        }),
        Value::Array(items) => items.iter().any(schema_contains_complex_keyword),
        _ => false,
    }
}

fn extend_skill_hits(hits: &mut Vec<Value>, entry: &ServiceEntry, payload: &Value) {
    for skill in payload
        .get("skill_candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let skill_name = skill
            .get("skill_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let target_tool = skill
            .get("matching_tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.iter().filter_map(Value::as_str).next());
        let target_tool_slug = target_tool.map(|tool| local_instance::local_tool_slug(entry, tool));
        let mut load_arguments = json!({
            "skill_name": skill_name,
            "dcc": entry.dcc_type,
            "dcc_type": entry.dcc_type,
            "instance_id": entry.instance_id.to_string(),
        });
        if let Some(target_tool_slug) = &target_tool_slug {
            load_arguments["target_tool_slug"] = json!(target_tool_slug);
        }
        if let Some(tool_group) = skill.get("tool_group").and_then(Value::as_str) {
            load_arguments["tool_group"] = json!(tool_group);
        }
        hits.push(json!({
            "kind": "skill_candidate",
            "skill_name": skill_name,
            "slug": target_tool_slug,
            "target_tool_slug": target_tool_slug,
            "matching_tools": skill.get("matching_tools").cloned().unwrap_or_else(|| json!([])),
            "requires_load_skill": true,
            "load_hint": {
                "tool": "load_skill",
                "arguments": load_arguments,
            },
            "next_step": {
                "action": "load_skill",
                "arguments": load_arguments,
            },
            "instance_id": entry.instance_id.to_string(),
            "instance_short": local_instance::instance_short(entry),
            "dcc": entry.dcc_type,
            "dcc_type": entry.dcc_type,
            "summary": skill.get("description").cloned().unwrap_or(Value::Null),
            "loaded": false,
            "scope": "local",
            "source": "local_mcp",
            "mcp_url": local_instance::mcp_url(entry),
        }));
    }
}

fn attach_local_context(
    payload: &mut Value,
    entry: &ServiceEntry,
    backend_tool: Option<&str>,
    source: &str,
) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("source".to_string(), Value::String(source.to_string()));
        obj.insert(
            "dcc_type".to_string(),
            Value::String(entry.dcc_type.clone()),
        );
        obj.insert("dcc".to_string(), Value::String(entry.dcc_type.clone()));
        obj.insert(
            "instance_id".to_string(),
            Value::String(entry.instance_id.to_string()),
        );
        obj.insert(
            "instance_short".to_string(),
            Value::String(local_instance::instance_short(entry)),
        );
        obj.insert(
            "mcp_url".to_string(),
            Value::String(local_instance::mcp_url(entry)),
        );
        if let Some(tool) = backend_tool {
            obj.insert("backend_tool".to_string(), Value::String(tool.to_string()));
            obj.insert(
                "tool_slug".to_string(),
                Value::String(local_instance::local_tool_slug(entry, tool)),
            );
        }
    }
}

fn normalize_required_fields(fields: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = fields
        .into_iter()
        .map(|field| field.trim().to_ascii_lowercase().replace('-', "_"))
        .filter(|field| !field.is_empty())
        .collect();
    if normalized.is_empty() {
        normalized = DEFAULT_REQUIRED_READINESS_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect();
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_readiness_report(value: &Value) -> Option<Value> {
    if let Some(readiness) = value.get("readiness")
        && readiness.is_object()
    {
        return Some(readiness.clone());
    }
    if value.is_object() {
        return Some(value.clone());
    }
    None
}

async fn discovery_catalog_observable(gateway: &HttpGateway, entry: &ServiceEntry) -> bool {
    list_mcp_tools(gateway, &local_instance::discovery_mcp_url(entry))
        .await
        .is_ok_and(|tools| {
            tools.iter().any(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        matches!(name, "search_tools" | "search_skills" | "load_skill")
                    })
            })
        })
}

fn reconcile_catalog_readiness(mut readiness: Value, catalog_observable: bool) -> Value {
    if !catalog_observable || readiness.get("skill_catalog").and_then(Value::as_bool) == Some(true)
    {
        return readiness;
    }
    if let Some(object) = readiness.as_object_mut() {
        object.insert("skill_catalog".to_string(), Value::Bool(true));
        object.insert(
            "skill_catalog_source".to_string(),
            Value::String("discovery_mcp".to_string()),
        );
    }
    readiness
}

fn missing_required_fields(readiness: Option<&Value>, required: &[String]) -> Vec<String> {
    required
        .iter()
        .filter(|field| {
            readiness
                .and_then(|report| report.get(field.as_str()))
                .and_then(Value::as_bool)
                != Some(true)
        })
        .cloned()
        .collect()
}

fn guard_metadata(
    entry: &ServiceEntry,
    label: &str,
    expected: Option<&str>,
    keys: &[&str],
) -> anyhow::Result<()> {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let actual = metadata_value(entry, keys);
    if Some(expected) != actual {
        anyhow::bail!("expected {label}='{expected}' but instance metadata has {actual:?}");
    }
    Ok(())
}

fn metadata_value<'a>(entry: &'a ServiceEntry, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| entry.metadata.get(*key).map(String::as_str))
        .find(|value| !value.trim().is_empty())
}

#[cfg(test)]
#[path = "local_control_tests.rs"]
mod tests;
