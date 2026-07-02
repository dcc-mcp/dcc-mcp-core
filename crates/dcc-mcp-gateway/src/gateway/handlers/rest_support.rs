use super::*;

use crate::gateway::admin::trace::TraceContext;
use crate::gateway::capability_service::{ServiceError, index_generation, service_error_to_json};
use crate::gateway::response_codec::{
    compact_search_payload, negotiate_response_format, negotiated_response,
    token_telemetry_for_response,
};
use crate::gateway::search_telemetry::{RANKER_VERSION, SearchFollowupInput, SearchTelemetryHit};

#[derive(Debug, Clone)]
pub(super) struct RestResponseMetadata {
    pub(super) request_id: String,
    pub(super) trace_id: String,
    traceparent: Option<String>,
    pub(super) index_generation: Option<String>,
    search_id: Option<String>,
    ranker_version: Option<String>,
}

impl RestResponseMetadata {
    pub(super) fn from_headers(headers: &HeaderMap) -> Self {
        Self::from_trace_context(&TraceContext::from_headers(headers))
    }

    pub(super) fn from_trace_context(trace_context: &TraceContext) -> Self {
        Self {
            request_id: trace_context.request_id.clone(),
            trace_id: trace_context.trace_id.clone(),
            traceparent: trace_context.traceparent(),
            index_generation: None,
            search_id: None,
            ranker_version: None,
        }
    }

    pub(super) fn with_index_generation(mut self, generation: String) -> Self {
        if !generation.is_empty() {
            self.index_generation = Some(generation);
        }
        self
    }

    pub(super) fn with_search(mut self, search_id: String, ranker_version: String) -> Self {
        if !search_id.is_empty() {
            self.search_id = Some(search_id);
        }
        if !ranker_version.is_empty() {
            self.ranker_version = Some(ranker_version);
        }
        self
    }

    pub(super) fn insert_body_fields(&self, value: &mut Value) {
        if let Some(obj) = value.as_object_mut() {
            obj.entry("request_id".to_string())
                .or_insert_with(|| json!(self.request_id));
            obj.entry("trace_id".to_string())
                .or_insert_with(|| json!(self.trace_id));
            if let Some(generation) = self.index_generation.as_deref() {
                obj.entry("index_generation".to_string())
                    .or_insert_with(|| json!(generation));
            }
            if let Some(search_id) = self.search_id.as_deref() {
                obj.entry("search_id".to_string())
                    .or_insert_with(|| json!(search_id));
            }
            if let Some(ranker_version) = self.ranker_version.as_deref() {
                obj.entry("ranker_version".to_string())
                    .or_insert_with(|| json!(ranker_version));
            }
        }
    }

    fn attach_headers(&self, headers: &mut HeaderMap) {
        insert_header(
            headers,
            crate::gateway::response_codec::HEADER_REQUEST_ID,
            &self.request_id,
        );
        insert_header(headers, "x-request-id", &self.request_id);
        insert_header(
            headers,
            crate::gateway::response_codec::HEADER_TRACE_ID,
            &self.trace_id,
        );
        if let Some(traceparent) = self.traceparent.as_deref() {
            insert_header(headers, "traceparent", traceparent);
        }
        if let Some(generation) = self.index_generation.as_deref() {
            insert_header(
                headers,
                crate::gateway::response_codec::HEADER_INDEX_GENERATION,
                generation,
            );
        }
        if let Some(search_id) = self.search_id.as_deref() {
            insert_header(
                headers,
                crate::gateway::response_codec::HEADER_SEARCH_ID,
                search_id,
            );
        }
        if let Some(ranker_version) = self.ranker_version.as_deref() {
            insert_header(
                headers,
                crate::gateway::response_codec::HEADER_RANKER_VERSION,
                ranker_version,
            );
        }
    }
}

fn token_telemetry_with_metadata(
    headers: &HeaderMap,
    request_body: &Value,
    mut legacy_json: Value,
    compact_json: Option<Value>,
    metadata: &RestResponseMetadata,
    include_body_metadata: bool,
) -> Option<crate::gateway::admin::trace::TokenTelemetry> {
    let mut compact_json = compact_json;
    if include_body_metadata {
        metadata.insert_body_fields(&mut legacy_json);
        if let Some(compact) = compact_json.as_mut() {
            metadata.insert_body_fields(compact);
        }
    }
    token_telemetry_for_response(
        &legacy_json,
        compact_json.as_ref(),
        negotiate_response_format(headers, request_body),
    )
}

pub(super) fn record_token_accounting(
    ctx: &mut crate::gateway::middleware::CallContext,
    gs: &GatewayState,
    headers: &HeaderMap,
    request_body: &Value,
    legacy_json: Value,
    compact_json: Option<Value>,
    include_body_metadata: bool,
) {
    let metadata = RestResponseMetadata::from_trace_context(&ctx.trace_context)
        .with_index_generation(index_generation(&gs.capability_index));
    ctx.token_accounting = token_telemetry_with_metadata(
        headers,
        request_body,
        legacy_json,
        compact_json,
        &metadata,
        include_body_metadata,
    );
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(value) = axum::http::HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

pub(super) fn negotiated_response_with_metadata(
    headers: &HeaderMap,
    request_body: &Value,
    status: StatusCode,
    mut legacy_json: Value,
    compact_json: Option<Value>,
    metadata: &RestResponseMetadata,
    include_body_metadata: bool,
) -> Response {
    let mut compact_json = compact_json;
    if include_body_metadata {
        metadata.insert_body_fields(&mut legacy_json);
        if let Some(compact) = compact_json.as_mut() {
            metadata.insert_body_fields(compact);
        }
    }
    let mut response =
        negotiated_response(headers, request_body, status, legacy_json, compact_json);
    metadata.attach_headers(response.headers_mut());
    response
}

pub(super) fn search_response_with_metadata(
    headers: &HeaderMap,
    body: &Value,
    hits: Vec<Value>,
    metadata: &RestResponseMetadata,
    semantic: Option<Value>,
) -> Response {
    let total = hits.len();
    let mut legacy = json!({
        "total": total,
        "hits": hits,
    });
    if let Some(ref sem) = semantic {
        legacy["semantic"] = sem.clone();
    }
    metadata.insert_body_fields(&mut legacy);
    let mut compact = compact_search_payload(
        total,
        legacy["hits"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
    );
    if let Some(ref sem) = semantic {
        compact["semantic"] = sem.clone();
    }
    metadata.insert_body_fields(&mut compact);
    negotiated_response_with_metadata(
        headers,
        body,
        StatusCode::OK,
        legacy,
        Some(compact),
        metadata,
        false,
    )
}

pub(super) fn search_hits_for_telemetry(
    hits: &[crate::gateway::capability::SearchHit],
) -> Vec<SearchTelemetryHit> {
    hits.iter()
        .map(|hit| SearchTelemetryHit {
            tool_slug: hit.record.tool_slug.clone(),
            skill_name: hit.record.skill_name.clone(),
            dcc_type: hit.record.dcc_type.clone(),
            rank: hit.rank,
            score: hit.score,
            match_reasons: hit.match_reasons.clone(),
            loaded: hit.record.loaded,
        })
        .collect()
}

pub(super) fn record_search_followup(
    gs: &GatewayState,
    search_id: Option<&str>,
    kind: &str,
    tool_slug: Option<&str>,
    skill_name: Option<String>,
    success: bool,
    trace_context: &TraceContext,
) {
    let Some(search_id) = search_id else {
        return;
    };
    gs.search_telemetry.record_followup(SearchFollowupInput {
        search_id: search_id.to_string(),
        kind: kind.to_string(),
        tool_slug: tool_slug.map(str::to_string),
        skill_name,
        success,
        trace_context: Some(trace_context.clone()),
    });
}

pub(super) fn skill_name_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("skill_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .get("skill_names")
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find_map(Value::as_str))
                .map(str::to_string)
        })
}

pub(super) fn call_next_step(
    slug: &str,
    search_id: &str,
    metadata: &RestResponseMetadata,
) -> Value {
    let mut meta = json!({
        "search_id": search_id,
        "ranker_version": RANKER_VERSION,
    });
    if let Some(generation) = metadata.index_generation.as_deref() {
        meta["index_generation"] = json!(generation);
    }
    let arguments = json!({
        "tool_slug": slug,
        "arguments": {},
        "meta": meta,
    });
    json!({
        "action": "call",
        "arguments": arguments.clone(),
        "mcp": {
            "tool": "call",
            "arguments": arguments.clone(),
            "_meta": arguments["meta"].clone(),
        },
        "rest": {
            "method": "POST",
            "path": "/v1/call",
            "body": arguments,
        },
    })
}

pub(super) struct RestTrafficFrame<'a> {
    pub(super) path: &'a str,
    pub(super) direction: &'static str,
    pub(super) leg: &'static str,
    pub(super) status: Option<u16>,
    pub(super) body: Value,
}

pub(super) fn emit_rest_traffic_frame(
    gs: &GatewayState,
    ctx: &crate::gateway::middleware::CallContext,
    headers: &HeaderMap,
    frame: RestTrafficFrame<'_>,
) {
    gs.traffic_capture.emit_json_frame(
        crate::gateway::traffic::TrafficFrame::json(
            crate::gateway::traffic::gateway_source(
                &gs.server_name,
                &gs.server_version,
                &gs.own_host,
                gs.own_port,
            ),
            crate::gateway::traffic::correlation(
                Some(&ctx.trace_context.request_id),
                Some(&ctx.trace_context.trace_id),
                ctx.session_id.as_deref(),
            ),
            frame.direction,
            frame.leg,
            "http",
            frame.body,
        )
        .with_session_id(ctx.session_id.as_deref())
        .with_http(crate::gateway::traffic::http_post(
            frame.path,
            Some(headers),
            frame.status,
        ))
        .with_mcp(crate::gateway::traffic::mcp_message(
            if frame.direction == "inbound" {
                "request"
            } else {
                "response"
            },
            "tools/call",
            None,
        )),
    );
}

pub(super) fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    header_string(headers, "mcp-session-id")
        .or_else(|| header_string(headers, "x-session-id"))
        .or_else(|| header_string(headers, "x-dcc-mcp-session-id"))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

pub(super) fn service_error_status(err: &ServiceError) -> StatusCode {
    match err.kind.as_str() {
        "unknown-slug" => StatusCode::NOT_FOUND,
        "ambiguous" => StatusCode::CONFLICT,
        "instance-offline" => StatusCode::SERVICE_UNAVAILABLE,
        "policy-denied" => StatusCode::FORBIDDEN,
        "throttled" => StatusCode::TOO_MANY_REQUESTS,
        "host-busy" => StatusCode::SERVICE_UNAVAILABLE,
        "host-died" => StatusCode::BAD_GATEWAY,
        "backend-error" | "schema-unavailable" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_REQUEST,
    }
}

pub(super) fn service_error_response_with_metadata(
    headers: &HeaderMap,
    body: &Value,
    err: &ServiceError,
    metadata: &RestResponseMetadata,
    include_body_metadata: bool,
) -> Response {
    negotiated_response_with_metadata(
        headers,
        body,
        service_error_status(err),
        service_error_to_json(err),
        None,
        metadata,
        include_body_metadata,
    )
}
