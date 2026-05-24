//! Response-format negotiation and token accounting for agent-facing REST.
//!
//! The gateway keeps legacy JSON as the default wire contract. Compact
//! responses are explicit and reuse this codec so later REST/MCP slices can
//! add more routes without reimplementing TOON encoding or accounting.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Map, Value, json};

pub(crate) const TOON_MIME: &str = "application/toon";
pub(crate) const JSON_MIME: &str = "application/json";
pub(crate) const TOKEN_ESTIMATOR: &str = "dcc-mcp-byte4-v1";

const HEADER_RESPONSE_FORMAT: &str = "x-dcc-mcp-response-format";
const HEADER_TOKEN_ESTIMATOR: &str = "x-dcc-mcp-token-estimator";
const HEADER_ORIGINAL_BYTES: &str = "x-dcc-mcp-original-bytes";
const HEADER_RETURNED_BYTES: &str = "x-dcc-mcp-returned-bytes";
const HEADER_ORIGINAL_TOKENS: &str = "x-dcc-mcp-original-tokens";
const HEADER_RETURNED_TOKENS: &str = "x-dcc-mcp-returned-tokens";
const HEADER_SAVED_TOKENS: &str = "x-dcc-mcp-saved-tokens";
const HEADER_SAVINGS_PERCENT: &str = "x-dcc-mcp-savings-pct";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseFormat {
    Json,
    Toon,
}

impl ResponseFormat {
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toon => "toon",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Json => JSON_MIME,
            Self::Toon => "application/toon; charset=utf-8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenAccounting {
    pub(crate) original_bytes: usize,
    pub(crate) returned_bytes: usize,
    pub(crate) original_tokens: usize,
    pub(crate) returned_tokens: usize,
    pub(crate) saved_tokens: usize,
}

impl TokenAccounting {
    #[must_use]
    pub(crate) fn savings_percent(self) -> f64 {
        if self.original_tokens == 0 {
            0.0
        } else {
            (self.saved_tokens as f64 / self.original_tokens as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EncodedResponse {
    pub(crate) format: ResponseFormat,
    pub(crate) body: Vec<u8>,
    pub(crate) accounting: TokenAccounting,
}

#[derive(Debug)]
pub(crate) struct EncodeError {
    message: String,
}

impl EncodeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "success": false,
            "error": {
                "kind": "response-encoding-error",
                "message": self.message,
            }
        })
    }
}

pub(crate) fn negotiate_response_format(headers: &HeaderMap, body: &Value) -> ResponseFormat {
    if let Some(format) = explicit_format(body) {
        return format;
    }
    if header_contains(headers, header::ACCEPT.as_str(), TOON_MIME)
        || header_contains(headers, header::ACCEPT.as_str(), "application/x-toon")
        || header_contains(headers, header::ACCEPT.as_str(), "text/toon")
    {
        ResponseFormat::Toon
    } else {
        ResponseFormat::Json
    }
}

pub(crate) fn encode_response(
    legacy_json: &Value,
    compact_json: Option<&Value>,
    format: ResponseFormat,
) -> Result<EncodedResponse, EncodeError> {
    let original_body = serde_json::to_vec(legacy_json)
        .map_err(|err| EncodeError::new(format!("encode legacy JSON: {err}")))?;
    let returned_body = match format {
        ResponseFormat::Json => original_body.clone(),
        ResponseFormat::Toon => {
            let compact = compact_json.unwrap_or(legacy_json);
            toon_format::encode_default(compact)
                .map_err(|err| EncodeError::new(format!("encode TOON: {err}")))?
                .into_bytes()
        }
    };
    let original_tokens = estimate_tokens(&original_body);
    let returned_tokens = estimate_tokens(&returned_body);
    let accounting = TokenAccounting {
        original_bytes: original_body.len(),
        returned_bytes: returned_body.len(),
        original_tokens,
        returned_tokens,
        saved_tokens: original_tokens.saturating_sub(returned_tokens),
    };
    Ok(EncodedResponse {
        format,
        body: returned_body,
        accounting,
    })
}

pub(crate) fn encoded_response(status: StatusCode, encoded: EncodedResponse) -> Response {
    let format = encoded.format;
    let accounting = encoded.accounting;
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, format.content_type())
        .body(Body::from(encoded.body))
        .unwrap_or_else(|_| Response::new(Body::empty()));
    attach_accounting_headers(response.headers_mut(), format, accounting);
    response
}

pub(crate) fn search_response(headers: &HeaderMap, body: &Value, hits: Vec<Value>) -> Response {
    let total = hits.len();
    let legacy = json!({
        "total": total,
        "hits": hits,
    });
    let compact = compact_search_payload(
        total,
        legacy["hits"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
    );
    match encode_response(
        &legacy,
        Some(&compact),
        negotiate_response_format(headers, body),
    ) {
        Ok(encoded) => encoded_response(StatusCode::OK, encoded),
        Err(err) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    }
}

fn json_error_response(status: StatusCode, err: &EncodeError) -> Response {
    let body = serde_json::to_vec(&err.to_json()).unwrap_or_else(|_| {
        br#"{"success":false,"error":{"kind":"response-encoding-error"}}"#.to_vec()
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, JSON_MIME)
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub(crate) fn compact_search_payload(total: usize, hits: &[Value]) -> Value {
    let compact_hits: Vec<Value> = hits.iter().map(compact_search_hit).collect();
    json!({
        "total": total,
        "hits": compact_hits,
    })
}

fn compact_search_hit(hit: &Value) -> Value {
    let mut out = Map::new();
    copy_field(&mut out, hit, "tool_slug");
    copy_field(&mut out, hit, "backend_tool");
    copy_callable_if_distinct(&mut out, hit);
    copy_field(&mut out, hit, "skill_name");
    copy_field(&mut out, hit, "summary");
    copy_field(&mut out, hit, "tags");
    copy_field(&mut out, hit, "dcc_type");
    copy_field(&mut out, hit, "instance_id");
    copy_field(&mut out, hit, "has_schema");
    copy_field(&mut out, hit, "loaded");
    copy_field(&mut out, hit, "score");
    copy_field(&mut out, hit, "annotations");
    copy_field(&mut out, hit, "metadata");
    copy_field(&mut out, hit, "next_step");
    Value::Object(out)
}

fn copy_field(out: &mut Map<String, Value>, hit: &Value, field: &str) {
    if let Some(value) = hit.get(field)
        && !value.is_null()
        && !is_empty_array(value)
        && !is_empty_object(value)
    {
        out.insert(field.to_string(), value.clone());
    }
}

fn copy_callable_if_distinct(out: &mut Map<String, Value>, hit: &Value) {
    let callable = hit.get("callable_id");
    let backend = hit.get("backend_tool");
    if callable.is_some() && callable != backend {
        copy_field(out, hit, "callable_id");
    }
}

fn is_empty_array(value: &Value) -> bool {
    value.as_array().is_some_and(Vec::is_empty)
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

fn explicit_format(body: &Value) -> Option<ResponseFormat> {
    let raw = body
        .get("response_format")
        .or_else(|| body.get("format"))
        .or_else(|| body.get("output_format"));
    match raw
        .and_then(Value::as_str)
        .map(normalize_format_name)
        .as_deref()
    {
        Some("json") | Some("application/json") => Some(ResponseFormat::Json),
        Some("toon") | Some("compact") | Some("application/toon") | Some("text/toon") => {
            Some(ResponseFormat::Toon)
        }
        _ => {
            if body.get("compact").and_then(Value::as_bool) == Some(true) {
                Some(ResponseFormat::Toon)
            } else {
                None
            }
        }
    }
}

fn normalize_format_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn header_contains(headers: &HeaderMap, name: &str, needle: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|part| {
                part.trim()
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(needle)
            })
        })
}

fn estimate_tokens(body: &[u8]) -> usize {
    if body.is_empty() {
        0
    } else {
        body.len().div_ceil(4)
    }
}

fn attach_accounting_headers(
    headers: &mut HeaderMap,
    format: ResponseFormat,
    accounting: TokenAccounting,
) {
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    headers.insert(
        HEADER_RESPONSE_FORMAT,
        HeaderValue::from_static(format.as_str()),
    );
    headers.insert(
        HEADER_TOKEN_ESTIMATOR,
        HeaderValue::from_static(TOKEN_ESTIMATOR),
    );
    insert_usize(headers, HEADER_ORIGINAL_BYTES, accounting.original_bytes);
    insert_usize(headers, HEADER_RETURNED_BYTES, accounting.returned_bytes);
    insert_usize(headers, HEADER_ORIGINAL_TOKENS, accounting.original_tokens);
    insert_usize(headers, HEADER_RETURNED_TOKENS, accounting.returned_tokens);
    insert_usize(headers, HEADER_SAVED_TOKENS, accounting.saved_tokens);
    insert_string(
        headers,
        HEADER_SAVINGS_PERCENT,
        format!("{:.2}", accounting.savings_percent()),
    );
}

fn insert_usize(headers: &mut HeaderMap, name: &'static str, value: usize) {
    insert_string(headers, name, value.to_string());
}

fn insert_string(headers: &mut HeaderMap, name: &'static str, value: String) {
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::json;

    async fn response_bytes(resp: Response) -> (StatusCode, HeaderMap, Vec<u8>) {
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        (status, headers, bytes.to_vec())
    }

    fn representative_search_hits() -> Vec<Value> {
        vec![
            json!({
                "tool_slug": "maya.abcdef01.create_sphere",
                "backend_tool": "create_sphere",
                "callable_id": "create_sphere",
                "skill_name": "maya-geometry",
                "summary": "Create a polygon sphere in the active Maya scene.",
                "tags": ["geometry", "smoke"],
                "dcc_type": "maya",
                "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
                "has_schema": true,
                "loaded": true,
                "score": 93,
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": false
                },
                "metadata": {
                    "affinity": "main",
                    "execution": "in-process",
                    "timeoutHintSecs": 30
                }
            }),
            json!({
                "tool_slug": "photoshop.12345678.select_layer",
                "backend_tool": "select_layer",
                "callable_id": "select_layer_by_name",
                "skill_name": "photoshop-layers",
                "summary": "Select a Photoshop layer by name before applying layer operations.",
                "tags": [],
                "dcc_type": "photoshop",
                "instance_id": "12345678-1234-5678-9abc-123456789abc",
                "has_schema": true,
                "loaded": false,
                "score": 87,
                "next_step": {
                    "action": "load_skill",
                    "arguments": {
                        "skill_name": "photoshop-layers",
                        "dcc": "photoshop",
                        "dcc_type": "photoshop",
                        "instance_id": "12345678-1234-5678-9abc-123456789abc"
                    },
                    "rest": {
                        "method": "POST",
                        "path": "/v1/load_skill"
                    },
                    "mcp": {
                        "name": "load_skill"
                    }
                }
            }),
        ]
    }

    #[test]
    fn negotiate_defaults_to_json() {
        assert_eq!(
            negotiate_response_format(&HeaderMap::new(), &json!({})),
            ResponseFormat::Json
        );
    }

    #[test]
    fn explicit_json_overrides_toon_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(TOON_MIME));

        assert_eq!(
            negotiate_response_format(&headers, &json!({"response_format": "json"})),
            ResponseFormat::Json
        );
    }

    #[tokio::test]
    async fn search_response_keeps_legacy_json_by_default() {
        let hits = representative_search_hits();

        let (status, headers, bytes) =
            response_bytes(search_response(&HeaderMap::new(), &json!({}), hits)).await;
        let body: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some(JSON_MIME)
        );
        assert_eq!(
            headers
                .get(HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("json")
        );
        assert_eq!(body["total"], 2);
        assert_eq!(body["hits"][0]["callable_id"], "create_sphere");
        assert_eq!(body["hits"][1]["callable_id"], "select_layer_by_name");
    }

    #[tokio::test]
    async fn search_response_honours_explicit_json_over_toon_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(TOON_MIME));

        let (status, response_headers, bytes) = response_bytes(search_response(
            &headers,
            &json!({"response_format": "json"}),
            representative_search_hits(),
        ))
        .await;
        let body: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            response_headers
                .get(HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("json")
        );
        assert_eq!(body["hits"][0]["callable_id"], "create_sphere");
    }

    #[tokio::test]
    async fn search_response_returns_compact_toon_with_token_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(TOON_MIME));

        let (status, response_headers, bytes) = response_bytes(search_response(
            &headers,
            &json!({}),
            representative_search_hits(),
        ))
        .await;
        let text = String::from_utf8(bytes).unwrap();
        let body: Value = toon_format::decode_default(&text).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert!(
            response_headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with(TOON_MIME))
        );
        assert_eq!(
            response_headers
                .get(HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("toon")
        );
        assert_eq!(
            response_headers
                .get(HEADER_TOKEN_ESTIMATOR)
                .and_then(|value| value.to_str().ok()),
            Some(TOKEN_ESTIMATOR)
        );
        assert!(response_headers.get(HEADER_ORIGINAL_TOKENS).is_some());
        assert!(response_headers.get(HEADER_RETURNED_TOKENS).is_some());
        assert!(response_headers.get(HEADER_SAVED_TOKENS).is_some());
        assert_eq!(body["total"], 2);
        assert_eq!(body["hits"][0]["dcc_type"], "maya");
        assert_eq!(body["hits"][1]["dcc_type"], "photoshop");
        assert_eq!(body["hits"][0]["tool_slug"], "maya.abcdef01.create_sphere");
        assert_eq!(body["hits"][1]["next_step"]["action"], "load_skill");
        assert!(body["hits"][0].get("callable_id").is_none());
        assert_eq!(body["hits"][1]["callable_id"], "select_layer_by_name");
        assert!(body["hits"][1].get("tags").is_none());
    }

    #[test]
    fn toon_encoding_round_trips_representative_search_payload() {
        let payload = json!({
            "total": 2,
            "hits": [
                {
                    "tool_slug": "maya.abcdef01.create_sphere",
                    "backend_tool": "create_sphere",
                    "dcc_type": "maya",
                    "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
                    "loaded": true,
                    "has_schema": true,
                    "score": 91,
                },
                {
                    "tool_slug": "photoshop.12345678.select_layer",
                    "backend_tool": "select_layer",
                    "dcc_type": "photoshop",
                    "instance_id": "12345678-1234-5678-9abc-123456789abc",
                    "loaded": false,
                    "has_schema": false,
                    "score": 42,
                },
            ],
        });

        let encoded = encode_response(&payload, Some(&payload), ResponseFormat::Toon).unwrap();
        let text = String::from_utf8(encoded.body).unwrap();
        let decoded: Value = toon_format::decode_default(&text).unwrap();

        assert_eq!(decoded, payload);
        assert_eq!(encoded.format, ResponseFormat::Toon);
    }

    #[test]
    fn compact_search_payload_omits_redundant_callable_id() {
        let legacy = json!({
            "total": 1,
            "hits": [{
                "tool_slug": "maya.abcdef01.create_sphere",
                "backend_tool": "create_sphere",
                "callable_id": "create_sphere",
                "tags": [],
                "dcc_type": "maya",
                "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
                "loaded": true,
                "has_schema": true,
                "score": 99,
            }],
        });

        let compact = compact_search_payload(1, legacy["hits"].as_array().unwrap());

        assert!(compact["hits"][0].get("callable_id").is_none());
        assert!(compact["hits"][0].get("tags").is_none());
        assert_eq!(
            compact["hits"][0]["tool_slug"],
            "maya.abcdef01.create_sphere"
        );
    }
}
