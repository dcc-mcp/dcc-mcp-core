//! Response-format negotiation and token accounting for agent-facing REST.
//!
//! Compact TOON is the default for gateway REST responses. Legacy JSON remains
//! an explicit compatibility path via request bodies, `Accept: application/json`,
//! or `DCC_MCP_GATEWAY_RESPONSE_FORMAT=json`.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Map, Value, json};

pub(crate) const TOON_MIME: &str = "application/toon";
pub(crate) const JSON_MIME: &str = "application/json";
pub(crate) const TOKEN_ESTIMATOR: &str = "dcc-mcp-byte4-v1";
pub(crate) const DEFAULT_RESPONSE_FORMAT_ENV: &str = "DCC_MCP_GATEWAY_RESPONSE_FORMAT";
pub(crate) const LEGACY_RESPONSE_FORMAT_ENV: &str = "DCC_MCP_RESPONSE_FORMAT";

pub(crate) const HEADER_RESPONSE_FORMAT: &str = "x-dcc-mcp-response-format";
pub(crate) const HEADER_TOKEN_ESTIMATOR: &str = "x-dcc-mcp-token-estimator";
pub(crate) const HEADER_ORIGINAL_BYTES: &str = "x-dcc-mcp-original-bytes";
pub(crate) const HEADER_RETURNED_BYTES: &str = "x-dcc-mcp-returned-bytes";
pub(crate) const HEADER_ORIGINAL_TOKENS: &str = "x-dcc-mcp-original-tokens";
pub(crate) const HEADER_RETURNED_TOKENS: &str = "x-dcc-mcp-returned-tokens";
pub(crate) const HEADER_SAVED_TOKENS: &str = "x-dcc-mcp-saved-tokens";
pub(crate) const HEADER_SAVINGS_PERCENT: &str = "x-dcc-mcp-savings-pct";
pub(crate) const HEADER_REQUEST_ID: &str = "x-dcc-mcp-request-id";
pub(crate) const HEADER_TRACE_ID: &str = "x-dcc-mcp-trace-id";
pub(crate) const HEADER_INDEX_GENERATION: &str = "x-dcc-mcp-index-generation";
pub(crate) const HEADER_SEARCH_ID: &str = "x-dcc-mcp-search-id";
pub(crate) const HEADER_RANKER_VERSION: &str = "x-dcc-mcp-ranker-version";

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

pub(crate) const DEFAULT_REST_RESPONSE_FORMAT: ResponseFormat = ResponseFormat::Toon;

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

    #[must_use]
    pub(crate) fn to_json(self, format: ResponseFormat) -> Value {
        json!({
            "response_format": format.as_str(),
            "token_estimator": TOKEN_ESTIMATOR,
            "original_bytes": self.original_bytes,
            "returned_bytes": self.returned_bytes,
            "original_tokens": self.original_tokens,
            "returned_tokens": self.returned_tokens,
            "saved_tokens": self.saved_tokens,
            "savings_pct": format!("{:.2}", self.savings_percent()),
        })
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
    negotiate_response_format_with_default(headers, body, default_rest_response_format())
}

pub(crate) fn default_rest_response_format() -> ResponseFormat {
    std::env::var(DEFAULT_RESPONSE_FORMAT_ENV)
        .ok()
        .or_else(|| std::env::var(LEGACY_RESPONSE_FORMAT_ENV).ok())
        .and_then(|value| response_format_from_str(&value))
        .unwrap_or(DEFAULT_REST_RESPONSE_FORMAT)
}

pub(crate) fn negotiate_response_format_with_default(
    headers: &HeaderMap,
    body: &Value,
    default: ResponseFormat,
) -> ResponseFormat {
    if let Some(format) = explicit_format(body) {
        return format;
    }
    if let Some(format) = accept_format(headers) {
        return format;
    }
    default
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

pub(crate) fn token_telemetry_for_response(
    legacy_json: &Value,
    compact_json: Option<&Value>,
    format: ResponseFormat,
) -> Option<crate::gateway::admin::trace::TokenTelemetry> {
    encode_response(legacy_json, compact_json, format)
        .ok()
        .map(|encoded| {
            crate::gateway::admin::trace::TokenTelemetry::from_accounting(
                encoded.format,
                encoded.accounting,
            )
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

pub(crate) fn negotiated_response(
    headers: &HeaderMap,
    body: &Value,
    status: StatusCode,
    legacy_json: Value,
    compact_json: Option<Value>,
) -> Response {
    match encode_response(
        &legacy_json,
        compact_json.as_ref(),
        negotiate_response_format(headers, body),
    ) {
        Ok(encoded) => encoded_response(status, encoded),
        Err(err) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    }
}

#[cfg(feature = "admin")]
pub(crate) fn negotiated_response_with_default(
    headers: &HeaderMap,
    body: &Value,
    status: StatusCode,
    legacy_json: Value,
    compact_json: Option<Value>,
    default: ResponseFormat,
) -> Response {
    match encode_response(
        &legacy_json,
        compact_json.as_ref(),
        negotiate_response_format_with_default(headers, body, default),
    ) {
        Ok(encoded) => encoded_response(status, encoded),
        Err(err) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    }
}

#[cfg(test)]
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
    negotiated_response(headers, body, StatusCode::OK, legacy, Some(compact))
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

pub(crate) fn compact_describe_payload(legacy: &Value) -> Value {
    let mut out = Map::new();
    copy_field(&mut out, legacy, "request_id");
    copy_field(&mut out, legacy, "trace_id");
    copy_field(&mut out, legacy, "index_generation");
    copy_field(&mut out, legacy, "next_step");
    if let Some(record) = legacy.get("record") {
        out.insert("record".to_string(), compact_record(record));
    }
    if let Some(tool) = legacy.get("tool") {
        // The schema-bearing tool definition is the describe contract.
        // Preserve it verbatim so compact output never hides validation hints.
        out.insert("tool".to_string(), tool.clone());
    }
    Value::Object(out)
}

pub(crate) fn compact_call_batch_payload(legacy: &Value) -> Value {
    let mut compact = legacy.clone();
    if let Some(results) = compact.get_mut("results").and_then(Value::as_array_mut) {
        for item in results {
            let item_payload = item.clone();
            if let Some(accounting) = token_accounting_for_compact_value(&item_payload)
                && let Some(obj) = item.as_object_mut()
            {
                obj.insert("token_accounting".to_string(), accounting);
            }
        }
    }
    compact
}

fn compact_search_hit(hit: &Value) -> Value {
    compact_record(hit)
}

fn compact_record(record: &Value) -> Value {
    let mut out = Map::new();
    copy_field(&mut out, record, "tool_slug");
    copy_field(&mut out, record, "backend_tool");
    copy_callable_if_distinct(&mut out, record);
    copy_field(&mut out, record, "skill_name");
    copy_field(&mut out, record, "summary");
    copy_field(&mut out, record, "tags");
    copy_field(&mut out, record, "dcc_type");
    copy_field(&mut out, record, "instance_id");
    copy_field(&mut out, record, "has_schema");
    copy_field(&mut out, record, "loaded");
    copy_field(&mut out, record, "load_state");
    copy_field(&mut out, record, "callable");
    copy_field(&mut out, record, "disabled_by_group");
    copy_field(&mut out, record, "available_groups");
    copy_field(&mut out, record, "rank");
    copy_field(&mut out, record, "score");
    copy_field(&mut out, record, "match_reasons");
    copy_field(&mut out, record, "annotations");
    copy_field(&mut out, record, "metadata");
    copy_field(&mut out, record, "next_step");
    Value::Object(out)
}

fn token_accounting_for_compact_value(value: &Value) -> Option<Value> {
    encode_response(value, Some(value), ResponseFormat::Toon)
        .ok()
        .map(|encoded| encoded.accounting.to_json(encoded.format))
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

pub(crate) fn explicit_format(body: &Value) -> Option<ResponseFormat> {
    let raw = body
        .get("response_format")
        .or_else(|| body.get("responseFormat"))
        .or_else(|| body.get("format"))
        .or_else(|| body.get("output_format"));
    if let Some(format) = raw
        .and_then(Value::as_str)
        .and_then(response_format_from_str)
    {
        Some(format)
    } else if body.get("compact").and_then(Value::as_bool) == Some(true) {
        Some(ResponseFormat::Toon)
    } else {
        None
    }
}

fn response_format_from_str(value: &str) -> Option<ResponseFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "json" | "legacy-json" | "legacy_json" | "application/json" => Some(ResponseFormat::Json),
        "toon" | "compact" | "application/toon" | "application/x-toon" | "text/toon" => {
            Some(ResponseFormat::Toon)
        }
        _ => None,
    }
}

fn accept_format(headers: &HeaderMap) -> Option<ResponseFormat> {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())?;
    for part in accept.split(',') {
        let mime = part.trim().split(';').next().unwrap_or("").trim();
        if let Some(format) = response_format_from_str(mime) {
            return Some(format);
        }
    }
    None
}

pub(crate) fn estimate_tokens(body: &[u8]) -> usize {
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
                "match_reasons": ["tool_lexical", "summary_fuzzy"],
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
    fn negotiate_defaults_to_compact_toon() {
        assert_eq!(
            negotiate_response_format_with_default(
                &HeaderMap::new(),
                &json!({}),
                DEFAULT_REST_RESPONSE_FORMAT,
            ),
            ResponseFormat::Toon
        );
    }

    #[test]
    fn accept_json_overrides_compact_default() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(JSON_MIME));

        assert_eq!(
            negotiate_response_format_with_default(
                &headers,
                &json!({}),
                DEFAULT_REST_RESPONSE_FORMAT,
            ),
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
    async fn search_response_returns_compact_toon_by_default() {
        let hits = representative_search_hits();

        let (status, headers, bytes) =
            response_bytes(search_response(&HeaderMap::new(), &json!({}), hits)).await;
        let text = String::from_utf8(bytes).unwrap();
        let body: Value = toon_format::decode_default(&text).unwrap();

        assert_eq!(status, StatusCode::OK);
        assert!(
            headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with(TOON_MIME))
        );
        assert_eq!(
            headers
                .get(HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("toon")
        );
        assert_eq!(body["total"], 2);
        assert!(body["hits"][0].get("callable_id").is_none());
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
        assert_eq!(
            body["hits"][0]["match_reasons"],
            json!(["tool_lexical", "summary_fuzzy"])
        );
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

    #[test]
    fn compact_describe_payload_preserves_schema_and_hints() {
        let legacy = json!({
            "record": {
                "tool_slug": "maya.abcdef01.export_fbx",
                "backend_tool": "export_fbx",
                "callable_id": "export_fbx",
                "skill_name": "maya-export",
                "summary": "Export selected Maya objects to FBX.",
                "tags": [],
                "dcc_type": "maya",
                "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
                "has_schema": true,
                "loaded": true,
                "annotations": {"readOnlyHint": false},
                "metadata": {"affinity": "main"}
            },
            "tool": {
                "name": "export_fbx",
                "description": "Export selected objects.",
                "inputSchema": {
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": {"type": "string"},
                        "selected_only": {"type": "boolean"}
                    }
                },
                "annotations": {"destructiveHint": false}
            }
        });

        let compact = compact_describe_payload(&legacy);

        assert!(compact["record"].get("callable_id").is_none());
        assert!(compact["record"].get("tags").is_none());
        assert_eq!(compact["record"]["metadata"]["affinity"], "main");
        assert_eq!(
            compact["tool"]["inputSchema"]["properties"]["path"]["type"],
            "string"
        );
        assert_eq!(compact["tool"]["inputSchema"]["required"][0], "path");
        assert_eq!(compact["tool"]["annotations"]["destructiveHint"], false);
    }

    #[test]
    fn compact_call_batch_payload_adds_per_item_token_accounting() {
        let legacy = json!({
            "success": false,
            "stop_on_error": false,
            "results": [
                {
                    "index": 0,
                    "tool_slug": "maya.abcdef01.big_result",
                    "ok": true,
                    "result": {
                        "output": {
                            "success": true,
                            "context": {
                                "objects": [
                                    {"name": "cube_a", "type": "mesh"},
                                    {"name": "cube_b", "type": "mesh"}
                                ]
                            }
                        }
                    }
                },
                {
                    "index": 1,
                    "tool_slug": "photoshop.12345678.select_layer",
                    "ok": false,
                    "error": {
                        "success": false,
                        "error": {
                            "kind": "backend-error",
                            "message": "Layer not found"
                        }
                    }
                }
            ]
        });

        let compact = compact_call_batch_payload(&legacy);

        assert_eq!(compact["success"], false);
        assert_eq!(compact["results"][0]["ok"], true);
        assert_eq!(
            compact["results"][1]["error"]["error"]["kind"],
            "backend-error"
        );
        for item in compact["results"].as_array().unwrap() {
            assert_eq!(item["token_accounting"]["response_format"], "toon");
            assert_eq!(item["token_accounting"]["token_estimator"], TOKEN_ESTIMATOR);
            assert!(item["token_accounting"]["original_tokens"].is_number());
            assert!(item["token_accounting"]["returned_tokens"].is_number());
            assert!(item["token_accounting"]["saved_tokens"].is_number());
        }
    }

    #[tokio::test]
    async fn negotiated_response_returns_compact_error_envelope() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(TOON_MIME));
        let legacy = json!({
            "success": false,
            "error": {
                "kind": "unknown-slug",
                "message": "No capability matched",
                "candidates": [
                    {"tool_slug": "maya.abcdef01.render"}
                ]
            }
        });

        let (status, response_headers, bytes) = response_bytes(negotiated_response(
            &headers,
            &json!({}),
            StatusCode::NOT_FOUND,
            legacy,
            None,
        ))
        .await;
        let text = String::from_utf8(bytes).unwrap();
        let body: Value = toon_format::decode_default(&text).unwrap();

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            response_headers
                .get(HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("toon")
        );
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["kind"], "unknown-slug");
        assert_eq!(body["error"]["message"], "No capability matched");
        assert_eq!(
            body["error"]["candidates"][0]["tool_slug"],
            "maya.abcdef01.render"
        );
    }
}
