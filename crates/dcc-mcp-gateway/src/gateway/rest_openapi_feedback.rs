//! OpenAPI fragments for gateway-level agent feedback.

use serde_json::{Value, json};

use dcc_mcp_models::FINDING_V1_JSON_SCHEMA;

pub(super) fn path_operation() -> Value {
    let mut operation = super::rest_openapi::post_operation(
        &["feedback"],
        "File gateway-level agent feedback",
        "Records structured feedback even when the referenced DCC instance has already exited. The receipt points to the gateway event resource containing the bounded in-band record.",
        super::rest_openapi::request_body_ref("GatewayFeedbackReport"),
        super::rest_openapi::json_response_ref("GatewayFeedbackReceipt"),
    );
    let responses = operation["post"]["responses"]
        .as_object_mut()
        .expect("POST operation responses must be an object");
    let mut response = responses
        .remove("200")
        .expect("POST operation must define a success response");
    response["headers"] = json!({
        "X-Request-ID": {
            "description": "Exact echo of the optional request correlation header.",
            "schema": {"type": "string"}
        }
    });
    responses.insert("201".to_string(), response);
    operation["post"]["parameters"] = json!([{
        "name": "X-Request-ID",
        "in": "header",
        "required": false,
        "description": "Optional transport correlation id echoed on success and validation errors.",
        "schema": {"type": "string"}
    }]);
    operation
}

pub(super) fn schemas() -> Vec<(&'static str, Value)> {
    let finding_schema = serde_json::from_str(FINDING_V1_JSON_SCHEMA)
        .expect("embedded Finding v1 JSON Schema must stay valid");
    vec![
        (
            "GatewayFeedbackReport",
            json!({
                "oneOf": [
                    {"$ref": "#/components/schemas/FeedbackFindingV1"},
                    {"$ref": "#/components/schemas/GatewayFeedbackLegacyReport"}
                ]
            }),
        ),
        ("FeedbackFindingV1", finding_schema),
        (
            "GatewayFeedbackLegacyReport",
            json!({
                "type": "object",
                "required": ["tool_name", "intent", "blocker", "severity"],
                "properties": {
                    "tool_name": {"type": "string", "minLength": 1, "maxLength": 256},
                    "intent": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "attempt": {"type": "string", "maxLength": 4096},
                    "blocker": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "severity": {"type": "string", "enum": ["blocked", "workaround_found", "suggestion"]},
                    "dcc_type": {"type": "string", "maxLength": 256},
                    "instance_id": {"type": "string", "maxLength": 256, "description": "May reference an instance that is no longer live."},
                    "request_id": {"type": "string", "maxLength": 256},
                    "job_id": {"type": "string", "maxLength": 256}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "GatewayFeedbackReceipt",
            json!({
                "type": "object",
                "required": ["ok", "success", "feedback_id", "recorded_at", "event_resource_uri"],
                "properties": {
                    "ok": {"type": "boolean", "const": true},
                    "success": {"type": "boolean", "const": true},
                    "feedback_id": {"type": "string", "format": "uuid"},
                    "recorded_at": {"type": "string", "format": "date-time"},
                    "event_resource_uri": {"type": "string", "const": "resources://gateway/events"},
                    "schema_version": {"type": "integer", "const": 1},
                    "fingerprint": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}
                },
                "additionalProperties": false,
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::rest_openapi::build_gateway_openapi_document;

    #[test]
    fn documents_feedback_without_live_instance_dependency() {
        let doc = build_gateway_openapi_document("1.2.3");
        let operation = &doc["paths"]["/v1/feedback"]["post"];
        assert_eq!(operation["tags"][0], "feedback");
        assert!(operation["responses"].get("200").is_none());
        assert_eq!(
            operation["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/GatewayFeedbackReceipt"
        );
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/GatewayFeedbackReport"
        );
        assert!(
            operation["parameters"]
                .as_array()
                .is_some_and(|parameters| {
                    parameters
                        .iter()
                        .any(|parameter| parameter["name"] == "X-Request-ID")
                })
        );
        assert_eq!(
            operation["responses"]["201"]["headers"]["X-Request-ID"]["schema"]["type"],
            "string"
        );
        let report = &doc["components"]["schemas"]["GatewayFeedbackReport"];
        assert_eq!(
            report["oneOf"][0]["$ref"],
            "#/components/schemas/FeedbackFindingV1"
        );
        let legacy = &doc["components"]["schemas"]["GatewayFeedbackLegacyReport"];
        assert!(legacy["properties"].get("instance_id").is_some());
        assert!(legacy["properties"].get("request_id").is_some());
        assert!(legacy["properties"].get("job_id").is_some());
        let finding = &doc["components"]["schemas"]["FeedbackFindingV1"];
        assert_eq!(finding["properties"]["schema_version"]["const"], 1);
        assert_eq!(
            doc["components"]["schemas"]["GatewayFeedbackReceipt"]["properties"]["fingerprint"]["pattern"],
            "^sha256:[0-9a-f]{64}$"
        );
    }
}
