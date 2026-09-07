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

fn contains_claim(body: &Value) -> bool {
    body.as_array().map_or_else(
        || has_modern_envelope_claim(body),
        |rows| rows.iter().any(contains_claim),
    )
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
    if body.is_array() {
        return if contains_claim(body) {
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
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !body.get("method").is_some_and(Value::is_string)
        || body.get("id").is_some_and(|v| !valid_id(v))
        || body.get("params").is_some_and(|v| !v.is_object())
    {
        return Err(JsonRpcResponse::invalid_request());
    }
    let request: JsonRpcRequest =
        serde_json::from_value(body.clone()).map_err(|_| JsonRpcResponse::invalid_request())?;
    let raw_meta = request.params.as_ref().and_then(|p| p.get("_meta"));
    let parsed_meta = StatelessRequestMeta::parse(raw_meta);
    // Preserve the SDK's legacy initialize exception; a VALID modern claim
    // instead reaches the modern registry and returns method-not-found.
    if request.method == "initialize"
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
