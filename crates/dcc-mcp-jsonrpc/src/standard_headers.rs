//! Shared SEP-2243 standard header values; schema-driven Mcp-Param is separate.

use crate::{JsonRpcRequest, JsonRpcResponse, ProtocolRequestHints};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

pub(crate) fn strip_ows(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

pub fn decode_mcp_header_value(value: &str) -> Option<String> {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|v| v.strip_suffix("?="))
    {
        let bytes = STANDARD.decode(encoded).ok()?;
        // Require canonical padding/unused bits, not a forgiving decoder.
        if STANDARD.encode(&bytes) != encoded {
            return None;
        }
        String::from_utf8(bytes).ok()
    } else {
        Some(value.into())
    }
}

pub fn encode_mcp_header_value(value: &str) -> String {
    let plain = !value.is_empty()
        && value.trim() == value
        && !(value.starts_with("=?base64?") && value.ends_with("?="))
        && value
            .bytes()
            .all(|b| b == b'\t' || (0x20..=0x7e).contains(&b));
    if plain {
        value.into()
    } else {
        format!("=?base64?{}?=", STANDARD.encode(value))
    }
}

/// The standard name table is shared by validation and opt-in producers.
/// Task rows describe header syntax, not support for those removed methods.
pub fn mcp_name_source<'a>(
    method: &str,
    params: Option<&'a Value>,
) -> Option<(&'static str, Option<&'a str>)> {
    let field = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" => "uri",
        "tasks/get" | "tasks/update" | "tasks/cancel" => "taskId",
        _ => return None,
    };
    Some((
        field,
        params.and_then(|p| p.get(field)).and_then(Value::as_str),
    ))
}

pub(crate) fn validate_standard_headers(
    headers: ProtocolRequestHints<'_>,
    request: &JsonRpcRequest,
) -> Result<(), Box<JsonRpcResponse>> {
    // Notification POST header presence is not specified by this revision.
    if request.id.is_none() {
        return Ok(());
    }
    for (value, name) in [
        (headers.protocol_version, "MCP-Protocol-Version"),
        (headers.method, "Mcp-Method"),
    ] {
        if value.is_none() {
            return Err(Box::new(JsonRpcResponse::header_mismatch(
                request.id.clone(),
                "(missing)",
                &format!("Required {name} header is absent"),
            )));
        }
    }
    let Some((field, expected)) = mcp_name_source(&request.method, request.params.as_ref()) else {
        return Ok(());
    };
    let Some(value) = headers.name else {
        return if expected.is_some() {
            Err(Box::new(JsonRpcResponse::header_mismatch(
                request.id.clone(),
                "(missing)",
                &format!("Mcp-Name must mirror params.{field}"),
            )))
        } else {
            Ok(())
        };
    };
    let value = strip_ows(value);
    let decoded = decode_mcp_header_value(value).ok_or_else(|| {
        JsonRpcResponse::header_mismatch(
            request.id.clone(),
            value,
            "Mcp-Name has invalid Base64 or UTF-8 encoding",
        )
    })?;
    if expected.is_some_and(|expected| expected != decoded) {
        return Err(Box::new(JsonRpcResponse::header_mismatch(
            request.id.clone(),
            value,
            &format!("Mcp-Name must match params.{field}"),
        )));
    }
    Ok(())
}
