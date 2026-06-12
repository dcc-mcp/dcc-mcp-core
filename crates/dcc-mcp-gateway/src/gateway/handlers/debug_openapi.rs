use serde_json::{Map, Value, json};

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
    let compact_params = vec![
        header_param(
            "Accept",
            json!({"type": "string", "enum": ["application/json", "application/toon"]}),
            "Set application/toon to receive compact TOON output on compact-aware debug routes.",
        ),
        query_param(
            "response_format",
            json!({"type": "string", "enum": ["json", "toon"]}),
            "Optional response-format override for clients that cannot set Accept.",
        ),
        query_param(
            "compact",
            json!({"type": "boolean"}),
            "Alias for response_format=toon when true.",
        ),
    ];
    let mut compact_list_params = list_params.clone();
    compact_list_params.extend(compact_params.clone());
    let mut compact_stats_params = compact_params.clone();
    compact_stats_params.push(query_param(
        "range",
        json!({"type": "string", "enum": ["1h", "24h", "7d", "all"]}),
        "Aggregation range.",
    ));
    let analytics_range_param = query_param(
        "range",
        json!({"type": "string", "enum": ["7d", "30d", "90d", "180d", "365d"]}),
        "Analytics aggregation range. Defaults to 30d.",
    );
    let analytics_range_params = vec![analytics_range_param.clone()];
    let analytics_timeseries_params = vec![
        analytics_range_param.clone(),
        query_param(
            "granularity",
            json!({"type": "string", "enum": ["day", "hour"]}),
            "Aggregation granularity. Defaults to day.",
        ),
    ];
    let analytics_export_params = vec![
        analytics_range_param.clone(),
        query_param(
            "format",
            json!({"type": "string", "enum": ["json", "csv"]}),
            "Export format. json returns newline-delimited JSON; csv returns a CSV file.",
        ),
    ];

    for (path, summary, description, params, compact_capable) in [
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
            false,
        ),
        (
            "/v1/debug/activity",
            "List gateway debug activity",
            "Stable agent-facing activity feed built from audits, traces, and gateway events.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/calls",
            "List recent debug calls",
            "Stable agent-facing recent call list backed by audit rows.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/traces",
            "List gateway debug traces",
            "Stable agent-facing recent dispatch trace list.",
            compact_list_params.clone(),
            true,
        ),
        (
            "/v1/debug/traffic",
            "List live traffic capture state and metadata frames",
            "Stable agent-facing capture_status plus retained metadata-only traffic.frame list from an explicit admin_live traffic sink.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/traces/{request_id}",
            "Get one debug trace by request id",
            "Stable agent-facing dispatch trace detail lookup.",
            compact_path_params(
                path_param("request_id", "Gateway request id."),
                &compact_params,
            ),
            true,
        ),
        (
            "/v1/debug/trace-context/{lookup_id}",
            "Resolve a trace context",
            "Lookup by trace id or request id and return the primary trace plus related request ids.",
            compact_path_params(
                path_param("lookup_id", "Trace id or request id."),
                &compact_params,
            ),
            true,
        ),
        (
            "/v1/debug/agent-traces/{lookup_id}",
            "Get an agent trace packet",
            "Compact public-safe trace packet for agents, resolved by trace id or request id.",
            vec![path_param("lookup_id", "Trace id or request id.")],
            false,
        ),
        (
            "/v1/debug/tasks",
            "List task outcomes",
            "Stable user-level task outcome projection grouped from retained dispatch traces and audits.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/workflows",
            "List agent workflows",
            "Stable agent-facing workflow/session projection from retained search telemetry, traces, and audits.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/bundles/{request_id}",
            "Get a debug bundle",
            "Full-chain debug bundle by request id or trace id. Set Accept: application/toon for a compact public-safe summary with links to the full JSON bundle.",
            compact_path_params(
                path_param("request_id", "Request id or trace id."),
                &compact_params,
            ),
            true,
        ),
        (
            "/v1/debug/issue-reports/{request_id}",
            "Get issue-report debug JSON",
            "Public-safe GitHub issue report by default. Use mode=raw only for reviewed local evidence.",
            vec![
                path_param("request_id", "Gateway request id."),
                query_param(
                    "mode",
                    json!({"type": "string", "enum": ["public-safe", "raw"]}),
                    "Report privacy mode. Defaults to public-safe; raw includes the full debug bundle.",
                ),
                query_param(
                    "include_raw",
                    json!({"type": "boolean"}),
                    "Compatibility flag for explicit raw bundle exports.",
                ),
            ],
            false,
        ),
        (
            "/v1/debug/logs",
            "List merged debug logs",
            "Merged gateway contention events, file logs, and audited call summaries.",
            list_params.clone(),
            false,
        ),
        (
            "/v1/debug/deregistered",
            "List auto-deregistered instances",
            "Recently auto-deregistered registry rows retained for forensic debugging.",
            limit_params.clone(),
            false,
        ),
        (
            "/v1/debug/stats",
            "Get debug statistics",
            "Aggregated gateway call statistics.",
            compact_stats_params,
            true,
        ),
        (
            "/v1/debug/analytics/overview",
            "Get analytics overview",
            "Stable analytics KPI summary for gateway dashboard and agent diagnostics.",
            analytics_range_params.clone(),
            false,
        ),
        (
            "/v1/debug/analytics/timeseries",
            "Get analytics time series",
            "Stable analytics time series for calls, tokens, average duration, and maximum duration.",
            analytics_timeseries_params,
            false,
        ),
        (
            "/v1/debug/analytics/heatmap",
            "Get analytics heatmap",
            "Stable analytics weekday-by-hour heatmap compatibility endpoint.",
            analytics_range_params,
            false,
        ),
        (
            "/v1/debug/governance",
            "Get traffic governance state",
            "Effective gateway policy, traffic capture, redaction, middleware controls, and recent allow/deny/throttle decisions.",
            limit_params.clone(),
            false,
        ),
        (
            "/v1/debug/search-telemetry",
            "Get search-quality telemetry",
            "Recent prompt-safe search records and aggregate search-to-describe/load/call hit-rate metrics.",
            limit_params.clone(),
            false,
        ),
        (
            "/v1/debug/integrations",
            "Get integration configuration state",
            "Stable agent-facing alias for the Admin integrations summary: Sentry, webhooks, WeCom message push, OTLP, and pending-restart state with secrets masked.",
            Vec::new(),
            false,
        ),
        (
            "/v1/debug/health",
            "Get debug subsystem health",
            "Compact health and readiness summary for debug providers.",
            Vec::new(),
            false,
        ),
    ] {
        let path_doc = if compact_capable {
            debug_get_path_compact(summary, description, params)
        } else {
            debug_get_path(summary, description, params)
        };
        paths.insert(path.to_string(), path_doc);
    }
    paths.insert(
        "/v1/debug/traffic/export".to_string(),
        debug_get_path_with_content(
            "Export live traffic capture JSONL",
            "Download the retained admin_live traffic.frame window as newline-delimited JSON.",
            limit_params.clone(),
            "application/x-ndjson",
            json!({"type": "string", "format": "binary"}),
        ),
    );
    paths.insert(
        "/v1/debug/analytics/export".to_string(),
        debug_get_path_with_contents(
            "Export analytics rows",
            "Download retained analytics audit rows as newline-delimited JSON or CSV.",
            analytics_export_params,
            vec![
                (
                    "application/x-ndjson",
                    json!({"type": "string", "format": "binary"}),
                ),
                ("text/csv", json!({"type": "string", "format": "binary"})),
            ],
        ),
    );

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
    debug_get_path_with_content(
        summary,
        description,
        parameters,
        "application/json",
        json!({"$ref": "#/components/schemas/GatewayDebugPayload"}),
    )
}

#[cfg(feature = "admin")]
fn debug_get_path_compact(summary: &str, description: &str, parameters: Vec<Value>) -> Value {
    let mut path = debug_get_path(summary, description, parameters);
    if let Some(content) = path
        .pointer_mut("/get/responses/200/content")
        .and_then(Value::as_object_mut)
    {
        content.insert(
            crate::gateway::response_codec::TOON_MIME.to_string(),
            json!({
                "schema": {"type": "string"},
                "description": "TOON-encoded compact debug payload.",
            }),
        );
    }
    path
}

#[cfg(feature = "admin")]
fn debug_get_path_with_content(
    summary: &str,
    description: &str,
    parameters: Vec<Value>,
    content_type: &str,
    schema: Value,
) -> Value {
    debug_get_path_with_contents(
        summary,
        description,
        parameters,
        vec![(content_type, schema)],
    )
}

#[cfg(feature = "admin")]
fn debug_get_path_with_contents(
    summary: &str,
    description: &str,
    parameters: Vec<Value>,
    contents: Vec<(&str, Value)>,
) -> Value {
    let mut content = Map::new();
    for (content_type, schema) in contents {
        content.insert(content_type.to_string(), json!({ "schema": schema }));
    }
    json!({
        "get": {
            "tags": ["debug"],
            "summary": summary,
            "description": description,
            "parameters": parameters,
            "responses": {
                "200": {
                    "description": "Debug payload",
                    "content": Value::Object(content)
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

#[cfg(feature = "admin")]
fn header_param(name: &str, schema: Value, description: &str) -> Value {
    json!({
        "name": name,
        "in": "header",
        "required": false,
        "description": description,
        "schema": schema
    })
}

#[cfg(feature = "admin")]
fn compact_path_params(path: Value, compact_params: &[Value]) -> Vec<Value> {
    let mut params = vec![path];
    params.extend(compact_params.iter().cloned());
    params
}
