//! Opt-in traffic capture for gateway debugging (RFC 0003 P0).
//!
//! Capture is disabled unless `DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path>` is set.
//! Frames are emitted on the shared `traffic.frame` event name and the quick
//! mode JSONL sink appends the structured EventBus envelope to disk.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use dcc_mcp_actions::EventBus;
use dcc_mcp_actions::events::EventEnvelope;
use http::HeaderMap;
use serde_json::{Map, Value, json};

pub const TRAFFIC_FRAME_EVENT: &str = "traffic.frame";
pub const TRAFFIC_FRAME_SCHEMA_VERSION: u32 = 1;
const ENV_CAPTURE: &str = "DCC_MCP_TRAFFIC_CAPTURE";
const ENV_PROD_PROFILE: &str = "DCC_MCP_PROD_PROFILE";
const ENV_FORCE_CAPTURE: &str = "DCC_MCP_FORCE_TRAFFIC_CAPTURE";

/// Gateway traffic capture bus plus optional sinks.
#[derive(Debug)]
pub struct TrafficCapture {
    event_bus: EventBus,
    jsonl_sink: Option<Arc<JsonlTrafficSink>>,
    next_capture_id: AtomicU64,
}

impl Default for TrafficCapture {
    fn default() -> Self {
        Self::disabled()
    }
}

impl TrafficCapture {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            event_bus: EventBus::new(),
            jsonl_sink: None,
            next_capture_id: AtomicU64::new(0),
        }
    }

    pub fn from_env() -> Result<Self, TrafficCaptureError> {
        let spec = match std::env::var(ENV_CAPTURE) {
            Ok(raw) if capture_enabled(&raw) => raw,
            _ => return Ok(Self::disabled()),
        };

        if truthy_env(ENV_PROD_PROFILE) && !truthy_env(ENV_FORCE_CAPTURE) {
            return Err(TrafficCaptureError::ProdProfileBlocked);
        }

        let Some(path) = spec.strip_prefix("jsonl:").map(str::trim) else {
            return Err(TrafficCaptureError::UnsupportedSpec(spec));
        };
        if path.is_empty() {
            return Err(TrafficCaptureError::EmptyPath);
        }
        Self::with_jsonl_sink(path)
    }

    pub fn with_jsonl_sink(path: impl AsRef<Path>) -> Result<Self, TrafficCaptureError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self {
            event_bus: EventBus::new(),
            jsonl_sink: Some(Arc::new(JsonlTrafficSink::open(path)?)),
            next_capture_id: AtomicU64::new(0),
        })
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.jsonl_sink.is_some() || self.event_bus.has_subscribers(TRAFFIC_FRAME_EVENT)
    }

    pub fn emit_json_frame(&self, frame: TrafficFrame) -> Option<EventEnvelope> {
        if !self.is_enabled() {
            return None;
        }

        let body_size = serialized_size(&frame.body);
        let attributes = json!({
            "schema_version": TRAFFIC_FRAME_SCHEMA_VERSION,
            "capture_id": self.next_frame_id(),
            "session_id": frame.session_id,
            "direction": frame.direction,
            "leg": frame.leg,
            "transport": frame.transport,
            "http": frame.http,
            "mcp": frame.mcp,
            "body": {
                "encoding": "json",
                "data": frame.body,
                "size_bytes": body_size,
                "redacted_paths": [],
            },
        });

        let event = self.event_bus.emit(
            TRAFFIC_FRAME_EVENT,
            frame.source,
            frame.correlation,
            attributes,
        );
        if let Some(sink) = &self.jsonl_sink {
            sink.record(&event);
        }
        Some(event)
    }

    fn next_frame_id(&self) -> String {
        let seq = self.next_capture_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("cap_{seq:016x}")
    }
}

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

#[derive(Debug, thiserror::Error)]
pub enum TrafficCaptureError {
    #[error(
        "traffic capture is blocked when DCC_MCP_PROD_PROFILE is enabled; set DCC_MCP_FORCE_TRAFFIC_CAPTURE=1 to override"
    )]
    ProdProfileBlocked,
    #[error("unsupported DCC_MCP_TRAFFIC_CAPTURE value: {0}; expected jsonl:<path>")]
    UnsupportedSpec(String),
    #[error("DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path> requires a non-empty path")]
    EmptyPath,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct JsonlTrafficSink {
    file: Mutex<File>,
}

impl JsonlTrafficSink {
    fn open(path: &Path) -> Result<Self, std::io::Error> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    fn record(&self, event: &EventEnvelope) {
        let Ok(mut line) = serde_json::to_vec(&event.to_value()) else {
            tracing::warn!("traffic capture: failed to encode EventBus envelope");
            return;
        };
        line.push(b'\n');

        let Ok(mut file) = self.file.lock() else {
            tracing::warn!("traffic capture: JSONL sink lock poisoned");
            return;
        };
        if let Err(err) = file.write_all(&line).and_then(|_| file.flush()) {
            tracing::warn!(error = %err, "traffic capture: failed to write JSONL frame");
        }
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

fn serialized_size(value: &Value) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

fn capture_enabled(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && !matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "none"
        )
}

fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn jsonl_sink_writes_traffic_frame_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let path: PathBuf = dir.path().join("capture.jsonl");
        let capture = TrafficCapture::with_jsonl_sink(&path).unwrap();

        capture.emit_json_frame(
            TrafficFrame::json(
                basic_gateway_source(),
                correlation(Some("req-1"), Some("trace-1"), Some("sess-1")),
                "inbound",
                "client_to_gateway",
                "http",
                json!({"hello": "world"}),
            )
            .with_session_id(Some("sess-1"))
            .with_http(http_post("/mcp", None, Some(200)))
            .with_mcp(mcp_message("request", "tools/call", Some(json!(1)))),
        );

        let raw = std::fs::read_to_string(path).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 1);
        let value: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["name"], TRAFFIC_FRAME_EVENT);
        assert_eq!(value["correlation"]["request_id"], "req-1");
        assert_eq!(value["attributes"]["schema_version"], 1);
        assert_eq!(value["attributes"]["capture_id"], "cap_0000000000000001");
        assert_eq!(value["attributes"]["direction"], "inbound");
        assert_eq!(value["attributes"]["body"]["data"]["hello"], "world");
    }
}
