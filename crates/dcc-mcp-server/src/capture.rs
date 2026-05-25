//! Offline helpers for gateway traffic capture files.
//!
//! The gateway writes `traffic.frame` EventBus envelopes to JSONL or SQLite.
//! This module keeps replay and diff tooling on that persisted contract rather
//! than reaching into the live capture implementation.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Args, Subcommand, ValueEnum};
use dcc_mcp_actions::events::EventEnvelope;
use reqwest::Client;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Map, Value};

const TRAFFIC_FRAME_EVENT: &str = "traffic.frame";

#[derive(Debug, Subcommand)]
pub(crate) enum CaptureAction {
    /// Replay recorded client-to-gateway requests against a live gateway.
    Replay(ReplayArgs),
    /// Compare two capture sessions frame-by-frame.
    Diff(DiffArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ReplayArgs {
    /// Capture file to replay (`.jsonl`, `.db`, `.sqlite`, or `.sqlite3`).
    capture: PathBuf,
    /// Gateway MCP endpoint to post recorded client frames to.
    #[arg(long)]
    target: String,
    /// Optional session id to replay. Defaults to all captured sessions.
    #[arg(long)]
    session: Option<String>,
    /// Input file format. `auto` uses the file extension.
    #[arg(long, value_enum, default_value_t = CaptureFormat::Auto)]
    format: CaptureFormat,
    /// Assertion mode for recorded vs replayed responses.
    #[arg(long, value_enum, default_value_t = ReplayAssert::Compatible)]
    assert: ReplayAssert,
    /// Replace captured gateway tool slug instance ids with this instance id.
    #[arg(long)]
    rebind_instance_id: Option<String>,
    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
}

#[derive(Debug, Args)]
pub(crate) struct DiffArgs {
    /// Baseline capture file.
    before: PathBuf,
    /// Candidate capture file.
    after: PathBuf,
    /// Optional baseline session id to diff.
    #[arg(long)]
    before_session: Option<String>,
    /// Optional candidate session id to diff.
    #[arg(long)]
    after_session: Option<String>,
    /// Input file format. `auto` uses each file extension.
    #[arg(long, value_enum, default_value_t = CaptureFormat::Auto)]
    format: CaptureFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CaptureFormat {
    Auto,
    Jsonl,
    Sqlite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ReplayAssert {
    #[value(name = "outputs-equal")]
    Equal,
    #[value(name = "outputs-compatible")]
    Compatible,
    #[value(name = "outputs-ignored")]
    Ignored,
}

impl ReplayAssert {
    fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "outputs-equal",
            Self::Compatible => "outputs-compatible",
            Self::Ignored => "outputs-ignored",
        }
    }
}

pub(crate) async fn run(action: CaptureAction) -> anyhow::Result<()> {
    match action {
        CaptureAction::Replay(args) => {
            let summary = replay(args).await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            if summary.failed > 0 {
                bail!("traffic replay failed for {} frame(s)", summary.failed);
            }
        }
        CaptureAction::Diff(args) => {
            let summary = diff(args)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            if summary.changed > 0 || summary.added > 0 || summary.removed > 0 {
                bail!("traffic captures differ");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct CapturedFrame {
    ordinal: usize,
    envelope_id: String,
    session_id: Option<String>,
    direction: String,
    leg: String,
    transport: String,
    http_status: Option<u16>,
    mcp_id: Option<String>,
    mcp_method: Option<String>,
    body: Value,
}

impl CapturedFrame {
    fn from_envelope(ordinal: usize, envelope: EventEnvelope) -> Option<Self> {
        if envelope.name != TRAFFIC_FRAME_EVENT {
            return None;
        }

        let attrs = envelope.attributes.clone();
        let body = attrs.pointer("/body/data").cloned().unwrap_or(Value::Null);
        Some(Self {
            ordinal,
            envelope_id: envelope.id,
            session_id: attrs
                .pointer("/session_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            direction: attr_string(&attrs, "/direction").unwrap_or_default(),
            leg: attr_string(&attrs, "/leg").unwrap_or_default(),
            transport: attr_string(&attrs, "/transport").unwrap_or_default(),
            http_status: attrs
                .pointer("/http/status")
                .and_then(Value::as_u64)
                .and_then(|v| u16::try_from(v).ok()),
            mcp_id: attrs.pointer("/mcp/id").map(value_key),
            mcp_method: attr_string(&attrs, "/mcp/method"),
            body,
        })
    }

    fn is_client_request(&self) -> bool {
        self.direction == "inbound" && self.leg == "client_to_gateway"
    }

    fn is_gateway_response(&self) -> bool {
        self.direction == "outbound" && self.leg == "gateway_to_client"
    }

    fn same_session(&self, wanted: Option<&str>) -> bool {
        wanted.is_none_or(|session| self.session_id.as_deref() == Some(session))
    }

    fn signature(&self) -> FrameSignature {
        FrameSignature {
            direction: self.direction.clone(),
            leg: self.leg.clone(),
            transport: self.transport.clone(),
            mcp_method: self.mcp_method.clone(),
            http_status: self.http_status,
            body: self.body.clone(),
        }
    }

    fn brief(&self) -> FrameBrief {
        FrameBrief {
            ordinal: self.ordinal,
            envelope_id: self.envelope_id.clone(),
            session_id: self.session_id.clone(),
            direction: self.direction.clone(),
            leg: self.leg.clone(),
            mcp_method: self.mcp_method.clone(),
            mcp_id: self.mcp_id.clone(),
            http_status: self.http_status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FrameSignature {
    direction: String,
    leg: String,
    transport: String,
    mcp_method: Option<String>,
    http_status: Option<u16>,
    body: Value,
}

#[derive(Debug, Clone, Serialize)]
struct FrameBrief {
    ordinal: usize,
    envelope_id: String,
    session_id: Option<String>,
    direction: String,
    leg: String,
    mcp_method: Option<String>,
    mcp_id: Option<String>,
    http_status: Option<u16>,
}

#[derive(Debug, Serialize)]
struct ReplaySummary {
    capture: String,
    target: String,
    session: Option<String>,
    assert: String,
    planned: usize,
    replayed: usize,
    matched: usize,
    failed: usize,
    failures: Vec<ReplayFailure>,
}

#[derive(Debug, Serialize)]
struct ReplayFailure {
    ordinal: usize,
    reason: String,
}

#[derive(Debug, Serialize)]
struct DiffSummary {
    before: String,
    after: String,
    before_session: Option<String>,
    after_session: Option<String>,
    before_count: usize,
    after_count: usize,
    matched: usize,
    changed: usize,
    added: usize,
    removed: usize,
    changes: Vec<DiffChange>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
enum DiffChange {
    Changed {
        index: usize,
        before: FrameBrief,
        after: FrameBrief,
    },
    Added {
        index: usize,
        after: FrameBrief,
    },
    Removed {
        index: usize,
        before: FrameBrief,
    },
}

async fn replay(args: ReplayArgs) -> anyhow::Result<ReplaySummary> {
    let frames = read_capture(&args.capture, args.format)?;
    let requests = frames
        .iter()
        .filter(|frame| frame.same_session(args.session.as_deref()))
        .filter(|frame| frame.is_client_request())
        .cloned()
        .collect::<Vec<_>>();
    let responses = response_index(&frames, args.session.as_deref());
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout_secs))
        .build()?;

    let mut matched = 0usize;
    let mut failures = Vec::new();
    for request in &requests {
        let mut body = request.body.clone();
        if let Some(instance_id) = args.rebind_instance_id.as_deref() {
            rebind_instance(&mut body, instance_id);
        }

        let response = match client.post(&args.target).json(&body).send().await {
            Ok(response) => response,
            Err(error) => {
                failures.push(ReplayFailure {
                    ordinal: request.ordinal,
                    reason: format!("request failed: {error}"),
                });
                continue;
            }
        };

        let status = response.status().as_u16();
        let actual_body = match response.json::<Value>().await {
            Ok(value) => value,
            Err(error) => {
                failures.push(ReplayFailure {
                    ordinal: request.ordinal,
                    reason: format!("response was not JSON: {error}"),
                });
                continue;
            }
        };

        if args.assert == ReplayAssert::Ignored {
            matched += 1;
            continue;
        }

        let expected = responses
            .get(&frame_key(request))
            .or_else(|| request.mcp_id.as_ref().and_then(|id| responses.get(id)));

        let Some(expected) = expected else {
            failures.push(ReplayFailure {
                ordinal: request.ordinal,
                reason: "no recorded gateway response for request".to_string(),
            });
            continue;
        };

        let ok = match args.assert {
            ReplayAssert::Equal => {
                status == expected.http_status.unwrap_or(200) && actual_body == expected.body
            }
            ReplayAssert::Compatible => {
                status == expected.http_status.unwrap_or(200)
                    && jsonrpc_shape(&actual_body) == jsonrpc_shape(&expected.body)
            }
            ReplayAssert::Ignored => true,
        };

        if ok {
            matched += 1;
        } else {
            failures.push(ReplayFailure {
                ordinal: request.ordinal,
                reason: format!(
                    "response mismatch: expected {} status {:?}, got status {}",
                    jsonrpc_shape(&expected.body),
                    expected.http_status,
                    status
                ),
            });
        }
    }

    Ok(ReplaySummary {
        capture: args.capture.display().to_string(),
        target: args.target,
        session: args.session,
        assert: args.assert.as_str().to_string(),
        planned: requests.len(),
        replayed: requests.len(),
        matched,
        failed: failures.len(),
        failures,
    })
}

fn diff(args: DiffArgs) -> anyhow::Result<DiffSummary> {
    let before = filter_session(
        read_capture(&args.before, args.format)?,
        args.before_session.as_deref(),
    );
    let after = filter_session(
        read_capture(&args.after, args.format)?,
        args.after_session.as_deref(),
    );
    let mut changes = Vec::new();
    let mut matched = 0usize;
    let max_len = before.len().max(after.len());

    for index in 0..max_len {
        match (before.get(index), after.get(index)) {
            (Some(left), Some(right)) if left.signature() == right.signature() => {
                matched += 1;
            }
            (Some(left), Some(right)) => changes.push(DiffChange::Changed {
                index,
                before: left.brief(),
                after: right.brief(),
            }),
            (Some(left), None) => changes.push(DiffChange::Removed {
                index,
                before: left.brief(),
            }),
            (None, Some(right)) => changes.push(DiffChange::Added {
                index,
                after: right.brief(),
            }),
            (None, None) => {}
        }
    }

    let changed = changes
        .iter()
        .filter(|change| matches!(change, DiffChange::Changed { .. }))
        .count();
    let added = changes
        .iter()
        .filter(|change| matches!(change, DiffChange::Added { .. }))
        .count();
    let removed = changes
        .iter()
        .filter(|change| matches!(change, DiffChange::Removed { .. }))
        .count();

    Ok(DiffSummary {
        before: args.before.display().to_string(),
        after: args.after.display().to_string(),
        before_session: args.before_session,
        after_session: args.after_session,
        before_count: before.len(),
        after_count: after.len(),
        matched,
        changed,
        added,
        removed,
        changes,
    })
}

fn read_capture(path: &Path, format: CaptureFormat) -> anyhow::Result<Vec<CapturedFrame>> {
    match resolve_format(path, format)? {
        CaptureFormat::Jsonl => read_jsonl(path),
        CaptureFormat::Sqlite => read_sqlite(path),
        CaptureFormat::Auto => unreachable!("auto format must be resolved"),
    }
}

fn resolve_format(path: &Path, format: CaptureFormat) -> anyhow::Result<CaptureFormat> {
    if format != CaptureFormat::Auto {
        return Ok(format);
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jsonl") | Some("ndjson") => Ok(CaptureFormat::Jsonl),
        Some("db") | Some("sqlite") | Some("sqlite3") => Ok(CaptureFormat::Sqlite),
        _ => bail!(
            "could not infer capture format for {}; pass --format jsonl or --format sqlite",
            path.display()
        ),
    }
}

fn read_jsonl(path: &Path) -> anyhow::Result<Vec<CapturedFrame>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut frames = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: EventEnvelope = serde_json::from_str(&line)
            .with_context(|| format!("invalid JSONL envelope at line {}", index + 1))?;
        if let Some(frame) = CapturedFrame::from_envelope(index, envelope) {
            frames.push(frame);
        }
    }
    Ok(frames)
}

fn read_sqlite(path: &Path) -> anyhow::Result<Vec<CapturedFrame>> {
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut stmt = conn
        .prepare(
            "SELECT envelope_json
             FROM traffic_frames
             ORDER BY timestamp_ns ASC, id ASC",
        )
        .context("capture database is missing traffic_frames.envelope_json")?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut frames = Vec::new();
    for (index, row) in rows.enumerate() {
        let raw = row?;
        let envelope: EventEnvelope = serde_json::from_str(&raw)
            .with_context(|| format!("invalid envelope_json at row {}", index + 1))?;
        if let Some(frame) = CapturedFrame::from_envelope(index, envelope) {
            frames.push(frame);
        }
    }
    Ok(frames)
}

fn response_index(
    frames: &[CapturedFrame],
    session: Option<&str>,
) -> HashMap<String, CapturedFrame> {
    let mut map = HashMap::new();
    for frame in frames
        .iter()
        .filter(|frame| frame.same_session(session))
        .filter(|frame| frame.is_gateway_response())
    {
        map.insert(frame_key(frame), frame.clone());
        if let Some(id) = &frame.mcp_id {
            map.insert(id.clone(), frame.clone());
        }
    }
    map
}

fn frame_key(frame: &CapturedFrame) -> String {
    frame
        .mcp_id
        .clone()
        .unwrap_or_else(|| format!("ordinal:{}", frame.ordinal))
}

fn filter_session(frames: Vec<CapturedFrame>, session: Option<&str>) -> Vec<CapturedFrame> {
    frames
        .into_iter()
        .filter(|frame| frame.same_session(session))
        .collect()
}

fn attr_string(attrs: &Value, pointer: &str) -> Option<String> {
    attrs
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn value_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn jsonrpc_shape(value: &Value) -> &'static str {
    if value.get("error").is_some() {
        "error"
    } else if value.pointer("/result/isError").and_then(Value::as_bool) == Some(true) {
        "tool-error"
    } else if value.get("result").is_some() {
        "result"
    } else {
        "unknown"
    }
}

fn rebind_instance(value: &mut Value, replacement: &str) {
    match value {
        Value::Object(map) => rebind_object(map, replacement),
        Value::Array(items) => {
            for item in items {
                rebind_instance(item, replacement);
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn rebind_object(map: &mut Map<String, Value>, replacement: &str) {
    for (key, value) in map.iter_mut() {
        if key == "name" {
            if let Some(slug) = value
                .as_str()
                .and_then(|s| rebind_tool_slug(s, replacement))
            {
                *value = Value::String(slug);
                continue;
            }
        } else if (key == "instance_id" || key == "instance") && value.is_string() {
            *value = Value::String(replacement.to_string());
            continue;
        }
        rebind_instance(value, replacement);
    }
}

fn rebind_tool_slug(slug: &str, replacement: &str) -> Option<String> {
    let mut parts = slug.splitn(3, '.');
    let dcc = parts.next()?;
    let _instance = parts.next()?;
    let tool = parts.next()?;
    if dcc.is_empty() || tool.is_empty() {
        return None;
    }
    Some(format!("{dcc}.{replacement}.{tool}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use dcc_mcp_actions::events::EventEnvelope;
    use serde_json::json;
    use tempfile::NamedTempFile;

    use super::*;

    fn frame(id: &str, direction: &str, leg: &str, session: &str, body: Value) -> EventEnvelope {
        EventEnvelope::new(
            TRAFFIC_FRAME_EVENT,
            id,
            json!({"service": "test"}),
            json!({}),
            json!({
                "capture_id": id,
                "session_id": session,
                "direction": direction,
                "leg": leg,
                "transport": "http",
                "http": {"status": if direction == "outbound" { Some(200) } else { None::<u16> }},
                "mcp": {"kind": if direction == "inbound" { "request" } else { "response" }, "method": "tools/call", "id": 1},
                "body": {"encoding": "json", "data": body, "size_bytes": 2, "redacted_paths": []}
            }),
        )
    }

    #[test]
    fn reads_jsonl_traffic_frames_only() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&frame(
                "cap_1",
                "inbound",
                "client_to_gateway",
                "sess",
                json!({"jsonrpc": "2.0", "id": 1})
            ))
            .unwrap()
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&EventEnvelope::new(
                "tool.completed",
                "ev_2",
                json!({}),
                json!({}),
                json!({})
            ))
            .unwrap()
        )
        .unwrap();

        let frames = read_capture(file.path(), CaptureFormat::Jsonl).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].direction, "inbound");
    }

    #[test]
    fn diff_detects_changed_frames() {
        let before = [CapturedFrame::from_envelope(
            0,
            frame(
                "cap_1",
                "outbound",
                "gateway_to_client",
                "sess",
                json!({"result": {"isError": false}}),
            ),
        )
        .unwrap()];
        let after = [CapturedFrame::from_envelope(
            0,
            frame(
                "cap_2",
                "outbound",
                "gateway_to_client",
                "sess",
                json!({"result": {"isError": true}}),
            ),
        )
        .unwrap()];

        assert_ne!(before[0].signature(), after[0].signature());
    }

    #[test]
    fn rebinds_gateway_tool_slug_and_instance_fields() {
        let mut body = json!({
            "params": {
                "name": "maya.old123.maya_tools__render",
                "arguments": {"instance_id": "old123", "other": "keep"}
            }
        });
        rebind_instance(&mut body, "new456");
        assert_eq!(
            body.pointer("/params/name").and_then(Value::as_str),
            Some("maya.new456.maya_tools__render")
        );
        assert_eq!(
            body.pointer("/params/arguments/instance_id")
                .and_then(Value::as_str),
            Some("new456")
        );
        assert_eq!(
            body.pointer("/params/arguments/other")
                .and_then(Value::as_str),
            Some("keep")
        );
    }

    #[test]
    fn sqlite_reader_orders_frames() {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE traffic_frames (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp_ns INTEGER NOT NULL,
                envelope_json TEXT NOT NULL
            );",
        )
        .unwrap();
        let envelope = serde_json::to_string(&frame(
            "cap_sqlite",
            "inbound",
            "client_to_gateway",
            "sess",
            json!({"jsonrpc": "2.0", "id": 1}),
        ))
        .unwrap();
        conn.execute(
            "INSERT INTO traffic_frames (timestamp_ns, envelope_json) VALUES (?1, ?2)",
            rusqlite::params![1_i64, envelope],
        )
        .unwrap();

        let frames = read_capture(file.path(), CaptureFormat::Sqlite).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].envelope_id, "cap_sqlite");
    }

    #[test]
    fn jsonrpc_shape_distinguishes_result_error_and_tool_error() {
        assert_eq!(jsonrpc_shape(&json!({"error": {"code": -32000}})), "error");
        assert_eq!(
            jsonrpc_shape(&json!({"result": {"isError": true}})),
            "tool-error"
        );
        assert_eq!(jsonrpc_shape(&json!({"result": {}})), "result");
    }
}
