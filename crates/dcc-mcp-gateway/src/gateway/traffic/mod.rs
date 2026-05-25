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

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use dcc_mcp_actions::EventBus;
use dcc_mcp_actions::events::EventEnvelope;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use self::config::{TrafficCaptureDocument, TrafficSinkDocument};
use self::filter::{TrafficFilter, TrafficFilterSnapshot};
pub use self::frame::{
    TrafficFrame, basic_gateway_source, correlation, gateway_source, http_post, mcp_message,
};
use self::redaction::{TrafficRedactionSnapshot, TrafficRedactor};
use self::sink::{JsonlTrafficSink, SqliteTrafficSink, TrafficSink};

pub const TRAFFIC_FRAME_EVENT: &str = "traffic.frame";
pub const TRAFFIC_FRAME_SCHEMA_VERSION: u32 = 1;
const ENV_CAPTURE: &str = "DCC_MCP_TRAFFIC_CAPTURE";
const ENV_CONFIG: &str = "DCC_MCP_TRAFFIC_CONFIG";
const ENV_PROD_PROFILE: &str = "DCC_MCP_PROD_PROFILE";
const ENV_FORCE_CAPTURE: &str = "DCC_MCP_FORCE_TRAFFIC_CAPTURE";
const DECISION_LOG_CAPACITY: usize = 200;

/// Gateway traffic capture bus plus optional sinks.
pub struct TrafficCapture {
    event_bus: EventBus,
    sinks: Vec<Arc<dyn TrafficSink>>,
    sink_descriptors: Vec<TrafficSinkSnapshot>,
    filter: TrafficFilter,
    redactor: TrafficRedactor,
    next_capture_id: AtomicU64,
    decisions: Mutex<VecDeque<TrafficCaptureDecision>>,
}

impl std::fmt::Debug for TrafficCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrafficCapture")
            .field("event_bus", &self.event_bus)
            .field("sink_count", &self.sinks.len())
            .field("sink_descriptors", &self.sink_descriptors)
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
            sink_descriptors: Vec::new(),
            filter: TrafficFilter::default(),
            redactor: TrafficRedactor::default(),
            next_capture_id: AtomicU64::new(0),
            decisions: Mutex::new(VecDeque::with_capacity(DECISION_LOG_CAPACITY)),
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
            sink_descriptors: vec![TrafficSinkSnapshot {
                kind: "jsonl".to_string(),
                path: Some(path.as_ref().to_string_lossy().to_string()),
            }],
            filter: TrafficFilter::default(),
            redactor: TrafficRedactor::default(),
            next_capture_id: AtomicU64::new(0),
            decisions: Mutex::new(VecDeque::with_capacity(DECISION_LOG_CAPACITY)),
        })
    }

    fn from_document(
        document: TrafficCaptureDocument,
        base_dir: &Path,
    ) -> Result<Self, TrafficCaptureError> {
        let filter = TrafficFilter::from_document(document.filters)?;
        let redactor = TrafficRedactor::from_document(document.redact)?;
        let mut sinks: Vec<Arc<dyn TrafficSink>> = Vec::new();
        let mut sink_descriptors = Vec::new();

        for sink in document.sinks.unwrap_or_default() {
            let descriptor = sink_descriptor(&sink, base_dir)?;
            if let Some(sink) = open_sink(sink, base_dir)? {
                sinks.push(sink);
                if let Some(descriptor) = descriptor {
                    sink_descriptors.push(descriptor);
                }
            }
        }

        Ok(Self {
            event_bus: EventBus::new(),
            sinks,
            sink_descriptors,
            filter,
            redactor,
            next_capture_id: AtomicU64::new(0),
            decisions: Mutex::new(VecDeque::with_capacity(DECISION_LOG_CAPACITY)),
        })
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !self.sinks.is_empty() || self.event_bus.has_subscribers(TRAFFIC_FRAME_EVENT)
    }

    pub fn emit_json_frame(&self, frame: TrafficFrame) -> Option<EventEnvelope> {
        if !self.is_enabled() {
            self.record_decision(TrafficCaptureDecision::from_frame(
                &frame,
                "skipped",
                Some("capture-disabled"),
                Vec::new(),
                0,
            ));
            return None;
        }

        let mut attributes = json!({
            "schema_version": TRAFFIC_FRAME_SCHEMA_VERSION,
            "capture_id": self.next_frame_id(),
            "session_id": frame.session_id.clone(),
            "direction": frame.direction,
            "leg": frame.leg,
            "transport": frame.transport,
            "http": frame.http.clone(),
            "mcp": frame.mcp.clone(),
            "body": {
                "encoding": "json",
                "data": frame.body.clone(),
                "size_bytes": 0,
                "redacted_paths": [],
            },
        });

        let redacted_paths = self.redactor.redact(&mut attributes);
        let redacted_for_decision = redacted_paths.clone();
        set_redacted_paths(&mut attributes, redacted_paths);
        set_body_size(&mut attributes);
        let body_size_bytes = attributes
            .pointer("/body/size_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        if !self.filter.allows(&attributes) {
            self.record_decision(TrafficCaptureDecision::from_frame(
                &frame,
                "skipped",
                Some("filter"),
                redacted_for_decision,
                body_size_bytes,
            ));
            return None;
        }

        let event = self.event_bus.emit(
            TRAFFIC_FRAME_EVENT,
            frame.source.clone(),
            frame.correlation.clone(),
            attributes,
        );
        for sink in &self.sinks {
            sink.record(&event);
        }
        self.record_decision(TrafficCaptureDecision::from_frame(
            &frame,
            "captured",
            None,
            redacted_for_decision,
            body_size_bytes,
        ));
        Some(event)
    }

    #[must_use]
    pub fn governance_snapshot(&self) -> TrafficCaptureSnapshot {
        let prod_profile = truthy_env(ENV_PROD_PROFILE);
        let force_capture = truthy_env(ENV_FORCE_CAPTURE);
        TrafficCaptureSnapshot {
            enabled: self.is_enabled(),
            mode: if self.is_enabled() {
                "high_sensitivity_capture".to_string()
            } else {
                "safe_aggregate_only".to_string()
            },
            sinks: self.sink_descriptors.clone(),
            sink_count: self.sinks.len(),
            subscriber_enabled: self.event_bus.has_subscribers(TRAFFIC_FRAME_EVENT),
            filter: self.filter.snapshot(),
            redaction: self.redactor.snapshot(),
            production_profile: prod_profile,
            force_capture,
            production_guardrail: if prod_profile && !force_capture {
                "capture-blocked"
            } else if prod_profile {
                "forced"
            } else {
                "inactive"
            }
            .to_string(),
            recent_decisions: self.recent_decisions(DECISION_LOG_CAPACITY),
        }
    }

    fn next_frame_id(&self) -> String {
        let seq = self.next_capture_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("cap_{seq:016x}")
    }

    fn record_decision(&self, decision: TrafficCaptureDecision) {
        let mut decisions = self.decisions.lock();
        decisions.push_back(decision);
        while decisions.len() > DECISION_LOG_CAPACITY {
            decisions.pop_front();
        }
    }

    fn recent_decisions(&self, limit: usize) -> Vec<TrafficCaptureDecision> {
        let decisions = self.decisions.lock();
        decisions
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficSinkSnapshot {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficCaptureSnapshot {
    pub enabled: bool,
    pub mode: String,
    pub sinks: Vec<TrafficSinkSnapshot>,
    pub sink_count: usize,
    pub subscriber_enabled: bool,
    pub filter: TrafficFilterSnapshot,
    pub redaction: TrafficRedactionSnapshot,
    pub production_profile: bool,
    pub force_capture: bool,
    pub production_guardrail: String,
    pub recent_decisions: Vec<TrafficCaptureDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficCaptureDecision {
    pub timestamp: SystemTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub direction: String,
    pub leg: String,
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_method: Option<String>,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub redacted_paths: Vec<String>,
    pub body_size_bytes: u64,
}

impl TrafficCaptureDecision {
    fn from_frame(
        frame: &TrafficFrame,
        outcome: &str,
        reason: Option<&str>,
        redacted_paths: Vec<String>,
        body_size_bytes: u64,
    ) -> Self {
        Self {
            timestamp: SystemTime::now(),
            request_id: frame
                .correlation
                .get("request_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            trace_id: frame
                .correlation
                .get("trace_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            session_id: frame.session_id.clone(),
            direction: frame.direction.to_string(),
            leg: frame.leg.to_string(),
            transport: frame.transport.to_string(),
            http_url: frame
                .http
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string),
            mcp_method: frame
                .mcp
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_string),
            outcome: outcome.to_string(),
            reason: reason.map(str::to_string),
            redacted_paths,
            body_size_bytes,
        }
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

fn sink_descriptor(
    sink: &TrafficSinkDocument,
    base_dir: &Path,
) -> Result<Option<TrafficSinkSnapshot>, TrafficCaptureError> {
    match sink.kind.trim().to_ascii_lowercase().as_str() {
        "file_jsonl" | "jsonl" | "sqlite" => {
            let path = sink.path_required()?;
            Ok(Some(TrafficSinkSnapshot {
                kind: sink.kind.trim().to_ascii_lowercase(),
                path: Some(
                    resolve_capture_path(base_dir, &path)
                        .to_string_lossy()
                        .to_string(),
                ),
            }))
        }
        "admin_live" | "ot_exporter" => Ok(None),
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn disabled_capture_records_safe_skip_decision() {
        let capture = TrafficCapture::disabled();
        capture.emit_json_frame(sample_frame("tools/call", "/mcp"));

        let snapshot = capture.governance_snapshot();
        assert!(!snapshot.enabled);
        assert_eq!(snapshot.mode, "safe_aggregate_only");
        assert_eq!(snapshot.recent_decisions.len(), 1);
        assert_eq!(snapshot.recent_decisions[0].outcome, "skipped");
        assert_eq!(
            snapshot.recent_decisions[0].reason.as_deref(),
            Some("capture-disabled")
        );
    }

    #[test]
    fn governance_snapshot_reports_enabled_redaction_and_decisions() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("traffic_capture.yaml");
        std::fs::write(
            &config_path,
            r#"
enabled: true
sinks:
  - kind: jsonl
    path: capture.jsonl
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
"#,
        )
        .unwrap();

        let capture = TrafficCapture::from_config_path(&config_path).unwrap();
        capture.emit_json_frame(sample_frame("tools/call", "/mcp"));
        let snapshot = capture.governance_snapshot();

        assert!(snapshot.enabled);
        assert_eq!(snapshot.mode, "high_sensitivity_capture");
        assert_eq!(snapshot.sink_count, 1);
        assert_eq!(
            snapshot.redaction.paths,
            vec!["body.data.params.arguments.api_key"]
        );
        assert_eq!(snapshot.recent_decisions[0].outcome, "captured");
        assert_eq!(
            snapshot.recent_decisions[0].redacted_paths,
            vec!["body.data.params.arguments.api_key"]
        );
    }

    #[test]
    fn production_profile_blocks_env_capture_without_force() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let capture_path = dir.path().join("capture.jsonl");
        // SAFETY: serialized by ENV_LOCK for this test module.
        unsafe {
            std::env::set_var(ENV_PROD_PROFILE, "1");
            std::env::remove_var(ENV_FORCE_CAPTURE);
            std::env::set_var(ENV_CAPTURE, format!("jsonl:{}", capture_path.display()));
            std::env::remove_var(ENV_CONFIG);
        }

        let err = TrafficCapture::from_env().unwrap_err();
        assert!(matches!(err, TrafficCaptureError::ProdProfileBlocked));

        // SAFETY: serialized by ENV_LOCK for this test module.
        unsafe {
            std::env::remove_var(ENV_PROD_PROFILE);
            std::env::remove_var(ENV_FORCE_CAPTURE);
            std::env::remove_var(ENV_CAPTURE);
            std::env::remove_var(ENV_CONFIG);
        }
    }
}
