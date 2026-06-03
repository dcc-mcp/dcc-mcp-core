//! Shared state for the admin UI handlers.

use std::fs;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gateway::middleware::{AuditEntry, AuditSink};
use crate::gateway::state::GatewayState;

use super::stats::StatsAggregator;
use super::trace::{AgentContextTrust, DispatchTrace, LlmUsage, TokenTelemetry, TraceLog};

type SqliteTracePersistFn = Arc<dyn Fn(&DispatchTrace) + Send + Sync>;
type SqliteAuditPersistFn = Arc<dyn Fn(&AdminAuditRecord) + Send + Sync>;

/// Minimal audit record that the admin UI consumes.
#[derive(Debug, Clone)]
pub struct AdminAuditRecord {
    /// Wall-clock time when the call completed.
    pub timestamp: SystemTime,
    /// Stable request id used to correlate with traces.
    pub request_id: String,
    /// End-to-end trace id shared by related requests.
    pub trace_id: Option<String>,
    /// Root gateway span id for this request, if known.
    pub span_id: Option<String>,
    /// Incoming parent span id, if known.
    pub parent_span_id: Option<String>,
    /// JSON-RPC / MCP method name.
    pub method: Option<String>,
    /// Target backend instance id, if resolved.
    pub instance_id: Option<String>,
    /// Originating MCP session id, if any.
    pub session_id: Option<String>,
    /// Transport surface that produced the request (`mcp`, `rest`, ...).
    pub transport: Option<String>,
    /// Agent/caller id supplied for telemetry correlation.
    pub agent_id: Option<String>,
    /// Human-readable agent/caller name.
    pub agent_name: Option<String>,
    /// Model or runtime name supplied by the caller.
    pub agent_model: Option<String>,
    /// Human/service actor id supplied for telemetry filtering.
    pub actor_id: Option<String>,
    /// Human/service actor name supplied for telemetry filtering.
    pub actor_name: Option<String>,
    /// Hashed actor email or stable user handle. Never store raw email here.
    pub actor_email_hash: Option<String>,
    /// Client platform/runtime such as `cursor`, `claude-desktop`, or `custom-http`.
    pub client_platform: Option<String>,
    /// Client operating system label.
    pub client_os: Option<String>,
    /// Client host label.
    pub client_host: Option<String>,
    /// Authentication subject when provided by middleware/auth integration.
    pub auth_subject: Option<String>,
    /// Server-derived source IP after proxy trust policy.
    pub source_ip: Option<String>,
    /// Server-computed trust source labels for attribution fields.
    pub attribution_trust: Option<AgentContextTrust>,
    /// Parent request id for request-chain correlation.
    pub parent_request_id: Option<String>,
    /// Tool slug or MCP method name.
    pub action: String,
    /// DCC type of the target backend (e.g. `"maya"`).
    pub dcc_type: Option<String>,
    /// Whether the call succeeded (`true`) or returned an error (`false`).
    pub success: bool,
    /// Error preview when `success == false`; otherwise `None`.
    pub error: Option<String>,
    /// Wall-clock call duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Token accounting for the client-visible response, if available.
    pub token_accounting: Option<TokenTelemetry>,
    /// Optional upstream LLM billing token counts, when supplied.
    pub llm_usage: Option<LlmUsage>,
}

pub type AuditLog = Mutex<Vec<AdminAuditRecord>>;

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
    /// Default on-disk cap for each JSONL file (~50 MiB).
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

    pub fn from_env() -> Option<Self> {
        let dir = std::env::var_os("DCC_MCP_GATEWAY_AUDIT_DIR")?;
        let max_rows = std::env::var("DCC_MCP_GATEWAY_AUDIT_MAX_ROWS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(Self::DEFAULT_MAX_ROWS);
        let max_bytes = std::env::var("DCC_MCP_GATEWAY_AUDIT_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(Self::DEFAULT_MAX_BYTES)
            .max(1024);
        Self::new(dir, max_rows, max_bytes).ok()
    }

    pub fn load_audit(&self) -> Vec<AdminAuditRecord> {
        read_jsonl(&self.path(Self::AUDIT_FILE))
            .into_iter()
            .filter_map(|value| serde_json::from_value::<PersistedAuditRecord>(value).ok())
            .map(AdminAuditRecord::from)
            .collect()
    }

    pub fn load_traces(&self) -> Vec<DispatchTrace> {
        read_jsonl(&self.path(Self::TRACE_FILE))
            .into_iter()
            .filter_map(|value| serde_json::from_value::<DispatchTrace>(value).ok())
            .collect()
    }

    fn append_audit(&self, record: &AdminAuditRecord) {
        let value = json!(PersistedAuditRecord::from(record));
        self.append_value(Self::AUDIT_FILE, &value);
    }

    fn append_trace(&self, trace: &DispatchTrace) {
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

    /// Drop oldest lines until the JSONL file is under `max_bytes`.
    fn enforce_byte_budget(&self, path: &Path) {
        for _ in 0..32 {
            let len = match fs::metadata(path) {
                Ok(m) => m.len(),
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
            let drop = (lines.len() / 2).max(1);
            lines.drain(0..drop);
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

/// [`AuditSink`] that pushes completed entries into the admin UI ring buffer
/// and optionally a [`TraceLog`] for Phase 2 dispatch traces.
pub struct AdminAuditSink {
    log: Arc<AuditLog>,
    capacity: usize,
    trace_log: Option<Arc<TraceLog>>,
    durable_store: Option<DurableAuditStore>,
    sqlite_trace: Option<SqliteTracePersistFn>,
    sqlite_audit: Option<SqliteAuditPersistFn>,
}

impl AdminAuditSink {
    /// Build a sink that pushes audit records into `log`, capped at `capacity`
    /// entries (oldest evicted first).
    pub fn new(log: Arc<AuditLog>, capacity: usize) -> Self {
        Self {
            log,
            capacity,
            trace_log: None,
            durable_store: None,
            sqlite_trace: None,
            sqlite_audit: None,
        }
    }

    /// Attach a durable JSONL store so audit and trace rows survive restarts.
    pub fn with_durable_store(mut self, store: DurableAuditStore) -> Self {
        self.durable_store = Some(store);
        self
    }

    /// Attach a trace log so `record()` also appends a [`DispatchTrace`].
    pub fn with_trace_log(mut self, trace_log: Arc<TraceLog>) -> Self {
        self.trace_log = Some(trace_log);
        self
    }

    /// Persist traces / audits to the admin SQLite lane (bounded `try_send`).
    pub fn with_sqlite_lane(
        mut self,
        lane: crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    ) -> Self {
        let lt = lane.clone();
        self.sqlite_trace = Some(Arc::new(move |t: &DispatchTrace| {
            lt.try_persist_trace(t);
        }));
        self.sqlite_audit = Some(Arc::new(move |r: &AdminAuditRecord| {
            lane.try_persist_audit(r);
        }));
        self
    }
}

impl AuditSink for AdminAuditSink {
    fn record(&self, entry: AuditEntry) {
        let agent_context = entry.agent_context.as_ref();
        let record = AdminAuditRecord {
            timestamp: entry.timestamp,
            request_id: entry.request_id.clone(),
            trace_id: Some(entry.trace_context.trace_id.clone()),
            span_id: entry.trace_context.span_id.clone(),
            parent_span_id: entry.trace_context.parent_span_id.clone(),
            method: Some(entry.method.clone()),
            instance_id: entry.instance_id.clone(),
            session_id: entry.session_id.clone(),
            transport: entry.transport.clone(),
            agent_id: agent_context.and_then(|ctx| ctx.agent_id.clone()),
            agent_name: agent_context.and_then(|ctx| ctx.agent_name.clone()),
            agent_model: agent_context
                .and_then(|ctx| ctx.model.clone().or_else(|| ctx.model_version.clone())),
            actor_id: agent_context.and_then(|ctx| ctx.actor_id.clone()),
            actor_name: agent_context.and_then(|ctx| ctx.actor_name.clone()),
            actor_email_hash: agent_context.and_then(|ctx| ctx.actor_email_hash.clone()),
            client_platform: agent_context.and_then(|ctx| ctx.client_platform.clone()),
            client_os: agent_context.and_then(|ctx| ctx.client_os.clone()),
            client_host: agent_context.and_then(|ctx| ctx.client_host.clone()),
            auth_subject: agent_context.and_then(|ctx| ctx.auth_subject.clone()),
            source_ip: agent_context.and_then(|ctx| ctx.source_ip.clone()),
            attribution_trust: agent_context
                .map(|ctx| ctx.trust.clone())
                .filter(|trust| !trust.is_empty()),
            parent_request_id: entry
                .trace_context
                .parent_request_id
                .clone()
                .or_else(|| agent_context.and_then(|ctx| ctx.parent_request_id.clone())),
            action: entry
                .tool_slug
                .clone()
                .unwrap_or_else(|| entry.method.clone()),
            dcc_type: entry.dcc_type.clone(),
            success: !entry.is_error,
            error: if entry.is_error {
                Some(entry.result_preview.clone())
            } else {
                None
            },
            duration_ms: entry.duration_ms,
            token_accounting: entry.token_accounting.clone(),
            llm_usage: entry.llm_usage.clone(),
        };
        if let Some(store) = &self.durable_store {
            store.append_audit(&record);
        }
        if let Some(cb) = &self.sqlite_audit {
            cb(&record);
        }
        let mut buf = self.log.lock();
        buf.push(record);
        if self.capacity > 0 {
            while buf.len() > self.capacity {
                buf.remove(0);
            }
        }

        // Phase 2: promote AuditEntry into a DispatchTrace when a trace log is attached.
        if let Some(tl) = &self.trace_log {
            let trace = DispatchTrace {
                request_id: entry.request_id.clone(),
                trace_id: entry.trace_context.trace_id.clone(),
                span_id: entry.trace_context.span_id.clone(),
                parent_span_id: entry.trace_context.parent_span_id.clone(),
                parent_request_id: entry.trace_context.parent_request_id.clone(),
                trace_flags: entry.trace_context.trace_flags.clone(),
                trace_state: entry.trace_context.trace_state.clone(),
                method: entry.method.clone(),
                tool_slug: entry.tool_slug.clone(),
                instance_id: entry.instance_id.clone(),
                session_id: entry.session_id.clone(),
                dcc_type: entry.dcc_type.clone(),
                transport: entry.transport.clone(),
                agent_context: entry.agent_context.clone(),
                started_at: entry.started_at,
                total_ms: entry.duration_ms.unwrap_or(0),
                ok: !entry.is_error,
                spans: entry.trace_spans,
                input: entry.input_payload,
                output: entry.output_payload,
                token_accounting: entry.token_accounting,
                llm_usage: entry.llm_usage.clone(),
            };
            if let Some(store) = &self.durable_store {
                store.append_trace(&trace);
            }
            if let Some(cb) = &self.sqlite_trace {
                cb(&trace);
            }
            tl.push(trace);
        }
    }
}

#[cfg(test)]
mod durable_tests {
    use super::*;

    fn audit_record(id: &str) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp: UNIX_EPOCH + Duration::from_millis(1),
            request_id: id.to_string(),
            trace_id: Some("trace-test".to_string()),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: Some("instance".to_string()),
            session_id: Some("session".to_string()),
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
            action: "maya.abcdef01.create_sphere".to_string(),
            dcc_type: Some("maya".to_string()),
            success: true,
            error: None,
            duration_ms: Some(7),
            token_accounting: None,
            llm_usage: None,
        }
    }

    fn token_telemetry() -> TokenTelemetry {
        TokenTelemetry {
            response_format: "toon".to_string(),
            token_estimator: "dcc-mcp-byte4-v1".to_string(),
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
            request_id: "req-1".to_string(),
            trace_id: "trace-test".to_string(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".to_string(),
            tool_slug: Some("maya.abcdef01.create_sphere".to_string()),
            instance_id: Some("instance".to_string()),
            session_id: Some("session".to_string()),
            dcc_type: Some("maya".to_string()),
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
        let trace_log = Arc::new(TraceLog::new(10));
        trace_log.extend(traces);
        let stats = crate::gateway::admin::stats::StatsAggregator::new(trace_log)
            .compute(crate::gateway::admin::stats::StatsRange::All);
        assert_eq!(stats.token_usage.total_saved_tokens, 60);
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
        for i in 0..40 {
            store.append_audit(&audit_record(&format!("req-{i:03}")));
        }
        let path = dir.path().join(DurableAuditStore::AUDIT_FILE);
        let len = fs::metadata(&path).unwrap().len();
        assert!(
            len <= 2000,
            "expected JSONL to shrink under byte budget, got {len} bytes"
        );
    }
}

/// State injected into every admin handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AdminState {
    /// Live gateway state — registry, capability index, server metadata.
    pub gateway: GatewayState,
    /// Audit log ring buffer — `None` until `with_audit_log` is called.
    pub audit_log: Option<Arc<AuditLog>>,
    /// Phase 2 trace log — `None` until `with_trace_log` is called.
    pub trace_log: Option<Arc<TraceLog>>,
    /// Phase 3 stats aggregator — `None` until `with_trace_log` is called.
    pub stats: Option<Arc<StatsAggregator>>,
    /// Wall-clock time the gateway started, used for the Health card uptime.
    pub started_at: SystemTime,
    /// Skill search path snapshot (CLI / env / bundled) for the admin UI.
    pub skill_paths_snapshot: Vec<crate::gateway::SkillPathEntry>,
    /// SQLite lane for custom skill path mutations from the admin API.
    pub admin_sqlite_lane: Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    /// Optional embedder hook: re-run disk skill discovery after admin SQLite path changes.
    pub skill_paths_reload: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl AdminState {
    /// Build an [`AdminState`] backed by the live `GatewayState`. Audit /
    /// trace / stats logs default to `None`; attach them via the
    /// `with_*` builders before mounting the admin router.
    pub fn new(gateway: GatewayState) -> Self {
        Self {
            gateway,
            audit_log: None,
            trace_log: None,
            stats: None,
            started_at: SystemTime::now(),
            skill_paths_snapshot: Vec::new(),
            admin_sqlite_lane: None,
            skill_paths_reload: None,
        }
    }

    /// Attach the [`AuditLog`] that `GET /admin/api/calls` reads from.
    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    /// Attach the Phase 2 [`TraceLog`]. Implicitly bootstraps a
    /// [`StatsAggregator`] (Phase 3) over the same log so the admin
    /// router can serve `GET /admin/api/stats` without extra wiring.
    pub fn with_trace_log(
        mut self,
        log: Arc<TraceLog>,
        sqlite_reader: Option<crate::gateway::admin::sqlite_lane::AdminSqliteReader>,
    ) -> Self {
        let mut agg = StatsAggregator::new(log.clone());
        if let Some(r) = sqlite_reader {
            agg = agg.with_sqlite_reader(r);
        }
        self.stats = Some(Arc::new(agg));
        self.trace_log = Some(log);
        self
    }

    /// Attach skill path snapshot rows (from CLI / env / bundled).
    pub fn with_skill_paths_snapshot(mut self, paths: Vec<crate::gateway::SkillPathEntry>) -> Self {
        self.skill_paths_snapshot = paths;
        self
    }

    /// Attach SQLite lane for admin API skill-path mutations.
    pub fn with_admin_sqlite_lane(
        mut self,
        lane: Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    ) -> Self {
        self.admin_sqlite_lane = lane;
        self
    }

    /// Hook invoked after SQLite-backed custom skill paths change (add/delete).
    pub fn with_skill_paths_reload(
        mut self,
        cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        self.skill_paths_reload = cb;
        self
    }
}
