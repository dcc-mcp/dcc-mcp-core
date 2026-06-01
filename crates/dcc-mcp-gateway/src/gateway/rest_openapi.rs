//! Gateway-specific OpenAPI contract for the canonical `/v1/*` REST surface.
//!
//! The gateway is an aggregating facade, not a per-DCC adapter server.  It
//! shares several request/response envelopes with `dcc-mcp-skill-rest`, but its
//! mounted routes are different.  Keep the path list here gateway-owned so
//! `GET /v1/openapi.json` does not advertise per-DCC-only resources, prompts, or
//! job routes.

use serde_json::{Map, Value, json};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GatewayOpenApiRoute {
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
}

#[cfg(test)]
pub(crate) const GATEWAY_OPENAPI_ROUTES: &[GatewayOpenApiRoute] = &[
    get("/v1/instances"),
    post("/v1/instances/register"),
    post("/v1/instances/heartbeat"),
    post("/v1/instances/deregister"),
    get("/v1/healthz"),
    get("/v1/readyz"),
    get("/v1/openapi.json"),
    get("/docs"),
    get("/v1/skills"),
    post("/v1/list_skills"),
    post("/v1/search"),
    post("/v1/load_skill"),
    post("/v1/unload_skill"),
    post("/v1/describe"),
    get("/v1/tools/{slug}"),
    post("/v1/call"),
    post("/v1/call_batch"),
    get("/v1/context"),
    get("/v1/dcc/{dcc_type}/instances/{instance_id}/describe"),
    post("/v1/dcc/{dcc_type}/instances/{instance_id}/call"),
    post("/v1/dcc/{dcc_type}/instances/{instance_id}/stop"),
];

#[cfg(test)]
const fn get(path: &'static str) -> GatewayOpenApiRoute {
    GatewayOpenApiRoute {
        method: "GET",
        path,
    }
}

#[cfg(test)]
const fn post(path: &'static str) -> GatewayOpenApiRoute {
    GatewayOpenApiRoute {
        method: "POST",
        path,
    }
}

#[must_use]
pub(crate) fn build_gateway_openapi_document(server_version: &str) -> Value {
    let mut doc = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "dcc-mcp-gateway",
            "description": "Gateway-specific REST API for multi-DCC discovery, skill loading, routing, and instance-scoped tool calls.",
            "version": server_version,
        },
        "tags": [
            {"name": "health", "description": "Gateway liveness and readiness probes."},
            {"name": "instances", "description": "Live DCC instance inventory."},
            {"name": "skills", "description": "Gateway skill discovery and lifecycle operations."},
            {"name": "tools", "description": "Gateway capability describe and invocation operations."},
            {"name": "meta", "description": "Gateway API self-description."},
            {"name": "lifecycle", "description": "Guarded test-owned instance lifecycle operations."}
        ],
        "paths": {},
        "components": {
            "schemas": common_schemas(),
        },
    });

    let paths = doc
        .get_mut("paths")
        .and_then(Value::as_object_mut)
        .expect("gateway OpenAPI document owns a paths object");

    paths.insert(
        "/v1/instances".to_string(),
        get_operation(
            &["instances"],
            "List live gateway instances",
            "Returns the gateway registry rows that are currently live and routable.",
            json_response_ref("GatewayInstanceList"),
        ),
    );
    paths.insert(
        "/v1/instances/register".to_string(),
        post_operation(
            &["instances"],
            "Register a remote gateway instance",
            "Adds or replaces a TTL-scoped DCC instance row using a direct MCP URL. This is the remote-machine companion to the local FileRegistry source.",
            request_body_ref("GatewayInstanceRegisterRequest"),
            json_response_ref("GatewayInstanceRegistrationResponse"),
        ),
    );
    paths.insert(
        "/v1/instances/heartbeat".to_string(),
        post_operation(
            &["instances"],
            "Refresh a remote gateway instance",
            "Refreshes the TTL for an HTTP-registered instance and may update scene or capability fingerprint metadata.",
            request_body_ref("GatewayInstanceHeartbeatRequest"),
            json_response_ref("GatewayInstanceRegistrationResponse"),
        ),
    );
    paths.insert(
        "/v1/instances/deregister".to_string(),
        post_operation(
            &["instances"],
            "Deregister a remote gateway instance",
            "Removes an HTTP-registered instance row and drops its cached gateway capability records.",
            request_body_ref("GatewayInstanceDeregisterRequest"),
            json_response_ref("GatewayInstanceRegistrationResponse"),
        ),
    );
    paths.insert(
        "/v1/healthz".to_string(),
        get_operation(
            &["health"],
            "Check gateway liveness",
            "Reports whether the gateway HTTP handler is alive.",
            json_response_ref("GatewayHealth"),
        ),
    );
    paths.insert(
        "/v1/readyz".to_string(),
        get_operation(
            &["health"],
            "Summarize gateway readiness",
            "Aggregates live instance readiness bits; the gateway itself remains reachable even when no DCC instance is ready.",
            json_response_ref("GatewayReadyz"),
        ),
    );
    paths.insert(
        "/v1/openapi.json".to_string(),
        get_operation(
            &["meta"],
            "Return the gateway OpenAPI document",
            "Returns this gateway-specific OpenAPI document.",
            json_response_ref("OpenApiDocument"),
        ),
    );
    paths.insert(
        "/docs".to_string(),
        get_operation(
            &["meta"],
            "Render the gateway API reference",
            "Renders Scalar using the same gateway-specific OpenAPI document served by /v1/openapi.json.",
            text_response("text/html"),
        ),
    );
    paths.insert(
        "/v1/skills".to_string(),
        get_operation(
            &["skills"],
            "List indexed gateway skills",
            "Lists loaded gateway capability records as skill-like entries.",
            json_response_ref("GatewaySkillList"),
        ),
    );
    paths.insert(
        "/v1/list_skills".to_string(),
        post_operation(
            &["skills"],
            "List backend skills",
            "Forwards a skill list request to one gateway-selected backend instance.",
            request_body_ref("GatewaySkillLifecycleRequest"),
            json_response_ref("SkillLifecycleResponse"),
        ),
    );
    paths.insert(
        "/v1/search".to_string(),
        post_operation_with_params(
            &["skills"],
            "Search gateway capabilities",
            "Searches loaded and unloaded capabilities across live DCC instances.",
            vec![accept_response_format_header()],
            request_body_ref("SearchRequest"),
            gateway_response_ref("SearchResponse"),
        ),
    );
    paths.insert(
        "/v1/load_skill".to_string(),
        post_operation_with_params(
            &["skills"],
            "Load a backend skill",
            "Loads a skill on a selected backend instance. Gateway load_skill defaults to lazy group activation.",
            vec![accept_response_format_header()],
            request_body_ref("LoadSkillRequest"),
            gateway_response_ref("SkillLifecycleResponse"),
        ),
    );
    paths.insert(
        "/v1/unload_skill".to_string(),
        post_operation_with_params(
            &["skills"],
            "Unload a backend skill",
            "Unloads a skill from a selected backend instance.",
            vec![accept_response_format_header()],
            request_body_ref("UnloadSkillRequest"),
            gateway_response_ref("SkillLifecycleResponse"),
        ),
    );
    paths.insert(
        "/v1/describe".to_string(),
        post_operation_with_params(
            &["tools"],
            "Describe a gateway capability",
            "Resolves a gateway tool_slug and returns its schema, annotations, backend owner, and loading state.",
            vec![accept_response_format_header()],
            request_body_ref("DescribeRequest"),
            gateway_response_ref("DescribeResponse"),
        ),
    );
    paths.insert(
        "/v1/tools/{slug}".to_string(),
        get_operation_with_params(
            &["tools"],
            "Describe a gateway capability by URL slug",
            "URL alias for /v1/describe.",
            vec![
                path_param("slug", "Gateway capability slug."),
                accept_response_format_header(),
            ],
            gateway_response_ref("DescribeResponse"),
        ),
    );
    paths.insert(
        "/v1/call".to_string(),
        post_operation_with_params(
            &["tools"],
            "Call a gateway capability",
            "Invokes one gateway capability by tool_slug, or up to 25 capabilities in order via calls[] with optional stop_on_error semantics. When calls is present the response shape follows the batch result envelope; single-call responses are returned unwrapped.",
            vec![accept_response_format_header()],
            request_body_ref("CallRequest"),
            gateway_response_ref("CallOutcome"),
        ),
    );
    paths.insert(
        "/v1/call_batch".to_string(),
        post_operation_with_params(
            &["tools"],
            "Call multiple gateway capabilities",
            "Invokes up to 25 gateway capabilities in order with optional stop_on_error semantics.",
            vec![accept_response_format_header()],
            request_body_ref("GatewayCallBatchRequest"),
            gateway_response_ref("GatewayCallBatchResponse"),
        ),
    );
    paths.insert(
        "/v1/context".to_string(),
        get_operation(
            &["instances"],
            "Return gateway context",
            "Returns gateway metadata, live instance rows, and aggregate capability counts.",
            json_response_ref("GatewayContext"),
        ),
    );
    paths.insert(
        "/v1/dcc/{dcc_type}/instances/{instance_id}/describe".to_string(),
        get_operation_with_params(
            &["tools"],
            "Describe an instance-scoped backend tool",
            "Describes a backend tool by DCC type and instance id or unique id prefix.",
            vec![
                path_param("dcc_type", "DCC type, for example maya or blender."),
                path_param(
                    "instance_id",
                    "Full instance UUID, instance_short, or unique UUID prefix.",
                ),
                query_param("backend_tool", true, "Backend callable id to describe."),
                accept_response_format_header(),
            ],
            gateway_response_ref("DescribeResponse"),
        ),
    );
    paths.insert(
        "/v1/dcc/{dcc_type}/instances/{instance_id}/call".to_string(),
        post_operation_with_params(
            &["tools"],
            "Call an instance-scoped backend tool",
            "Calls a backend tool by DCC type and instance id or unique id prefix.",
            vec![
                path_param("dcc_type", "DCC type, for example maya or blender."),
                path_param(
                    "instance_id",
                    "Full instance UUID, instance_short, or unique UUID prefix.",
                ),
                accept_response_format_header(),
            ],
            request_body_ref("GatewayDirectCallRequest"),
            gateway_response_ref("CallOutcome"),
        ),
    );
    paths.insert(
        "/v1/dcc/{dcc_type}/instances/{instance_id}/stop".to_string(),
        post_operation_with_params(
            &["lifecycle"],
            "Safely stop a test-owned instance",
            "Forwards a guarded stop request only when the instance advertises safe_stop_url metadata.",
            vec![
                path_param("dcc_type", "DCC type, for example maya or blender."),
                path_param("instance_id", "Full instance UUID, instance_short, or unique UUID prefix."),
            ],
            request_body_ref("GatewaySafeStopRequest"),
            json_response_ref("GatewaySafeStopResponse"),
        ),
    );

    doc
}

fn common_schemas() -> Map<String, Value> {
    let source = dcc_mcp_skill_rest::openapi::build_openapi_document("schema-source", "0.0.0");
    let mut schemas = source
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    for (name, schema) in gateway_schemas() {
        schemas.insert(name.to_string(), schema);
    }
    annotate_service_error_policy(&mut schemas);
    annotate_search_quality_contract(&mut schemas);
    annotate_response_format_controls(&mut schemas);

    schemas
}

fn annotate_service_error_policy(schemas: &mut Map<String, Value>) {
    let gateway_error = json!({
        "type": "object",
        "required": ["error"],
        "properties": {
            "error": {
                "type": "object",
                "required": ["kind", "message"],
                "properties": {
                    "kind": {"type": "string"},
                    "message": {"type": "string"},
                    "candidates": {"type": "array", "items": {"type": "object", "additionalProperties": true}},
                    "previous_status": {"type": "string"},
                    "previous_instance_id": {"type": "string"},
                    "backend": {"type": "object", "additionalProperties": true},
                    "policy": {"$ref": "#/components/schemas/GatewayPolicyDenial"}
                },
                "additionalProperties": true
            }
        },
        "additionalProperties": true
    });
    schemas.insert("ServiceError".to_string(), gateway_error);
}

fn annotate_search_quality_contract(schemas: &mut Map<String, Value>) {
    if let Some(search_response) = schemas.get_mut("SearchResponse")
        && let Some(properties) = search_response
            .get_mut("properties")
            .and_then(Value::as_object_mut)
    {
        properties.insert(
            "search_id".to_string(),
            json!({
                "type": "string",
                "description": "Stable correlation id for this search result set. Pass it as meta.search_id on describe, load_skill, call, or batch follow-ups."
            }),
        );
        properties.insert(
            "ranker_version".to_string(),
            json!({
                "type": "string",
                "description": "Bounded ranker identifier used for the response."
            }),
        );
        properties.insert(
            "index_generation".to_string(),
            json!({
                "type": "string",
                "description": "Opaque capability-index fingerprint used to produce the result set."
            }),
        );
    }

    if let Some(skill_entry) = schemas.get_mut("SkillListEntry")
        && let Some(properties) = skill_entry
            .get_mut("properties")
            .and_then(Value::as_object_mut)
    {
        properties.insert(
            "rank".to_string(),
            json!({
                "type": "integer",
                "minimum": 1,
                "description": "One-based rank within the returned search result set."
            }),
        );
        properties.insert(
            "score".to_string(),
            json!({"type": "integer", "minimum": 0}),
        );
        properties.insert(
            "match_reasons".to_string(),
            json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Bounded ranking reason labels, for example tool_lexical or schema_fuzzy."
            }),
        );
        properties.insert(
            "next_step".to_string(),
            json!({
                "$ref": "#/components/schemas/ProgressiveNextStep",
                "description": "Suggested follow-up with meta.search_id attached when the hit is unloaded or ready for describe/call."
            }),
        );
    }
}

fn annotate_response_format_controls(schemas: &mut Map<String, Value>) {
    for schema_name in [
        "SearchRequest",
        "LoadSkillRequest",
        "UnloadSkillRequest",
        "DescribeRequest",
        "CallRequest",
        "GatewayDirectCallRequest",
        "GatewayCallBatchRequest",
    ] {
        if let Some(schema) = schemas.get_mut(schema_name)
            && let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut)
        {
            properties.insert(
                "response_format".to_string(),
                json!({
                    "type": "string",
                    "enum": ["toon", "json"],
                    "description": "Optional response-format override. Omit for the gateway default compact TOON response; set json for legacy compatibility."
                }),
            );
            properties.insert(
                "compact".to_string(),
                json!({
                    "type": "boolean",
                    "description": "Alias for response_format=toon when true."
                }),
            );
        }
    }
}

fn gateway_schemas() -> Vec<(&'static str, Value)> {
    vec![
        (
            "GatewayHealth",
            json!({
                "type": "object",
                "required": ["ok"],
                "properties": {"ok": {"type": "boolean"}},
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayInstance",
            json!({
                "type": "object",
                "description": "Live gateway registry row. Custom DCC metadata is preserved as additional properties.",
                "required": ["instance_id", "instance_short", "dcc_type", "status", "mcp_url", "source", "source_meta"],
                "properties": {
                    "instance_id": {"type": "string", "format": "uuid"},
                    "instance_short": {"type": "string"},
                    "display_id": {"type": "string"},
                    "dcc_type": {"type": "string"},
                    "status": {"type": "string"},
                    "mcp_url": {"type": "string"},
                    "source": {"type": "string", "enum": ["file", "http", "mdns", "relay"]},
                    "source_meta": {"type": "object", "additionalProperties": true},
                    "lifecycle": {"type": "object", "additionalProperties": true},
                    "diagnostics": {"type": "object", "additionalProperties": true}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayInstanceRegisterRequest",
            json!({
                "type": "object",
                "required": ["instance_id", "dcc_type", "mcp_url"],
                "properties": {
                    "instance_id": {"type": "string", "format": "uuid"},
                    "dcc_type": {"type": "string"},
                    "mcp_url": {
                        "type": "string",
                        "format": "uri",
                        "description": "Direct MCP endpoint URL for the backend, usually ending in /mcp."
                    },
                    "capabilities_fingerprint": {"type": "string"},
                    "adapter_version": {"type": "string"},
                    "scene": {"type": "string"},
                    "ttl_secs": {"type": "integer", "minimum": 1, "maximum": 86400}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayInstanceHeartbeatRequest",
            json!({
                "type": "object",
                "required": ["instance_id"],
                "properties": {
                    "instance_id": {"type": "string", "format": "uuid"},
                    "capabilities_fingerprint": {"type": "string"},
                    "scene": {"type": "string"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayInstanceDeregisterRequest",
            json!({
                "type": "object",
                "required": ["instance_id"],
                "properties": {
                    "instance_id": {"type": "string", "format": "uuid"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayInstanceRegistrationResponse",
            json!({
                "type": "object",
                "required": ["ok", "success"],
                "properties": {
                    "ok": {"type": "boolean"},
                    "success": {"type": "boolean"},
                    "operation": {"type": "string"},
                    "instance_id": {"type": "string", "format": "uuid"},
                    "instance_short": {"type": "string"},
                    "registered_at": {"type": "integer"},
                    "heartbeat_interval_secs": {"type": "integer", "minimum": 1},
                    "error": {"$ref": "#/components/schemas/ServiceError"}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayInstanceList",
            json!({
                "type": "object",
                "required": ["total", "by_source", "instances"],
                "properties": {
                    "total": {"type": "integer", "minimum": 0},
                    "by_source": {
                        "type": "object",
                        "additionalProperties": {"type": "integer", "minimum": 0}
                    },
                    "instances": {
                        "type": "array",
                        "items": {"$ref": "#/components/schemas/GatewayInstance"}
                    }
                },
            }),
        ),
        (
            "GatewayReadyz",
            json!({
                "type": "object",
                "required": ["ok", "checks", "live_instance_count", "ready_instance_count", "not_ready_instance_count", "instances"],
                "properties": {
                    "ok": {"type": "boolean"},
                    "checks": {
                        "type": "array",
                        "items": {"type": "object", "additionalProperties": true}
                    },
                    "live_instance_count": {"type": "integer", "minimum": 0},
                    "ready_instance_count": {"type": "integer", "minimum": 0},
                    "not_ready_instance_count": {"type": "integer", "minimum": 0},
                    "instances": {
                        "type": "array",
                        "items": {"$ref": "#/components/schemas/GatewayInstance"}
                    }
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayContext",
            json!({
                "type": "object",
                "required": ["dcc", "version", "instances"],
                "properties": {
                    "dcc": {"type": "string", "const": "gateway"},
                    "version": {"type": "string"},
                    "display_name": {"type": "string"},
                    "instances": {
                        "type": "array",
                        "items": {"$ref": "#/components/schemas/GatewayInstance"}
                    },
                    "capabilities": {"type": "object", "additionalProperties": true},
                    "loaded_skill_count": {"type": "integer", "minimum": 0},
                    "tool_count": {"type": "integer", "minimum": 0}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewaySkillList",
            json!({
                "type": "object",
                "required": ["total", "skills"],
                "properties": {
                    "total": {"type": "integer", "minimum": 0},
                    "skills": {
                        "type": "array",
                        "items": {"type": "object", "additionalProperties": true}
                    }
                },
            }),
        ),
        (
            "GatewaySkillLifecycleRequest",
            json!({
                "type": "object",
                "properties": {
                    "dcc_type": {"type": "string"},
                    "instance_id": {"type": "string"},
                    "query": {"type": "string"}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayPolicyDenial",
            json!({
                "type": "object",
                "required": ["reason", "operation", "message", "read_only"],
                "properties": {
                    "reason": {
                        "type": "string",
                        "enum": ["read-only", "dcc-allowlist", "skill-allowlist", "tool-allowlist"]
                    },
                    "operation": {
                        "type": "string",
                        "enum": ["search", "describe", "load_skill", "call"]
                    },
                    "message": {"type": "string"},
                    "read_only": {"type": "boolean"},
                    "dcc_type": {"type": "string"},
                    "skill_name": {"type": "string"},
                    "tool_slug": {"type": "string"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayDirectCallRequest",
            json!({
                "type": "object",
                "required": ["backend_tool"],
                "properties": {
                    "backend_tool": {"type": "string"},
                    "arguments": {"type": "object", "additionalProperties": true},
                    "params": {"type": "object", "additionalProperties": true},
                    "meta": {"type": "object", "additionalProperties": true},
                    "response_format": {"type": "string", "enum": ["toon", "json"]},
                    "compact": {"type": "boolean"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "CallRequest",
            json!({
                "type": "object",
                "description": "Invoke one gateway capability by tool_slug, or run an ordered batch via calls (maximum 25).",
                "properties": {
                    "tool_slug": {"type": "string"},
                    "arguments": {"type": "object", "additionalProperties": true, "default": {}},
                    "params": {"type": "object", "additionalProperties": true},
                    "meta": {"type": "object", "additionalProperties": true},
                    "calls": {
                        "type": "array",
                        "maxItems": 25,
                        "items": {"$ref": "#/components/schemas/GatewayBatchCallItem"}
                    },
                    "stop_on_error": {"type": "boolean", "default": false},
                    "response_format": {"type": "string", "enum": ["toon", "json"]},
                    "compact": {"type": "boolean"}
                },
                "anyOf": [
                    {"required": ["tool_slug"]},
                    {"required": ["calls"]}
                ],
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayBatchCallItem",
            json!({
                "type": "object",
                "required": ["tool_slug"],
                "properties": {
                    "id": {
                        "description": "Optional client correlation id echoed unchanged in the matching result item.",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "integer"},
                            {"type": "number"},
                            {"type": "boolean"}
                        ]
                    },
                    "tool_slug": {"type": "string"},
                    "arguments": {"type": "object", "additionalProperties": true},
                    "params": {"type": "object", "additionalProperties": true},
                    "meta": {"type": "object", "additionalProperties": true}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayCallBatchRequest",
            json!({
                "type": "object",
                "required": ["calls"],
                "properties": {
                    "calls": {
                        "type": "array",
                        "maxItems": 25,
                        "items": {"$ref": "#/components/schemas/GatewayBatchCallItem"}
                    },
                    "stop_on_error": {"type": "boolean"},
                    "meta": {"type": "object", "additionalProperties": true},
                    "response_format": {"type": "string", "enum": ["json", "toon"]},
                    "compact": {"type": "boolean"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayCallBatchResponse",
            json!({
                "type": "object",
                "required": ["results"],
                "properties": {
                    "request_id": {"type": "string"},
                    "trace_id": {"type": "string"},
                    "index_generation": {
                        "type": "string",
                        "description": "Opaque capability-index fingerprint after the batch completes."
                    },
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["index", "ok"],
                            "properties": {
                                "index": {"type": "integer", "minimum": 0},
                                "id": {"description": "Client id echoed from the corresponding request item."},
                                "tool_slug": {"type": "string"},
                                "ok": {"type": "boolean"},
                                "result": {"type": "object", "additionalProperties": true},
                                "error": {"type": "object", "additionalProperties": true}
                            },
                            "additionalProperties": true
                        }
                    },
                    "stop_on_error": {"type": "boolean"}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewaySafeStopRequest",
            json!({
                "type": "object",
                "properties": {
                    "expected_owner": {"type": "string"},
                    "expected_session": {"type": "string"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewaySafeStopResponse",
            json!({
                "type": "object",
                "required": ["ok", "stopping", "instance_id", "dcc_type"],
                "properties": {
                    "ok": {"type": "boolean"},
                    "stopping": {"type": "boolean"},
                    "instance_id": {"type": "string", "format": "uuid"},
                    "dcc_type": {"type": "string"},
                    "safe_stop_url": {"type": "string"},
                    "response": {"type": "object", "additionalProperties": true}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "OpenApiDocument",
            json!({
                "type": "object",
                "required": ["openapi", "info", "paths"],
                "additionalProperties": true,
            }),
        ),
    ]
}

fn get_operation(tags: &[&str], summary: &str, description: &str, response: Value) -> Value {
    get_operation_with_params(tags, summary, description, Vec::new(), response)
}

fn get_operation_with_params(
    tags: &[&str],
    summary: &str,
    description: &str,
    parameters: Vec<Value>,
    response: Value,
) -> Value {
    operation(
        "get",
        tags,
        summary,
        description,
        parameters,
        None,
        response,
    )
}

fn post_operation(
    tags: &[&str],
    summary: &str,
    description: &str,
    request_body: Value,
    response: Value,
) -> Value {
    post_operation_with_params(
        tags,
        summary,
        description,
        Vec::new(),
        request_body,
        response,
    )
}

fn post_operation_with_params(
    tags: &[&str],
    summary: &str,
    description: &str,
    parameters: Vec<Value>,
    request_body: Value,
    response: Value,
) -> Value {
    operation(
        "post",
        tags,
        summary,
        description,
        parameters,
        Some(request_body),
        response,
    )
}

fn operation(
    method: &str,
    tags: &[&str],
    summary: &str,
    description: &str,
    parameters: Vec<Value>,
    request_body: Option<Value>,
    response: Value,
) -> Value {
    let mut op = json!({
        "tags": tags,
        "summary": summary,
        "description": description,
        "responses": {
            "200": response,
            "400": error_response("Bad request"),
            "403": error_response("Gateway policy denied"),
            "404": error_response("Not found"),
            "409": error_response("Conflict"),
            "502": error_response("Backend error"),
            "503": error_response("Unavailable")
        }
    });
    if !parameters.is_empty() {
        op["parameters"] = json!(parameters);
    }
    if let Some(request_body) = request_body {
        op["requestBody"] = request_body;
    }
    json!({method: op})
}

fn request_body_ref(schema: &str) -> Value {
    json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": {"$ref": format!("#/components/schemas/{schema}")}
            }
        }
    })
}

fn json_response_ref(schema: &str) -> Value {
    json!({
        "description": "JSON response",
        "content": {
            "application/json": {
                "schema": {"$ref": format!("#/components/schemas/{schema}")}
            }
        }
    })
}

fn gateway_response_ref(schema: &str) -> Value {
    let mut response = json_response_ref(schema);
    response["description"] =
        json!("Compact TOON by default; legacy JSON when the request opts out.");
    response["content"][crate::gateway::response_codec::TOON_MIME] = json!({
        "schema": {"type": "string"},
        "examples": {
            "toon": {
                "summary": "TOON-encoded compact payload",
                "value": "total:1\nhits[1]{tool_slug,summary}:\n  maya.abcdef01.render,Render current frame"
            }
        }
    });
    response["headers"] = gateway_metadata_headers();
    response
}

fn gateway_metadata_headers() -> Value {
    json!({
        "x-dcc-mcp-request-id": {
            "description": "Gateway request id. Mirrors client X-Request-Id when supplied.",
            "schema": {"type": "string"}
        },
        "x-dcc-mcp-trace-id": {
            "description": "End-to-end trace id propagated through gateway, sidecar, and host calls.",
            "schema": {"type": "string"}
        },
        "x-dcc-mcp-index-generation": {
            "description": "Opaque capability-index fingerprint when the route touches discovery, describe, load, or call state.",
            "schema": {"type": "string"}
        },
        "x-dcc-mcp-search-id": {
            "description": "Stable search correlation id when a route creates or consumes search-quality telemetry.",
            "schema": {"type": "string"}
        },
        "x-dcc-mcp-ranker-version": {
            "description": "Bounded search ranker identifier when a route creates or consumes search-quality telemetry.",
            "schema": {"type": "string"}
        },
        "x-dcc-mcp-response-format": {
            "description": "Returned response format: toon by default, json when explicitly requested for compatibility.",
            "schema": {"type": "string", "enum": ["toon", "json"]}
        },
        "x-dcc-mcp-token-estimator": {
            "description": "Approximate token estimator id used for x-dcc-mcp-* token counts.",
            "schema": {"type": "string", "const": crate::gateway::response_codec::TOKEN_ESTIMATOR}
        },
        "x-dcc-mcp-original-bytes": {
            "description": "Serialized legacy JSON byte count before compaction.",
            "schema": {"type": "integer", "minimum": 0}
        },
        "x-dcc-mcp-returned-bytes": {
            "description": "Returned response body byte count.",
            "schema": {"type": "integer", "minimum": 0}
        },
        "x-dcc-mcp-original-tokens": {
            "description": "Approximate legacy JSON token count.",
            "schema": {"type": "integer", "minimum": 0}
        },
        "x-dcc-mcp-returned-tokens": {
            "description": "Approximate returned response token count.",
            "schema": {"type": "integer", "minimum": 0}
        },
        "x-dcc-mcp-saved-tokens": {
            "description": "Approximate tokens saved compared with legacy JSON.",
            "schema": {"type": "integer", "minimum": 0}
        },
        "x-dcc-mcp-savings-pct": {
            "description": "Approximate percent saved compared with legacy JSON.",
            "schema": {"type": "string"}
        },
        "traceparent": {
            "description": "W3C trace context for downstream HTTP clients.",
            "schema": {"type": "string"}
        }
    })
}

fn accept_response_format_header() -> Value {
    json!({
        "name": "Accept",
        "in": "header",
        "required": false,
        "description": "Set application/json to opt out of the default compact TOON response; set application/toon or omit for compact output.",
        "schema": {
            "type": "string",
            "enum": [
                crate::gateway::response_codec::TOON_MIME,
                crate::gateway::response_codec::JSON_MIME
            ]
        }
    })
}

fn text_response(content_type: &str) -> Value {
    json!({
        "description": "Text response",
        "content": {
            content_type: {
                "schema": {"type": "string"}
            }
        }
    })
}

fn error_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": {
            "application/json": {
                "schema": {"$ref": "#/components/schemas/ServiceError"}
            }
        }
    })
}

fn path_param(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "description": description,
        "schema": {"type": "string"}
    })
}

fn query_param(name: &str, required: bool, description: &str) -> Value {
    json!({
        "name": name,
        "in": "query",
        "required": required,
        "description": description,
        "schema": {"type": "string"}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn gateway_openapi_lists_only_gateway_routes() {
        let doc = build_gateway_openapi_document("1.2.3");
        assert_eq!(doc["info"]["title"], "dcc-mcp-gateway");
        assert_eq!(doc["info"]["version"], "1.2.3");

        let paths = doc["paths"].as_object().expect("paths object");
        let expected: HashSet<_> = GATEWAY_OPENAPI_ROUTES
            .iter()
            .map(|route| route.path)
            .collect();
        let actual: HashSet<_> = paths.keys().map(String::as_str).collect();
        assert_eq!(actual, expected);

        for forbidden in [
            "/v1/resources",
            "/v1/resources/{uri}",
            "/v1/resources/{uri}/events",
            "/v1/prompts",
            "/v1/prompts/{name}",
            "/v1/jobs/{id}/events",
            "/v1/jobs/{id}",
            "/v1/dcc/{dcc_type}/call",
        ] {
            assert!(
                !paths.contains_key(forbidden),
                "gateway OpenAPI must not advertise per-DCC-only path {forbidden}"
            );
        }
    }

    #[test]
    fn gateway_openapi_keeps_shared_envelope_schemas() {
        let doc = build_gateway_openapi_document("1.2.3");
        let schemas = doc["components"]["schemas"]
            .as_object()
            .expect("schemas object");
        for schema in [
            "ServiceError",
            "SearchRequest",
            "SearchResponse",
            "LoadSkillRequest",
            "UnloadSkillRequest",
            "SkillLifecycleResponse",
            "DescribeRequest",
            "DescribeResponse",
            "CallRequest",
            "CallOutcome",
            "GatewayDirectCallRequest",
            "GatewayPolicyDenial",
            "GatewayBatchCallItem",
            "GatewayCallBatchRequest",
        ] {
            assert!(
                schemas.contains_key(schema),
                "gateway OpenAPI schema set missing {schema}"
            );
        }
    }

    #[test]
    fn gateway_openapi_documents_metadata_headers_and_batch_ids() {
        let doc = build_gateway_openapi_document("1.2.3");
        let search_headers = &doc["paths"]["/v1/search"]["post"]["responses"]["200"]["headers"];
        assert!(search_headers.get("x-dcc-mcp-request-id").is_some());
        assert!(search_headers.get("x-dcc-mcp-trace-id").is_some());
        assert!(search_headers.get("x-dcc-mcp-index-generation").is_some());
        assert!(search_headers.get("x-dcc-mcp-search-id").is_some());
        assert!(search_headers.get("x-dcc-mcp-ranker-version").is_some());
        assert!(search_headers.get("x-dcc-mcp-response-format").is_some());
        assert!(search_headers.get("x-dcc-mcp-token-estimator").is_some());
        assert!(search_headers.get("x-dcc-mcp-saved-tokens").is_some());
        assert!(search_headers.get("traceparent").is_some());
        assert!(
            doc["paths"]["/v1/search"]["post"]["responses"]["200"]["content"]
                .get(crate::gateway::response_codec::TOON_MIME)
                .is_some()
        );
        assert!(
            doc["paths"]["/v1/search"]["post"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|param| param["name"] == "Accept")
        );

        let search_response = &doc["components"]["schemas"]["SearchResponse"];
        assert!(search_response["properties"].get("search_id").is_some());
        assert!(
            search_response["properties"]
                .get("ranker_version")
                .is_some()
        );
        let search_hit = &doc["components"]["schemas"]["SkillListEntry"];
        assert!(search_hit["properties"].get("rank").is_some());
        assert!(search_hit["properties"].get("match_reasons").is_some());

        let batch_item = &doc["components"]["schemas"]["GatewayBatchCallItem"];
        assert!(batch_item["properties"].get("id").is_some());
        let search_request = &doc["components"]["schemas"]["SearchRequest"];
        assert!(
            search_request["properties"]
                .get("response_format")
                .is_some()
        );
        assert!(search_request["properties"].get("compact").is_some());
        let call_request = &doc["components"]["schemas"]["CallRequest"];
        assert_eq!(
            call_request["properties"]["response_format"]["description"],
            "Optional response-format override. Omit for the gateway default compact TOON response; set json for legacy compatibility."
        );
        let response = &doc["components"]["schemas"]["GatewayCallBatchResponse"];
        assert!(
            response["properties"]["results"]["items"]["properties"]
                .get("id")
                .is_some()
        );
        assert!(response["properties"].get("request_id").is_some());
        assert!(response["properties"].get("trace_id").is_some());
        assert!(response["properties"].get("index_generation").is_some());
        assert!(
            doc["paths"]["/v1/call"]["post"]["responses"]
                .get("403")
                .is_some()
        );
        assert!(
            doc["components"]["schemas"]["ServiceError"]["properties"]["error"]["properties"]
                .get("policy")
                .is_some()
        );
    }
}
