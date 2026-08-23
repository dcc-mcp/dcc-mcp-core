//! Durable JSONL persistence for admin audit and trace records.

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AdminAuditRecord, AgentContextTrust, DispatchTrace, LlmUsage, TokenTelemetry};

/// Bounded JSONL store for audit records and dispatch traces.
#[derive(Debug, Clone)]
pub struct DurableAuditStore {
    dir: Arc<PathBuf>,
    max_rows: usize,
    max_bytes: u64,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAuditRecord {
    timestamp_ms: u64,
    request_id: String,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    parent_span_id: Option<String>,
    method: Option<String>,
    instance_id: Option<String>,
    session_id: Option<String>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    agent_name: Option<String>,
    #[serde(default)]
    agent_model: Option<String>,
    #[serde(default)]
    actor_id: Option<String>,
    #[serde(default)]
    actor_name: Option<String>,
    #[serde(default)]
    actor_email_hash: Option<String>,
    #[serde(default)]
    client_platform: Option<String>,
    #[serde(default)]
    client_os: Option<String>,
    #[serde(default)]
    client_host: Option<String>,
    #[serde(default)]
    auth_subject: Option<String>,
    #[serde(default)]
    source_ip: Option<String>,
    #[serde(default)]
    attribution_trust: Option<AgentContextTrust>,
    #[serde(default)]
    parent_request_id: Option<String>,
    action: String,
    dcc_type: Option<String>,
    success: bool,
    error: Option<String>,
    duration_ms: Option<u64>,
    #[serde(default)]
    token_accounting: Option<TokenTelemetry>,
    #[serde(default)]
    llm_usage: Option<LlmUsage>,
}

impl DurableAuditStore {
    pub const AUDIT_FILE: &'static str = "audit.jsonl";
    pub const TRACE_FILE: &'static str = "traces.jsonl";
    pub const DEFAULT_MAX_ROWS: usize = 5_000;
    /// Default on-disk cap for each JSONL file (about 50 MiB).
    pub const DEFAULT_MAX_BYTES: u64 = 52_428_800;

    pub fn new(dir: impl Into<PathBuf>, max_rows: usize, max_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir: Arc::new(dir),
            max_rows: max_rows.max(1),
            max_bytes: max_bytes.max(1024),
            lock: Arc::new(Mutex::new(())),
        })
    }

    #[must_use]
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var_os("DCC_MCP_GATEWAY_AUDIT_DIR")?;
        let max_rows = std::env::var("DCC_MCP_GATEWAY_AUDIT_MAX_ROWS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(Self::DEFAULT_MAX_ROWS);
        let max_bytes = std::env::var("DCC_MCP_GATEWAY_AUDIT_MAX_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(Self::DEFAULT_MAX_BYTES)
            .max(1024);
        Self::new(dir, max_rows, max_bytes).ok()
    }

    #[must_use]
    pub fn load_audit(&self) -> Vec<AdminAuditRecord> {
        read_jsonl(&self.path(Self::AUDIT_FILE))
            .into_iter()
            .filter_map(|value| serde_json::from_value::<PersistedAuditRecord>(value).ok())
            .map(AdminAuditRecord::from)
            .collect()
    }

    #[must_use]
    pub fn load_traces(&self) -> Vec<DispatchTrace> {
        read_jsonl(&self.path(Self::TRACE_FILE))
            .into_iter()
            .filter_map(|value| serde_json::from_value::<DispatchTrace>(value).ok())
            .collect()
    }

    pub fn append_audit(&self, record: &AdminAuditRecord) {
        self.append_value(Self::AUDIT_FILE, &json!(PersistedAuditRecord::from(record)));
    }

    pub fn append_trace(&self, trace: &DispatchTrace) {
        if let Ok(value) = serde_json::to_value(trace) {
            self.append_value(Self::TRACE_FILE, &value);
        }
    }

    fn append_value(&self, filename: &str, value: &Value) {
        let _guard = self.lock.lock();
        let path = self.path(filename);
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
            return;
        };
        if serde_json::to_writer(&mut file, value).is_ok() {
            let _ = file.write_all(b"\n");
        }
        self.trim_file(&path);
    }

    fn trim_file(&self, path: &Path) {
        let Ok(file) = fs::File::open(path) else {
            return;
        };
        let mut lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|line| !line.trim().is_empty())
            .collect();
        if lines.len() > self.max_rows {
            let keep_from = lines.len() - self.max_rows;
            lines.drain(0..keep_from);
            let _ = fs::write(path, lines.join("\n") + "\n");
        }
        self.enforce_byte_budget(path);
    }

    fn enforce_byte_budget(&self, path: &Path) {
        for _ in 0..32 {
            let len = match fs::metadata(path) {
                Ok(metadata) => metadata.len(),
                Err(_) => return,
            };
            if len <= self.max_bytes {
                return;
            }
            let Ok(file) = fs::File::open(path) else {
                return;
            };
            let mut lines: Vec<String> = std::io::BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .filter(|line| !line.trim().is_empty())
                .collect();
            if lines.len() <= 1 {
                return;
            }
            let drop_count = (lines.len() / 2).max(1);
            lines.drain(0..drop_count);
            let _ = fs::write(path, lines.join("\n") + "\n");
        }
    }

    fn path(&self, filename: &str) -> PathBuf {
        self.dir.join(filename)
    }
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect()
}

impl From<&AdminAuditRecord> for PersistedAuditRecord {
    fn from(record: &AdminAuditRecord) -> Self {
        Self {
            timestamp_ms: record
                .timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            request_id: record.request_id.clone(),
            trace_id: record.trace_id.clone(),
            span_id: record.span_id.clone(),
            parent_span_id: record.parent_span_id.clone(),
            method: record.method.clone(),
            instance_id: record.instance_id.clone(),
            session_id: record.session_id.clone(),
            transport: record.transport.clone(),
            agent_id: record.agent_id.clone(),
            agent_name: record.agent_name.clone(),
            agent_model: record.agent_model.clone(),
            actor_id: record.actor_id.clone(),
            actor_name: record.actor_name.clone(),
            actor_email_hash: record.actor_email_hash.clone(),
            client_platform: record.client_platform.clone(),
            client_os: record.client_os.clone(),
            client_host: record.client_host.clone(),
            auth_subject: record.auth_subject.clone(),
            source_ip: record.source_ip.clone(),
            attribution_trust: record.attribution_trust.clone(),
            parent_request_id: record.parent_request_id.clone(),
            action: record.action.clone(),
            dcc_type: record.dcc_type.clone(),
            success: record.success,
            error: record.error.clone(),
            duration_ms: record.duration_ms,
            token_accounting: record.token_accounting.clone(),
            llm_usage: record.llm_usage.clone(),
        }
    }
}

impl From<PersistedAuditRecord> for AdminAuditRecord {
    fn from(record: PersistedAuditRecord) -> Self {
        Self {
            timestamp: UNIX_EPOCH + Duration::from_millis(record.timestamp_ms),
            request_id: record.request_id,
            trace_id: record.trace_id,
            span_id: record.span_id,
            parent_span_id: record.parent_span_id,
            method: record.method,
            instance_id: record.instance_id,
            session_id: record.session_id,
            transport: record.transport,
            agent_id: record.agent_id,
            agent_name: record.agent_name,
            agent_model: record.agent_model,
            actor_id: record.actor_id,
            actor_name: record.actor_name,
            actor_email_hash: record.actor_email_hash,
            client_platform: record.client_platform,
            client_os: record.client_os,
            client_host: record.client_host,
            auth_subject: record.auth_subject,
            source_ip: record.source_ip,
            attribution_trust: record.attribution_trust,
            parent_request_id: record.parent_request_id,
            action: record.action,
            dcc_type: record.dcc_type,
            success: record.success,
            error: record.error,
            duration_ms: record.duration_ms,
            token_accounting: record.token_accounting,
            llm_usage: record.llm_usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TraceLog;

    fn audit_record(id: &str) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp: UNIX_EPOCH + Duration::from_millis(1),
            request_id: id.to_owned(),
            trace_id: Some("trace-test".to_owned()),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_owned()),
            instance_id: Some("instance".to_owned()),
            session_id: Some("session".to_owned()),
            transport: None,
            agent_id: None,
            agent_name: None,
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: "maya.abcdef01.create_sphere".to_owned(),
            dcc_type: Some("maya".to_owned()),
            success: true,
            error: None,
            duration_ms: Some(7),
            token_accounting: None,
            llm_usage: None,
        }
    }

    fn token_telemetry() -> TokenTelemetry {
        TokenTelemetry {
            response_format: "toon".to_owned(),
            token_estimator: "dcc-mcp-byte4-v1".to_owned(),
            original_bytes: 400,
            returned_bytes: 160,
            original_tokens: 100,
            returned_tokens: 40,
            saved_tokens: 60,
            savings_pct: 60.0,
        }
    }

    #[test]
    fn durable_store_roundtrips_audit_and_traces() {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableAuditStore::new(dir.path(), 10, 10_000_000).unwrap();
        let mut audit = audit_record("req-1");
        audit.token_accounting = Some(token_telemetry());
        store.append_audit(&audit);
        let trace = DispatchTrace {
            request_id: "req-1".to_owned(),
            trace_id: "trace-test".to_owned(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".to_owned(),
            tool_slug: Some("maya.abcdef01.create_sphere".to_owned()),
            instance_id: Some("instance".to_owned()),
            session_id: Some("session".to_owned()),
            dcc_type: Some("maya".to_owned()),
            transport: None,
            agent_context: None,
            started_at: UNIX_EPOCH + Duration::from_millis(1),
            total_ms: 7,
            ok: true,
            spans: Vec::new(),
            input: None,
            output: None,
            token_accounting: Some(token_telemetry()),
            llm_usage: None,
        };
        store.append_trace(&trace);

        let audits = store.load_audit();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].request_id, "req-1");
        assert_eq!(audits[0].dcc_type.as_deref(), Some("maya"));
        assert_eq!(
            audits[0].token_accounting.as_ref().unwrap().saved_tokens,
            60
        );
        let traces = store.load_traces();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].request_id, "req-1");
        assert_eq!(
            traces[0].token_accounting.as_ref().unwrap().response_format,
            "toon"
        );
        let trace_log = TraceLog::new(10);
        trace_log.extend(traces);
        assert_eq!(trace_log.recent(10).len(), 1);
    }

    #[test]
    fn durable_store_trims_old_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableAuditStore::new(dir.path(), 2, 10_000_000).unwrap();
        for id in ["req-1", "req-2", "req-3"] {
            store.append_audit(&audit_record(id));
        }

        let ids: Vec<String> = store
            .load_audit()
            .into_iter()
            .map(|record| record.request_id)
            .collect();
        assert_eq!(ids, vec!["req-2", "req-3"]);
    }

    #[test]
    fn durable_store_trims_when_over_byte_budget() {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableAuditStore::new(dir.path(), 100_000, 800).unwrap();
        for index in 0..40 {
            store.append_audit(&audit_record(&format!("req-{index:03}")));
        }
        let len = fs::metadata(dir.path().join(DurableAuditStore::AUDIT_FILE))
            .unwrap()
            .len();
        assert!(
            len <= 2_000,
            "expected JSONL to shrink under byte budget, got {len} bytes"
        );
    }
}
