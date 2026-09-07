//! Schema-driven SEP-2243 parameter headers, independent of HTTP frameworks.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{decode_mcp_header_value, encode_mcp_header_value};

/// Primitive types permitted by the final MCP 2026-07-28 specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpParamType {
    String,
    Integer,
    Boolean,
}

/// One statically reachable argument and its case-preserved header suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpParamDeclaration {
    pub path: Vec<String>,
    pub header_name: String,
    pub value_type: McpParamType,
}

/// Invalid server-owned schema metadata, without argument or header values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpParamSchemaError {
    pub path: Vec<String>,
    pub reason: &'static str,
}

/// Scan the source schema before any client-compatibility projection.
pub fn scan_mcp_param_headers(
    schema: &Value,
) -> Result<Vec<McpParamDeclaration>, McpParamSchemaError> {
    let mut declarations = Vec::new();
    let mut names = HashSet::new();
    let mut pending = vec![(schema, Vec::new(), true)];
    while let Some((node, path, reachable)) = pending.pop() {
        if let Some(annotation) = node.get("x-mcp-header") {
            let invalid = |reason| McpParamSchemaError {
                path: path.clone(),
                reason,
            };
            if !reachable || path.is_empty() {
                return Err(invalid(
                    "annotation must be reached through properties only",
                ));
            }
            let header = annotation
                .as_str()
                .filter(|name| valid_suffix(name))
                .ok_or_else(|| invalid("header suffix must be a nonempty ASCII token"))?;
            let value_type = match node.get("type").and_then(Value::as_str) {
                Some("string") => McpParamType::String,
                Some("integer") => McpParamType::Integer,
                Some("boolean") => McpParamType::Boolean,
                _ => {
                    return Err(invalid(
                        "annotated type must be string, integer, or boolean",
                    ));
                }
            };
            if !names.insert(header.to_ascii_lowercase()) {
                return Err(invalid("header suffixes must be case-insensitively unique"));
            }
            declarations.push(McpParamDeclaration {
                path: path.clone(),
                header_name: header.to_string(),
                value_type,
            });
        }
        if let Some(properties) = node.get("properties").and_then(Value::as_object) {
            for (name, child) in properties.iter().rev() {
                let mut child_path = path.clone();
                child_path.push(name.clone());
                pending.push((child, child_path, reachable));
            }
        }
        // Sweep schema-bearing keywords, including unused definitions: skipping
        // them would launder invalid annotations during compatibility projection.
        for keyword in [
            "patternProperties",
            "dependentSchemas",
            "dependencies",
            "$defs",
            "definitions",
        ] {
            if let Some(children) = node.get(keyword).and_then(Value::as_object) {
                for child in children.values() {
                    pending.push((child, path.clone(), false));
                }
            }
        }
        for keyword in [
            "items",
            "additionalItems",
            "contentSchema",
            "prefixItems",
            "contains",
            "additionalProperties",
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "oneOf",
            "anyOf",
            "allOf",
            "not",
            "if",
            "then",
            "else",
        ] {
            if let Some(child) = node.get(keyword) {
                if let Some(children) = child.as_array() {
                    for child in children {
                        pending.push((child, path.clone(), false));
                    }
                } else {
                    pending.push((child, path.clone(), false));
                }
            }
        }
    }
    Ok(declarations)
}

/// Failures contain schema paths or fixed reasons, never argument/header values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpParamValidationError {
    InvalidArgument {
        path: Vec<String>,
        reason: &'static str,
    },
    HeaderMismatch {
        reason: &'static str,
    },
}

/// Produce HTTP-safe mirrors without modifying the authoritative arguments.
/// Declarations must come from [`scan_mcp_param_headers`] or satisfy its same
/// suffix/type/uniqueness contract; this function does not re-scan a schema.
pub fn build_mcp_param_headers(
    declarations: &[McpParamDeclaration],
    arguments: &Value,
) -> Result<Vec<(String, String)>, McpParamValidationError> {
    Ok(parameter_values(declarations, arguments)?
        .into_iter()
        .map(|(declaration, value)| {
            (
                format!("Mcp-Param-{}", declaration.header_name),
                encode_mcp_header_value(&value),
            )
        })
        .collect())
}

/// Validate the HTTP mirrors before any tool side effects.
/// Declarations obey [`scan_mcp_param_headers`]'s contract. The callback must
/// use case-insensitive names and normalize HTTP field OWS before decoding,
/// joining duplicates rather than selecting one. Decoded payloads are not trimmed.
pub fn validate_mcp_param_headers(
    declarations: &[McpParamDeclaration],
    arguments: &Value,
    header: impl Fn(&str) -> Option<String>,
) -> Result<(), McpParamValidationError> {
    // Validate arguments first so an invalid body never becomes a misleading
    // header mismatch (and unsafe numbers never silently skip enforcement).
    let values = parameter_values(declarations, arguments)?;
    let mismatch = |reason| McpParamValidationError::HeaderMismatch { reason };
    let mut decoded_headers = HashMap::new();
    for declaration in declarations {
        if let Some(encoded) = header(&format!("Mcp-Param-{}", declaration.header_name)) {
            // A supplied recognized header must be valid even if its body
            // argument is absent/null. Unknown headers are never inspected.
            if !encoded
                .bytes()
                .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
            {
                return Err(mismatch("parameter header contains invalid characters"));
            }
            let decoded = decode_mcp_header_value(&encoded)
                .ok_or_else(|| mismatch("parameter header encoding is invalid"))?;
            decoded_headers.insert(declaration.header_name.as_str(), decoded);
        }
    }
    for (declaration, expected) in values {
        let decoded = decoded_headers
            .get(declaration.header_name.as_str())
            .ok_or_else(|| mismatch("required parameter header is absent"))?;
        let matches = if declaration.value_type == McpParamType::Integer {
            normalize_integer_decimal(decoded).is_some_and(|value| value == expected)
        } else {
            decoded == &expected
        };
        if !matches {
            return Err(mismatch("parameter header does not match its argument"));
        }
    }
    Ok(())
}

fn parameter_values<'a>(
    declarations: &'a [McpParamDeclaration],
    arguments: &Value,
) -> Result<Vec<(&'a McpParamDeclaration, String)>, McpParamValidationError> {
    let mut values = Vec::new();
    for declaration in declarations {
        let value = declaration
            .path
            .iter()
            .try_fold(arguments, |node, key| node.as_object()?.get(key));
        let Some(value) = value.filter(|value| !value.is_null()) else {
            continue;
        };
        let primitive = match declaration.value_type {
            McpParamType::String => value.as_str().map(str::to_string),
            McpParamType::Boolean => value.as_bool().map(|value| value.to_string()),
            McpParamType::Integer => safe_integer(value).map(|value| value.to_string()),
        }
        .ok_or_else(|| McpParamValidationError::InvalidArgument {
            path: declaration.path.clone(),
            reason: "annotated argument must match its primitive type and safe integer range",
        })?;
        values.push((declaration, primitive));
    }
    Ok(values)
}

fn safe_integer(value: &Value) -> Option<i64> {
    const MAX_SAFE: i64 = 9_007_199_254_740_991;
    if let Some(integer) = value.as_i64() {
        return (-MAX_SAFE..=MAX_SAFE).contains(&integer).then_some(integer);
    }
    // JSON Schema integer includes e.g. 42.0. This operates on the parsed JSON
    // value, like JS clients; header decimals below never pass through f64.
    let number = value.as_f64()?;
    (number.is_finite() && number.fract() == 0.0 && number.abs() <= MAX_SAFE as f64)
        .then_some(number as i64)
}

fn normalize_integer_decimal(value: &str) -> Option<String> {
    let (negative, unsigned) = match value.strip_prefix('-') {
        Some(unsigned) => (true, unsigned),
        None => (false, value),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, "0"));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_empty()
        || !fraction.bytes().all(|byte| byte == b'0')
    {
        return None;
    }
    let digits = whole.trim_start_matches('0');
    if digits.is_empty() {
        return Some("0".into());
    }
    Some(format!("{}{digits}", if negative { "-" } else { "" }))
}

fn valid_suffix(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}
