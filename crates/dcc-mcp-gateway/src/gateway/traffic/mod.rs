//! Opt-in traffic capture for gateway debugging (RFC 0003).
//!
//! Capture is disabled unless either `DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path>`
//! or `DCC_MCP_TRAFFIC_CONFIG=<traffic_capture.yaml>` is set. Frames are
//! emitted on the shared `traffic.frame` event name and written to configured
//! sinks after filters and redaction rules run.

mod config;
mod filter;
mod frame;
mod redaction;
mod sink;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dcc_mcp_actions::EventBus;
use dcc_mcp_actions::events::EventEnvelope;
use serde_json::{Value, json};

use self::config::{TrafficCaptureDocument, TrafficSinkDocument};
use self::filter::TrafficFilter;
pub use self::frame::{
    TrafficFrame, basic_gateway_source, correlation, gateway_source, http_post, mcp_message,
};
use self::redaction::TrafficRedactor;
use self::sink::{JsonlTrafficSink, SqliteTrafficSink, TrafficSink};

pub const TRAFFIC_FRAME_EVENT: &str = "traffic.frame";
pub const TRAFFIC_FRAME_SCHEMA_VERSION: u32 = 1;
const ENV_CAPTURE: &str = "DCC_MCP_TRAFFIC_CAPTURE";
const ENV_CONFIG: &str = "DCC_MCP_TRAFFIC_CONFIG";
const ENV_PROD_PROFILE: &str = "DCC_MCP_PROD_PROFILE";
const ENV_FORCE_CAPTURE: &str = "DCC_MCP_FORCE_TRAFFIC_CAPTURE";

/// Gateway traffic capture bus plus optional sinks.
pub struct TrafficCapture {
    event_bus: EventBus,
    sinks: Vec<Arc<dyn TrafficSink>>,
    filter: TrafficFilter,
    redactor: TrafficRedactor,
    next_capture_id: AtomicU64,
}

impl std::fmt::Debug for TrafficCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrafficCapture")
            .field("event_bus", &self.event_bus)
            .field("sink_count", &self.sinks.len())
            .field("filter", &self.filter)
            .field("redactor", &self.redactor)
            .finish()
    }
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
            sinks: Vec::new(),
            filter: TrafficFilter::default(),
            redactor: TrafficRedactor::default(),
            next_capture_id: AtomicU64::new(0),
        }
    }

    pub fn from_env() -> Result<Self, TrafficCaptureError> {
        if let Some(config_path) = env_value(ENV_CONFIG).filter(|v| capture_enabled(v)) {
            return Self::from_config_path(config_path);
        }

        let spec = match std::env::var(ENV_CAPTURE) {
            Ok(raw) if capture_enabled(&raw) => raw,
            _ => return Ok(Self::disabled()),
        };

        block_prod_capture()?;

        let Some(path) = spec.strip_prefix("jsonl:").map(str::trim) else {
            return Err(TrafficCaptureError::UnsupportedSpec(spec));
        };
        if path.is_empty() {
            return Err(TrafficCaptureError::EmptyPath);
        }
        Self::with_jsonl_sink(path)
    }

    pub fn from_config_path(path: impl AsRef<Path>) -> Result<Self, TrafficCaptureError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)?;
        let document: TrafficCaptureDocument =
            serde_yaml_ng::from_str(&raw).map_err(|err| TrafficCaptureError::ConfigParse {
                path: path.to_path_buf(),
                message: err.to_string(),
            })?;

        if !document.enabled.unwrap_or(true) {
            return Ok(Self::disabled());
        }

        block_prod_capture()?;

        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        Self::from_document(document, base_dir)
    }

    pub fn with_jsonl_sink(path: impl AsRef<Path>) -> Result<Self, TrafficCaptureError> {
        Ok(Self {
            event_bus: EventBus::new(),
            sinks: vec![Arc::new(JsonlTrafficSink::open(path.as_ref())?)],
            filter: TrafficFilter::default(),
            redactor: TrafficRedactor::default(),
            next_capture_id: AtomicU64::new(0),
        })
    }

    fn from_document(
        document: TrafficCaptureDocument,
        base_dir: &Path,
    ) -> Result<Self, TrafficCaptureError> {
        let filter = TrafficFilter::from_document(document.filters)?;
        let redactor = TrafficRedactor::from_document(document.redact)?;
        let mut sinks: Vec<Arc<dyn TrafficSink>> = Vec::new();

        for sink in document.sinks.unwrap_or_default() {
            if let Some(sink) = open_sink(sink, base_dir)? {
                sinks.push(sink);
            }
        }

        Ok(Self {
            event_bus: EventBus::new(),
            sinks,
            filter,
            redactor,
            next_capture_id: AtomicU64::new(0),
        })
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !self.sinks.is_empty() || self.event_bus.has_subscribers(TRAFFIC_FRAME_EVENT)
    }

    pub fn emit_json_frame(&self, frame: TrafficFrame) -> Option<EventEnvelope> {
        if !self.is_enabled() {
            return None;
        }

        let mut attributes = json!({
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
                "size_bytes": 0,
                "redacted_paths": [],
            },
        });

        let redacted_paths = self.redactor.redact(&mut attributes);
        set_redacted_paths(&mut attributes, redacted_paths);
        set_body_size(&mut attributes);

        if !self.filter.allows(&attributes) {
            return None;
        }

        let event = self.event_bus.emit(
            TRAFFIC_FRAME_EVENT,
            frame.source,
            frame.correlation,
            attributes,
        );
        for sink in &self.sinks {
            sink.record(&event);
        }
        Some(event)
    }

    fn next_frame_id(&self) -> String {
        let seq = self.next_capture_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("cap_{seq:016x}")
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
    #[error("traffic capture config {path} is invalid: {message}")]
    ConfigParse { path: PathBuf, message: String },
    #[error("traffic capture sink '{kind}' requires a path")]
    SinkPathRequired { kind: String },
    #[error("unsupported traffic capture sink kind: {0}")]
    UnsupportedSink(String),
    #[error("traffic capture rule '{0}' must contain exactly one field matcher")]
    InvalidRule(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

fn open_sink(
    sink: TrafficSinkDocument,
    base_dir: &Path,
) -> Result<Option<Arc<dyn TrafficSink>>, TrafficCaptureError> {
    match sink.kind.trim().to_ascii_lowercase().as_str() {
        "file_jsonl" | "jsonl" => {
            let path = sink.path_required()?;
            Ok(Some(Arc::new(JsonlTrafficSink::open(
                &resolve_capture_path(base_dir, &path),
            )?)))
        }
        "sqlite" => {
            let path = sink.path_required()?;
            Ok(Some(Arc::new(SqliteTrafficSink::open(
                &resolve_capture_path(base_dir, &path),
            )?)))
        }
        "admin_live" | "ot_exporter" => {
            tracing::warn!(
                kind = %sink.kind,
                "traffic capture sink is reserved for a later RFC 0003 phase; skipping"
            );
            Ok(None)
        }
        other => Err(TrafficCaptureError::UnsupportedSink(other.to_string())),
    }
}

fn resolve_capture_path(base_dir: &Path, raw: &str) -> PathBuf {
    let expanded = raw.replace("${TIMESTAMP}", &timestamp_label());
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn timestamp_label() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn block_prod_capture() -> Result<(), TrafficCaptureError> {
    if truthy_env(ENV_PROD_PROFILE) && !truthy_env(ENV_FORCE_CAPTURE) {
        Err(TrafficCaptureError::ProdProfileBlocked)
    } else {
        Ok(())
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn set_redacted_paths(attributes: &mut Value, redacted_paths: Vec<String>) {
    if let Some(slot) = attributes.pointer_mut("/body/redacted_paths") {
        *slot = Value::Array(redacted_paths.into_iter().map(Value::String).collect());
    }
}

fn set_body_size(attributes: &mut Value) {
    let size = attributes
        .pointer("/body/data")
        .map(serialized_size)
        .unwrap_or(0);
    if let Some(slot) = attributes.pointer_mut("/body/size_bytes") {
        *slot = json!(size);
    }
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
    use serde_json::Value;
    use std::io::Write;
    use std::path::PathBuf;

    fn sample_frame(method: &str, url: &str) -> TrafficFrame {
        TrafficFrame::json(
            basic_gateway_source(),
            correlation(Some("req-1"), Some("trace-1"), Some("sess-1")),
            "inbound",
            "client_to_gateway",
            "http",
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": {
                    "arguments": {
                        "api_key": "secret-token",
                        "keep": "visible"
                    }
                }
            }),
        )
        .with_session_id(Some("sess-1"))
        .with_http(http_post(url, None, Some(200)))
        .with_mcp(mcp_message("request", method, Some(json!(1))))
    }

    #[test]
    fn jsonl_sink_writes_traffic_frame_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let path: PathBuf = dir.path().join("capture.jsonl");
        let capture = TrafficCapture::with_jsonl_sink(&path).unwrap();

        capture.emit_json_frame(sample_frame("tools/call", "/mcp"));

        let raw = std::fs::read_to_string(path).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 1);
        let value: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["name"], TRAFFIC_FRAME_EVENT);
        assert_eq!(value["correlation"]["request_id"], "req-1");
        assert_eq!(value["attributes"]["schema_version"], 1);
        assert_eq!(value["attributes"]["capture_id"], "cap_0000000000000001");
        assert_eq!(value["attributes"]["direction"], "inbound");
        assert_eq!(value["attributes"]["body"]["data"]["method"], "tools/call");
    }

    #[test]
    fn config_filters_and_redacts_before_jsonl_sink_write() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("traffic_capture.yaml");
        let jsonl_path = dir.path().join("capture.jsonl");
        let mut file = std::fs::File::create(&config_path).unwrap();
        writeln!(
            file,
            r#"
enabled: true
sinks:
  - kind: jsonl
    path: capture.jsonl
  - kind: admin_live
    ring_buffer: 5000
filters:
  include:
    - mcp.method: tools/call
  exclude:
    - http.url: "*/v1/readyz"
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
"#
        )
        .unwrap();

        let capture = TrafficCapture::from_config_path(&config_path).unwrap();
        capture.emit_json_frame(sample_frame("notifications/initialized", "/mcp"));
        capture.emit_json_frame(sample_frame("tools/call", "/v1/readyz"));
        capture.emit_json_frame(sample_frame("tools/call", "/mcp"));

        let raw = std::fs::read_to_string(jsonl_path).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 1);
        let value: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(
            value["attributes"]["body"]["data"]["params"]["arguments"]["api_key"],
            "[REDACTED]"
        );
        assert_eq!(
            value["attributes"]["body"]["redacted_paths"][0],
            "body.data.params.arguments.api_key"
        );
    }

    #[test]
    fn sqlite_sink_indexes_frames_for_replay_and_diff() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("traffic_capture.yaml");
        let sqlite_path = dir.path().join("capture.db");
        std::fs::write(
            &config_path,
            r#"
enabled: true
sinks:
  - kind: sqlite
    path: capture.db
filters:
  include:
    - mcp.method: tools/call
"#,
        )
        .unwrap();

        let capture = TrafficCapture::from_config_path(&config_path).unwrap();
        capture.emit_json_frame(sample_frame("tools/call", "/mcp"));

        let conn = rusqlite::Connection::open(sqlite_path).unwrap();
        let row: (String, String, String, String) = conn
            .query_row(
                "SELECT capture_id, session_id, mcp_method, http_url FROM traffic_frames",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "cap_0000000000000001");
        assert_eq!(row.1, "sess-1");
        assert_eq!(row.2, "tools/call");
        assert_eq!(row.3, "/mcp");
    }
}
