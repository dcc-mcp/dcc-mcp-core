//! Body-primary modern ingress without changing legacy envelopes or methods.
//!
//! Rejections are pre-dispatch HTTP 400 errors. Business errors remain owned
//! by the handler. The pinned SDK oracle is documented with the boundary tests.

use crate::standard_headers::{strip_ows, validate_standard_headers};
use crate::{
    JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION_2026, PROTOCOL_VERSION_META_KEY,
    ProtocolRequestHints, StatelessRequestMeta, error_codes, has_modern_envelope_claim,
};
use serde_json::{Value, json};

#[derive(Debug)]
pub enum InboundRoute {
    Legacy,
    Modern(JsonRpcRequest),
}

fn is_modern(version: &str) -> bool {
    version >= MCP_PROTOCOL_VERSION_2026
}

fn valid_id(value: &Value) -> bool {
    value.is_string() || crate::envelope_validation::is_safe_integer(value)
}

fn only_keys(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
}

fn valid_params(params: &Value) -> bool {
    params.is_object()
        && params.get("_meta").is_none_or(|meta| {
            meta.is_object()
                && meta.get("progressToken").is_none_or(valid_id)
                && meta
                    .get("io.modelcontextprotocol/related-task")
                    .is_none_or(|task| {
                        task.is_object() && task.get("taskId").is_some_and(Value::is_string)
                    })
        })
}

fn valid_request_message(body: &Value) -> bool {
    only_keys(body, &["jsonrpc", "id", "method", "params"])
        && body.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && body.get("method").is_some_and(Value::is_string)
        && body.get("id").is_none_or(valid_id)
        && body.get("params").is_none_or(valid_params)
}

fn valid_legacy_message(body: &Value) -> bool {
    if valid_request_message(body) {
        return true;
    }
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return false;
    }
    if let Some(result) = body.get("result") {
        return only_keys(body, &["jsonrpc", "id", "result"])
            && body.get("id").is_some_and(valid_id)
            && result.is_object()
            && result.get("_meta").is_none_or(Value::is_object);
    }
    if let Some(error) = body.get("error") {
        return only_keys(body, &["jsonrpc", "id", "error"])
            && body.get("id").is_none_or(valid_id)
            && error.is_object()
            && error
                .get("code")
                .is_some_and(crate::envelope_validation::is_safe_integer)
            && error.get("message").is_some_and(Value::is_string);
    }
    false
}

fn envelope_error(id: Option<Value>, issue: crate::EnvelopeIssue) -> JsonRpcResponse {
    JsonRpcResponse::error_with_data(
        id,
        error_codes::INVALID_PARAMS,
        format!("Invalid request envelope: {}: {}", issue.key, issue.problem),
        Some(json!({"envelope": issue})),
    )
}

/// Classify once from parsed JSON. `supported` is the modern-only version set.
/// Header hints never silently upgrade or downgrade a body claim. Legacy
/// routing means the caller must forward the ORIGINAL bytes and headers.
pub fn classify_protocol_request(
    http_method: &str,
    headers: ProtocolRequestHints<'_>,
    body: &Value,
    supported: &[&str],
) -> Result<InboundRoute, JsonRpcResponse> {
    if http_method != "POST" {
        return Ok(InboundRoute::Legacy);
    }
    let headers = ProtocolRequestHints {
        protocol_version: headers.protocol_version.map(strip_ows),
        method: headers.method.map(strip_ows),
        name: headers.name.map(strip_ows),
        ..headers
    };
    if let Some(rows) = body.as_array() {
        return if rows.is_empty()
            || rows
                .iter()
                .any(|row| has_modern_envelope_claim(row) || !valid_legacy_message(row))
        {
            Err(JsonRpcResponse::invalid_request())
        } else {
            Ok(InboundRoute::Legacy)
        };
    }
    let claim = has_modern_envelope_claim(body);
    let modern_header = headers.protocol_version.is_some_and(is_modern);
    // A posted response belongs to the legacy return channel, not modern RPC.
    if !claim
        && body.get("method").is_none()
        && (body.get("result").is_some() || body.get("error").is_some())
    {
        return Ok(InboundRoute::Legacy);
    }
    if !claim && !modern_header {
        return Ok(InboundRoute::Legacy);
    }

    let id = body.get("id").filter(|value| valid_id(value)).cloned();
    if !valid_request_message(body) {
        return Err(JsonRpcResponse::invalid_request());
    }
    let request: JsonRpcRequest =
        serde_json::from_value(body.clone()).map_err(|_| JsonRpcResponse::invalid_request())?;
    let raw_meta = request.params.as_ref().and_then(|p| p.get("_meta"));
    let parsed_meta = StatelessRequestMeta::parse(raw_meta);
    // Preserve the SDK's legacy initialize exception; a VALID modern claim
    // instead reaches the modern registry and returns method-not-found.
    if request.id.is_some()
        && request.method == "initialize"
        && !parsed_meta
            .as_ref()
            .is_ok_and(|m| is_modern(&m.protocol_version))
    {
        return if modern_header {
            Err(JsonRpcResponse::header_mismatch(
                id,
                headers.protocol_version.unwrap_or_default(),
                "initialize is a legacy handshake",
            ))
        } else {
            Ok(InboundRoute::Legacy)
        };
    }
    let revision = if request.id.is_none() {
        // No core notification POST protocol exists here; keep accepted/drop
        // behavior without requiring request-only client capabilities.
        if claim {
            raw_meta
                .and_then(|m| m.get(PROTOCOL_VERSION_META_KEY))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    envelope_error(
                        None,
                        crate::EnvelopeIssue::new(PROTOCOL_VERSION_META_KEY, "expected a string"),
                    )
                })?
        } else {
            headers.protocol_version.unwrap_or_default()
        }
    } else {
        parsed_meta
            .as_ref()
            .map_err(|issue| envelope_error(id.clone(), issue.clone()))?
            .protocol_version
            .as_str()
    };
    if let Some(header) = headers.protocol_version {
        if header != revision {
            return Err(JsonRpcResponse::header_mismatch(
                id,
                header,
                "MCP-Protocol-Version must match the request envelope",
            ));
        }
    }
    if let Some(header) = headers.method {
        if header != request.method {
            return Err(JsonRpcResponse::header_mismatch(
                id,
                header,
                "Mcp-Method must match the request method",
            ));
        }
    }
    if !supported.contains(&revision) {
        return Err(JsonRpcResponse::unsupported_protocol_version(
            id, revision, supported,
        ));
    }
    validate_standard_headers(headers, &request)?;
    Ok(InboundRoute::Modern(request))
}
