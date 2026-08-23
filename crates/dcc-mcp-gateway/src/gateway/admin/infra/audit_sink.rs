//! Gateway middleware adapter for admin audit and trace persistence.

use std::sync::Arc;

use crate::gateway::middleware::{AuditEntry, AuditSink};

use super::super::sqlite_lane::AdminSqliteLane;
use super::super::state::{AdminAuditRecord, AuditLog, DurableAuditStore};
use super::super::trace::{DispatchTrace, TraceLog};

type SqliteTracePersistFn = Arc<dyn Fn(&DispatchTrace) + Send + Sync>;
type SqliteAuditPersistFn = Arc<dyn Fn(&AdminAuditRecord) + Send + Sync>;

/// [`AuditSink`] that pushes completed entries into the admin UI ring buffer
/// and optionally a [`TraceLog`] for dispatch traces.
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

    /// Persist traces and audits to the admin SQLite lane using bounded sends.
    pub fn with_sqlite_lane(mut self, lane: AdminSqliteLane) -> Self {
        let trace_lane = lane.clone();
        self.sqlite_trace = Some(Arc::new(move |trace: &DispatchTrace| {
            trace_lane.try_persist_trace(trace);
        }));
        self.sqlite_audit = Some(Arc::new(move |record: &AdminAuditRecord| {
            lane.try_persist_audit(record);
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
            error: entry.is_error.then(|| entry.result_preview.clone()),
            duration_ms: entry.duration_ms,
            token_accounting: entry.token_accounting.clone(),
            llm_usage: entry.llm_usage.clone(),
        };
        if let Some(store) = &self.durable_store {
            store.append_audit(&record);
        }
        if let Some(persist) = &self.sqlite_audit {
            persist(&record);
        }
        let mut buffer = self.log.lock();
        buffer.push(record);
        if self.capacity > 0 {
            while buffer.len() > self.capacity {
                buffer.remove(0);
            }
        }

        if let Some(trace_log) = &self.trace_log {
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
                llm_usage: entry.llm_usage,
            };
            if let Some(store) = &self.durable_store {
                store.append_trace(&trace);
            }
            if let Some(persist) = &self.sqlite_trace {
                persist(&trace);
            }
            trace_log.push(trace);
        }
    }
}
