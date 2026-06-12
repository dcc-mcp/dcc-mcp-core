//! Local FileRegistry + direct MCP control path for `dcc-mcp-cli`.
//!
//! Remote profiles use the gateway REST surface. The built-in `local` profile
//! resolves live instances from the shared FileRegistry, then talks to the
//! selected DCC instance's MCP HTTP endpoint directly.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::{Map, Value, json};

use crate::application::local_instance;
use crate::domain::rest::{
    ReloadSkillsRequest, SearchRequest, StopInstanceRequest, WaitReadyRequest,
};
use crate::infra::http::HttpGateway;

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
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
    let limit = request.limit.unwrap_or(25).clamp(1, 100);

    for entry in &entries {
        let search_result = mcp_call_tool(
            &gateway,
            &local_instance::mcp_url(entry),
            "search_tools",
            json!({
                "query": request.query.clone().unwrap_or_default(),
                "dcc": entry.dcc_type,
                "limit": limit,
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
        let payload = call_result_payload(&search_result).unwrap_or(search_result);
        extend_tool_hits(&mut hits, entry, &payload);
        extend_skill_hits(&mut hits, entry, &payload);
        if hits.len() >= limit {
            hits.truncate(limit);
            break;
        }
    }

    Ok(json!({
        "total": hits.len(),
        "hits": hits,
        "source": "local_mcp",
        "registry_dir": registry_dir,
        "query": request.query,
    }))
}

pub async fn describe_local(registry_dir: PathBuf, tool_slug: String) -> anyhow::Result<Value> {
    let route = resolve_tool_route(&registry_dir, &tool_slug, None, None)?;
    let gateway = HttpGateway::default();
    let tools = list_mcp_tools(&gateway, &local_instance::mcp_url(&route.entry)).await?;
    let Some(tool) = tools.into_iter().find(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == route.backend_tool)
    }) else {
        anyhow::bail!(
            "tool '{}' was not found on local {} instance {}",
            route.backend_tool,
            route.entry.dcc_type,
            local_instance::instance_short(&route.entry)
        );
    };

    Ok(json!({
        "record": route.record(),
        "tool": tool,
        "instance": local_instance::instance_summary(&route.entry),
        "source": "local_mcp",
    }))
}

pub async fn load_skill_local(registry_dir: PathBuf, body: Value) -> anyhow::Result<Value> {
    let (dcc_type, instance_id, backend_body) = split_load_skill_request(body)?;
    let entry = local_instance::select_one_routable_entry(
        &registry_dir,
        dcc_type.as_deref(),
        instance_id.as_deref(),
    )?;
    let gateway = HttpGateway::default();
    let result = mcp_call_tool(
        &gateway,
        &local_instance::mcp_url(&entry),
        "load_skill",
        backend_body,
        None,
    )
    .await
    .with_context(|| {
        format!(
            "loading skill on local {} instance {}",
            entry.dcc_type,
            local_instance::instance_short(&entry)
        )
    })?;
    let mut payload = call_result_payload(&result).unwrap_or(result);
    attach_local_context(&mut payload, &entry, None, "local_mcp");
    Ok(payload)
}

fn split_load_skill_request(
    body: Value,
) -> anyhow::Result<(Option<String>, Option<String>, Value)> {
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

    object.remove("dcc_type");
    object.remove("dcc");
    object.remove("instance_id");

    Ok((dcc_type, instance_id, Value::Object(object)))
}

pub async fn call_local(
    registry_dir: PathBuf,
    tool_slug: String,
    dcc_type: Option<String>,
    instance_id: Option<String>,
    arguments: Value,
    meta: Option<Value>,
) -> anyhow::Result<Value> {
    let route = resolve_tool_route(
        &registry_dir,
        &tool_slug,
        dcc_type.as_deref(),
        instance_id.as_deref(),
    )?;
    let gateway = HttpGateway::default();
    let result = mcp_call_tool(
        &gateway,
        &local_instance::mcp_url(&route.entry),
        &route.backend_tool,
        arguments.clone(),
        meta,
    )
    .await
    .with_context(|| format!("calling local tool {}", route.tool_slug))?;

    Ok(json!({
        "success": !result.get("isError").and_then(Value::as_bool).unwrap_or(false),
        "tool_slug": route.tool_slug,
        "backend_tool": route.backend_tool,
        "dcc_type": route.entry.dcc_type,
        "instance_id": route.entry.instance_id.to_string(),
        "instance_short": local_instance::instance_short(&route.entry),
        "arguments": arguments,
        "result": result,
        "source": "local_mcp",
    }))
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
                let readiness = normalize_readiness_report(&value).unwrap_or(value);
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

    Ok(json!({
        "ok": true,
        "reloaded": true,
        "count": results.len(),
        "results": results,
        "source": "local_mcp",
        "registry_dir": registry_dir,
    }))
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
    Ok(result)
}

async fn mcp_request(
    gateway: &HttpGateway,
    mcp_url: &str,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": format!("dcc-mcp-cli-local-{method}"),
        "method": method,
        "params": params,
    });
    let response = gateway
        .post_json_with_headers(
            mcp_url,
            &body,
            &[
                ("Mcp-Protocol-Version", MCP_PROTOCOL_VERSION),
                ("Accept", MCP_ACCEPT),
            ],
        )
        .await?;
    if let Some(error) = response.get("error") {
        anyhow::bail!("MCP {method} failed: {error}");
    }
    Ok(response)
}

fn call_result_payload(result: &Value) -> Option<Value> {
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
        hits.push(json!({
            "kind": "tool",
            "slug": local_instance::local_tool_slug(entry, name),
            "backend_tool": name,
            "instance_id": entry.instance_id.to_string(),
            "instance_short": local_instance::instance_short(entry),
            "dcc": entry.dcc_type,
            "dcc_type": entry.dcc_type,
            "summary": tool.get("description").cloned().unwrap_or(Value::Null),
            "loaded": true,
            "scope": "local",
            "source": "local_mcp",
            "mcp_url": local_instance::mcp_url(entry),
        }));
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
        hits.push(json!({
            "kind": "skill_candidate",
            "skill_name": skill_name,
            "slug": target_tool.map(|tool| local_instance::local_tool_slug(entry, tool)),
            "target_tool_slug": target_tool.map(|tool| local_instance::local_tool_slug(entry, tool)),
            "matching_tools": skill.get("matching_tools").cloned().unwrap_or_else(|| json!([])),
            "requires_load_skill": true,
            "load_hint": {
                "tool": "load_skill",
                "arguments": { "skill_name": skill_name },
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
mod tests {
    use super::*;

    #[test]
    fn local_tool_slug_round_trips() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
        let slug = local_instance::local_tool_slug(&entry, "maya_scene__get_session_info");
        let parsed = parse_local_tool_slug(&slug);

        assert_eq!(parsed.dcc_type.as_deref(), Some("maya"));
        assert_eq!(
            parsed.instance_hint.as_deref(),
            Some(local_instance::instance_short(&entry).as_str())
        );
        assert_eq!(parsed.backend_tool, "maya_scene__get_session_info");
    }

    #[test]
    fn load_skill_request_strips_local_routing_fields() {
        let (dcc_type, instance_id, backend_body) = split_load_skill_request(json!({
            "skill_name": "workflow",
            "dcc_type": "maya",
            "dcc": "maya-legacy",
            "instance_id": "abc12345",
            "activate_groups": false
        }))
        .unwrap();

        assert_eq!(dcc_type.as_deref(), Some("maya"));
        assert_eq!(instance_id.as_deref(), Some("abc12345"));
        assert_eq!(
            backend_body,
            json!({
                "skill_name": "workflow",
                "activate_groups": false
            })
        );
    }
}
