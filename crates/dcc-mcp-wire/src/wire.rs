//! Canonical encode/decode for MCP JSON-RPC and gateway REST surfaces.
//!
//! - Works with [`serde_json::Value`] — no dependency on full server frameworks.
//! - Produces [`WireError`] variants — stable keys, no string matching.
//! - Normalises `arguments` and `meta` via [`crate::normalize`].
//! - Validates shapes via [`crate::validate`].

use crate::{WireError, WireResult};
use serde_json::{Map, Value, json};

/// Decoded tool call tuple: `(name_or_slug, normalized_arguments, normalized_meta)`.
pub type DecodedCall = (String, Value, Option<Map<String, Value>>);

/// Decoded batch call list.
pub type DecodedBatch = Vec<DecodedCall>;

/// Parse a raw JSON-RPC `tools/call` request into (name, arguments, meta).
pub fn decode_call_tool(request: &Value) -> WireResult<DecodedCall> {
    let obj = request.as_object().ok_or(WireError::EnvelopeNotObject)?;
    let params = obj.get("params").ok_or_else(|| WireError::MissingField {
        field: "params".to_string(),
    })?;
    let params_obj = params.as_object().ok_or_else(|| WireError::MissingField {
        field: "params (must be object)".to_string(),
    })?;
    let name = params_obj
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| WireError::MissingField {
            field: "name".to_string(),
        })?
        .to_string();
    let arguments = params_obj.get("arguments").cloned();
    let normalised = crate::normalize::normalize_arguments(arguments)?;
    let meta = crate::normalize::normalize_meta(params_obj.get("_meta").cloned())?;
    Ok((name, normalised, meta))
}

/// Parse a gateway REST `POST /v1/call` request into (tool_slug, arguments, meta).
pub fn decode_rest_call(request: &Value) -> WireResult<DecodedCall> {
    let obj = request.as_object().ok_or(WireError::EnvelopeNotObject)?;
    let name = obj
        .get("tool_slug")
        .or_else(|| obj.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| WireError::MissingField {
            field: "tool_slug (or name)".to_string(),
        })?
        .to_string();
    let arguments = obj.get("arguments").or_else(|| obj.get("params")).cloned();
    let normalised = crate::normalize::normalize_arguments(arguments)?;
    let meta = crate::normalize::normalize_meta(obj.get("_meta").cloned())?;
    Ok((name, normalised, meta))
}

/// Build a canonical MCP `CallToolResult` envelope.
pub fn encode_call_tool_result(
    content_text: &str,
    is_error: bool,
    structured_content: Option<&Value>,
    meta: Option<&Map<String, Value>>,
) -> Value {
    let mut content = Vec::new();
    content.push(json!({"type": "text", "text": content_text}));
    let mut result = Map::new();
    result.insert("content".to_string(), Value::Array(content));
    result.insert("isError".to_string(), Value::Bool(is_error));
    if let Some(sc) = structured_content {
        result.insert("structuredContent".to_string(), sc.clone());
    }
    if let Some(m) = meta {
        result.insert("_meta".to_string(), Value::Object(m.clone()));
    }
    Value::Object(result)
}

/// Encode a gateway REST `POST /v1/call` success response.
pub fn encode_rest_call_response(
    slug: &str,
    output: &Value,
    validation_skipped: bool,
    request_id: &str,
) -> Value {
    json!({
        "slug": slug,
        "output": output,
        "validation_skipped": validation_skipped,
        "request_id": request_id
    })
}

/// Encode a structured error response.
pub fn encode_error_response(error: &WireError, request_id: Option<&str>) -> Value {
    let mut body = Map::new();
    body.insert("kind".to_string(), Value::String(error.kind().to_string()));
    body.insert("message".to_string(), Value::String(error.to_string()));
    body.insert("hint".to_string(), Value::String(error.hint()));
    if let Some(rid) = request_id {
        body.insert("request_id".to_string(), Value::String(rid.to_string()));
    }
    Value::Object(body)
}

/// Parse a JSON-RPC batch request into individual message values.
pub fn parse_json_rpc_batch(payload: &Value) -> WireResult<Vec<Value>> {
    match payload {
        Value::Array(items) => Ok(items.clone()),
        Value::Object(_) => Ok(vec![payload.clone()]),
        _ => Err(WireError::EnvelopeNotObject),
    }
}

/// Normalise a `call_batch` REST payload into (tool_slug, arguments, meta) triples.
pub fn normalize_call_batch(payload: &Value) -> WireResult<DecodedBatch> {
    let calls = match payload {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(obj) => {
            if let Some(Value::Array(calls)) = obj.get("calls") {
                calls.as_slice()
            } else {
                return Err(WireError::MissingField {
                    field: "calls (array)".to_string(),
                });
            }
        }
        _ => return Err(WireError::EnvelopeNotObject),
    };

    let mut results = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        let obj = match call {
            Value::Object(map) => map,
            _ => {
                return Err(WireError::BatchItemInvalid {
                    index,
                    reason: "each call must be a JSON object".to_string(),
                });
            }
        };

        let slug = obj
            .get("tool_slug")
            .or_else(|| obj.get("name"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| WireError::BatchItemInvalid {
                index,
                reason: "missing `tool_slug` or `name`".to_string(),
            })?
            .to_string();

        let arguments = obj.get("arguments").or_else(|| obj.get("params")).cloned();

        let normalised = crate::normalize::normalize_arguments(arguments).map_err(|e| {
            WireError::BatchItemInvalid {
                index,
                reason: e.to_string(),
            }
        })?;

        let meta = crate::normalize::normalize_meta(obj.get("_meta").cloned()).map_err(|e| {
            WireError::BatchItemInvalid {
                index,
                reason: e.to_string(),
            }
        })?;

        results.push((slug, normalised, meta));
    }

    Ok(results)
}
