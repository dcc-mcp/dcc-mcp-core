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
        post_operation(
            &["skills"],
            "Search gateway capabilities",
            "Searches loaded and unloaded capabilities across live DCC instances.",
            request_body_ref("SearchRequest"),
            gateway_json_response_ref("SearchResponse"),
        ),
    );
    paths.insert(
        "/v1/load_skill".to_string(),
        post_operation(
            &["skills"],
            "Load a backend skill",
            "Loads a skill on a selected backend instance. Gateway load_skill defaults to lazy group activation.",
            request_body_ref("LoadSkillRequest"),
            gateway_json_response_ref("SkillLifecycleResponse"),
        ),
    );
    paths.insert(
        "/v1/unload_skill".to_string(),
        post_operation(
            &["skills"],
            "Unload a backend skill",
            "Unloads a skill from a selected backend instance.",
            request_body_ref("UnloadSkillRequest"),
            gateway_json_response_ref("SkillLifecycleResponse"),
        ),
    );
    paths.insert(
        "/v1/describe".to_string(),
        post_operation(
            &["tools"],
            "Describe a gateway capability",
            "Resolves a gateway tool_slug and returns its schema, annotations, backend owner, and loading state.",
            request_body_ref("DescribeRequest"),
            gateway_json_response_ref("DescribeResponse"),
        ),
    );
    paths.insert(
        "/v1/tools/{slug}".to_string(),
        get_operation_with_params(
            &["tools"],
            "Describe a gateway capability by URL slug",
            "URL alias for /v1/describe.",
            vec![path_param("slug", "Gateway capability slug.")],
            gateway_json_response_ref("DescribeResponse"),
        ),
    );
    paths.insert(
        "/v1/call".to_string(),
        post_operation(
            &["tools"],
            "Call a gateway capability",
            "Invokes one gateway capability by tool_slug.",
            request_body_ref("CallRequest"),
            gateway_json_response_ref("CallOutcome"),
        ),
    );
    paths.insert(
        "/v1/call_batch".to_string(),
        post_operation(
            &["tools"],
            "Call multiple gateway capabilities",
            "Invokes up to 25 gateway capabilities in order with optional stop_on_error semantics.",
            request_body_ref("GatewayCallBatchRequest"),
            gateway_json_response_ref("GatewayCallBatchResponse"),
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
            ],
            gateway_json_response_ref("DescribeResponse"),
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
            ],
            request_body_ref("GatewayDirectCallRequest"),
            gateway_json_response_ref("CallOutcome"),
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

    schemas
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
                "required": ["instance_id", "dcc_type", "status", "mcp_url"],
                "properties": {
                    "instance_id": {"type": "string", "format": "uuid"},
                    "instance_short": {"type": "string"},
                    "display_id": {"type": "string"},
                    "dcc_type": {"type": "string"},
                    "status": {"type": "string"},
                    "mcp_url": {"type": "string"},
                    "lifecycle": {"type": "object", "additionalProperties": true},
                    "diagnostics": {"type": "object", "additionalProperties": true}
                },
                "additionalProperties": true,
            }),
        ),
        (
            "GatewayInstanceList",
            json!({
                "type": "object",
                "required": ["total", "instances"],
                "properties": {
                    "total": {"type": "integer", "minimum": 0},
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
            "GatewayDirectCallRequest",
            json!({
                "type": "object",
                "required": ["backend_tool"],
                "properties": {
                    "backend_tool": {"type": "string"},
                    "arguments": {"type": "object", "additionalProperties": true},
                    "params": {"type": "object", "additionalProperties": true},
                    "meta": {"type": "object", "additionalProperties": true}
                },
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

fn gateway_json_response_ref(schema: &str) -> Value {
    let mut response = json_response_ref(schema);
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
        "traceparent": {
            "description": "W3C trace context for downstream HTTP clients.",
            "schema": {"type": "string"}
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
        assert!(search_headers.get("traceparent").is_some());

        let batch_item = &doc["components"]["schemas"]["GatewayBatchCallItem"];
        assert!(batch_item["properties"].get("id").is_some());
        let response = &doc["components"]["schemas"]["GatewayCallBatchResponse"];
        assert!(
            response["properties"]["results"]["items"]["properties"]
                .get("id")
                .is_some()
        );
        assert!(response["properties"].get("request_id").is_some());
        assert!(response["properties"].get("trace_id").is_some());
        assert!(response["properties"].get("index_generation").is_some());
    }
}
