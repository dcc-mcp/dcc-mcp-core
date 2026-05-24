use http::HeaderMap;
use serde_json::{Map, Value, json};

/// One structured traffic frame before EventBus envelope wrapping.
#[derive(Debug)]
pub struct TrafficFrame {
    pub source: Value,
    pub correlation: Value,
    pub session_id: Option<String>,
    pub direction: &'static str,
    pub leg: &'static str,
    pub transport: &'static str,
    pub http: Value,
    pub mcp: Value,
    pub body: Value,
}

impl TrafficFrame {
    #[must_use]
    pub fn json(
        source: Value,
        correlation: Value,
        direction: &'static str,
        leg: &'static str,
        transport: &'static str,
        body: Value,
    ) -> Self {
        Self {
            source,
            correlation,
            session_id: None,
            direction,
            leg,
            transport,
            http: json!({}),
            mcp: json!({}),
            body,
        }
    }

    #[must_use]
    pub fn with_session_id(mut self, session_id: Option<impl Into<String>>) -> Self {
        self.session_id = session_id.map(Into::into);
        self
    }

    #[must_use]
    pub fn with_http(mut self, http: Value) -> Self {
        self.http = http;
        self
    }

    #[must_use]
    pub fn with_mcp(mut self, mcp: Value) -> Self {
        self.mcp = mcp;
        self
    }
}

#[must_use]
pub fn gateway_source(server_name: &str, server_version: &str, host: &str, port: u16) -> Value {
    json!({
        "service": "dcc-mcp-gateway",
        "server_name": server_name,
        "server_version": server_version,
        "host": host,
        "port": port,
    })
}

#[must_use]
pub fn basic_gateway_source() -> Value {
    json!({"service": "dcc-mcp-gateway"})
}

#[must_use]
pub fn correlation(
    request_id: Option<&str>,
    trace_id: Option<&str>,
    session_id: Option<&str>,
) -> Value {
    let mut map = Map::new();
    if let Some(value) = request_id.filter(|s| !s.is_empty()) {
        map.insert("request_id".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = trace_id.filter(|s| !s.is_empty()) {
        map.insert("trace_id".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = session_id.filter(|s| !s.is_empty()) {
        map.insert("session_id".to_string(), Value::String(value.to_string()));
    }
    Value::Object(map)
}

#[must_use]
pub fn http_post(path: &str, headers: Option<&HeaderMap>, status: Option<u16>) -> Value {
    json!({
        "method": "POST",
        "url": path,
        "headers": headers.map(safe_headers).unwrap_or_else(|| json!({})),
        "status": status,
    })
}

#[must_use]
pub fn mcp_message(kind: &str, method: &str, id: Option<Value>) -> Value {
    json!({
        "kind": kind,
        "method": method,
        "id": id,
    })
}

fn safe_headers(headers: &HeaderMap) -> Value {
    let mut out = Map::new();
    for name in [
        "accept",
        "content-type",
        "mcp-session-id",
        "traceparent",
        "tracestate",
        "user-agent",
        "x-dcc-mcp-session-id",
        "x-request-id",
        "x-session-id",
    ] {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            out.insert(name.to_string(), Value::String(value.to_string()));
        }
    }
    Value::Object(out)
}
