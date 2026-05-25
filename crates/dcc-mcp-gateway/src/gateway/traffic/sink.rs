use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use dcc_mcp_actions::events::EventEnvelope;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

use super::TrafficCaptureError;

pub(super) trait TrafficSink: Send + Sync + std::fmt::Debug {
    fn record(&self, event: &EventEnvelope);
}

#[derive(Debug)]
pub(super) struct LiveTrafficSink {
    capacity: usize,
    frames: Mutex<VecDeque<EventEnvelope>>,
}

impl LiveTrafficSink {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            frames: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
        }
    }

    pub(super) fn recent(&self, limit: usize) -> Vec<EventEnvelope> {
        let Ok(frames) = self.frames.lock() else {
            tracing::warn!("traffic capture: live sink lock poisoned");
            return Vec::new();
        };
        frames.iter().rev().take(limit).cloned().collect()
    }
}

impl TrafficSink for LiveTrafficSink {
    fn record(&self, event: &EventEnvelope) {
        let Ok(mut frames) = self.frames.lock() else {
            tracing::warn!("traffic capture: live sink lock poisoned");
            return;
        };
        frames.push_back(event.clone());
        while frames.len() > self.capacity {
            frames.pop_front();
        }
    }
}

#[derive(Debug)]
pub(super) struct JsonlTrafficSink {
    file: Mutex<File>,
}

impl JsonlTrafficSink {
    pub(super) fn open(path: &Path) -> Result<Self, std::io::Error> {
        ensure_parent_dir(path)?;
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl TrafficSink for JsonlTrafficSink {
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

#[derive(Debug)]
pub(super) struct SqliteTrafficSink {
    conn: Mutex<Connection>,
}

impl SqliteTrafficSink {
    pub(super) fn open(path: &Path) -> Result<Self, TrafficCaptureError> {
        ensure_parent_dir(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SQLITE_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl TrafficSink for SqliteTrafficSink {
    fn record(&self, event: &EventEnvelope) {
        let envelope = event.to_value();
        let attrs = &event.attributes;
        let Ok(envelope_json) = serde_json::to_string(&envelope) else {
            tracing::warn!("traffic capture: failed to encode SQLite envelope");
            return;
        };
        let redacted_paths = attrs
            .pointer("/body/redacted_paths")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let redacted_paths_json =
            serde_json::to_string(&redacted_paths).unwrap_or_else(|_| "[]".to_string());

        let Ok(conn) = self.conn.lock() else {
            tracing::warn!("traffic capture: SQLite sink lock poisoned");
            return;
        };

        if let Err(err) = conn.execute(
            "INSERT INTO traffic_frames (
                capture_id,
                event_id,
                timestamp_ns,
                session_id,
                direction,
                leg,
                transport,
                mcp_kind,
                mcp_method,
                http_url,
                http_status,
                body_size_bytes,
                redacted_paths,
                envelope_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                attr_str(attrs, "/capture_id"),
                event.id,
                event.timestamp_ns as i64,
                attr_str(attrs, "/session_id"),
                attr_str(attrs, "/direction"),
                attr_str(attrs, "/leg"),
                attr_str(attrs, "/transport"),
                attr_str(attrs, "/mcp/kind"),
                attr_str(attrs, "/mcp/method"),
                attr_str(attrs, "/http/url"),
                attr_u64(attrs, "/http/status").map(|v| v as i64),
                attr_u64(attrs, "/body/size_bytes").unwrap_or(0) as i64,
                redacted_paths_json,
                envelope_json,
            ],
        ) {
            tracing::warn!(error = %err, "traffic capture: failed to write SQLite frame");
        }
    }
}

const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS traffic_frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    capture_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    timestamp_ns INTEGER NOT NULL,
    session_id TEXT,
    direction TEXT NOT NULL,
    leg TEXT NOT NULL,
    transport TEXT NOT NULL,
    mcp_kind TEXT,
    mcp_method TEXT,
    http_url TEXT,
    http_status INTEGER,
    body_size_bytes INTEGER NOT NULL,
    redacted_paths TEXT NOT NULL,
    envelope_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_traffic_frames_session
    ON traffic_frames(session_id, timestamp_ns);
CREATE INDEX IF NOT EXISTS idx_traffic_frames_method
    ON traffic_frames(mcp_method, timestamp_ns);
CREATE INDEX IF NOT EXISTS idx_traffic_frames_leg
    ON traffic_frames(leg, timestamp_ns);
"#;

fn ensure_parent_dir(path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn attr_str(attrs: &Value, pointer: &str) -> Option<String> {
    attrs
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn attr_u64(attrs: &Value, pointer: &str) -> Option<u64> {
    attrs.pointer(pointer).and_then(Value::as_u64)
}
