use serde_json::{Value, json};

#[cfg(feature = "admin")]
pub(crate) fn add_gateway_debug_openapi_paths(doc: &mut Value) {
    if let Some(tags) = doc.get_mut("tags").and_then(Value::as_array_mut)
        && !tags
            .iter()
            .any(|tag| tag.get("name").and_then(Value::as_str) == Some("debug"))
    {
        tags.push(json!({
            "name": "debug",
            "description": "Gateway agent/debug diagnostics promoted from the Admin providers (#1092).",
        }));
    }

    let Some(paths) = doc.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };

    let limit_params = vec![query_param(
        "limit",
        json!({"type": "integer", "minimum": 1, "maximum": 1000}),
        "Maximum number of rows to return.",
    )];
    let time_filter_params = vec![
        query_param(
            "cursor",
            json!({"type": "string"}),
            "Opaque pagination cursor. Reserved for stable clients; current implementation may return null.",
        ),
        query_param(
            "since",
            json!({"type": "string", "format": "date-time"}),
            "Lower timestamp bound in RFC3339 UTC. Reserved for stable clients.",
        ),
        query_param(
            "until",
            json!({"type": "string", "format": "date-time"}),
            "Upper timestamp bound in RFC3339 UTC. Reserved for stable clients.",
        ),
    ];
    let mut list_params = limit_params.clone();
    list_params.extend(time_filter_params.clone());

    for (path, summary, description, params) in [
        (
            "/v1/debug/instances",
            "List gateway debug instances",
            "Stable agent-facing alias for the Admin instance summary.",
            vec![
                query_param(
                    "view",
                    json!({"type": "string", "enum": ["live", "all", "registry"]}),
                    "Instance view to return.",
                ),
                query_param(
                    "include_stale",
                    json!({"type": "boolean"}),
                    "Include stale diagnostic rows.",
                ),
                query_param(
                    "include_dead",
                    json!({"type": "boolean"}),
                    "Include rows whose owner process is gone.",
                ),
            ],
        ),
        (
            "/v1/debug/activity",
            "List gateway debug activity",
            "Stable agent-facing activity feed built from audits, traces, and gateway events.",
            list_params.clone(),
        ),
        (
            "/v1/debug/calls",
            "List recent debug calls",
            "Stable agent-facing recent call list backed by audit rows.",
            list_params.clone(),
        ),
        (
            "/v1/debug/traces",
            "List gateway debug traces",
            "Stable agent-facing recent dispatch trace list.",
            list_params.clone(),
        ),
        (
            "/v1/debug/traces/{request_id}",
            "Get one debug trace by request id",
            "Stable agent-facing dispatch trace detail lookup.",
            vec![path_param("request_id", "Gateway request id.")],
        ),
        (
            "/v1/debug/trace-context/{lookup_id}",
            "Resolve a trace context",
            "Lookup by trace id or request id and return the primary trace plus related request ids.",
            vec![path_param("lookup_id", "Trace id or request id.")],
        ),
        (
            "/v1/debug/tasks",
            "List task-like debug snapshots",
            "Stable task projection reconstructed from dispatch traces.",
            list_params.clone(),
        ),
        (
            "/v1/debug/bundles/{request_id}",
            "Get a debug bundle",
            "Full-chain debug bundle by request id or trace id.",
            vec![path_param("request_id", "Request id or trace id.")],
        ),
        (
            "/v1/debug/issue-reports/{request_id}",
            "Get issue-report debug JSON",
            "GitHub-attachable debug report for one request.",
            vec![path_param("request_id", "Gateway request id.")],
        ),
        (
            "/v1/debug/logs",
            "List merged debug logs",
            "Merged gateway contention events, file logs, and audited call summaries.",
            list_params.clone(),
        ),
        (
            "/v1/debug/deregistered",
            "List auto-deregistered instances",
            "Recently auto-deregistered registry rows retained for forensic debugging.",
            limit_params.clone(),
        ),
        (
            "/v1/debug/stats",
            "Get debug statistics",
            "Aggregated gateway call statistics.",
            vec![query_param(
                "range",
                json!({"type": "string", "enum": ["1h", "24h", "7d", "all"]}),
                "Aggregation range.",
            )],
        ),
        (
            "/v1/debug/search-telemetry",
            "Get search-quality telemetry",
            "Recent prompt-safe search records and aggregate search-to-describe/load/call hit-rate metrics.",
            limit_params.clone(),
        ),
        (
            "/v1/debug/health",
            "Get debug subsystem health",
            "Compact health and readiness summary for debug providers.",
            Vec::new(),
        ),
    ] {
        paths.insert(
            path.to_string(),
            debug_get_path(summary, description, params),
        );
    }

    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        schemas.insert(
            "GatewayDebugPayload".to_string(),
            json!({
                "type": "object",
                "description": "Gateway debug JSON payload. Phase-1 endpoints preserve existing Admin provider payload fields while publishing them under stable /v1/debug routes.",
                "additionalProperties": true,
            }),
        );
        schemas.insert(
            "GatewayDebugError".to_string(),
            json!({
                "type": "object",
                "required": ["error"],
                "properties": {
                    "error": {"type": "string"},
                    "request_id": {"type": "string"},
                    "lookup_id": {"type": "string"}
                },
                "additionalProperties": true,
            }),
        );
    }
}

#[cfg(feature = "admin")]
fn debug_get_path(summary: &str, description: &str, parameters: Vec<Value>) -> Value {
    json!({
        "get": {
            "tags": ["debug"],
            "summary": summary,
            "description": description,
            "parameters": parameters,
            "responses": {
                "200": {
                    "description": "Debug payload",
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/GatewayDebugPayload"}
                        }
                    }
                },
                "404": {
                    "description": "Debug record not found",
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/GatewayDebugError"}
                        }
                    }
                }
            }
        }
    })
}

#[cfg(feature = "admin")]
fn path_param(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "description": description,
        "schema": {"type": "string"}
    })
}

#[cfg(feature = "admin")]
fn query_param(name: &str, schema: Value, description: &str) -> Value {
    json!({
        "name": name,
        "in": "query",
        "required": false,
        "description": description,
        "schema": schema
    })
}
