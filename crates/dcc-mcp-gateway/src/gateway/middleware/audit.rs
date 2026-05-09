//! Audit middleware — records every tool call to a structured log.

use std::sync::Arc;
use std::time::SystemTime;

use super::context::{CallContext, CallResult};
use super::traits::{AfterCallMiddleware, BeforeCallMiddleware, MiddlewareFuture};
use crate::gateway::admin::trace::{TracePayload, TraceSpan};

/// A single audit record produced for each tool call.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: SystemTime,
    pub method: String,
    pub tool_slug: Option<String>,
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub request_id: String,
    pub is_error: bool,
    pub result_preview: String,
    pub duration_ms: Option<u64>,
    /// Phase 2: waterfall of timing spans collected by the handler.
    pub trace_spans: Vec<TraceSpan>,
    /// Phase 2: captured input payload (bounded, pre-redacted).
    pub input_payload: Option<TracePayload>,
    /// Phase 2: captured output payload (bounded, pre-redacted).
    pub output_payload: Option<TracePayload>,
}

/// Sink that receives completed [`AuditEntry`] records.
pub trait AuditSink: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// Default sink — emits one structured `tracing::info!` log per call.
#[derive(Debug, Default)]
pub struct DefaultAuditSink;

impl AuditSink for DefaultAuditSink {
    fn record(&self, entry: AuditEntry) {
        tracing::info!(
            method        = %entry.method,
            tool_slug     = ?entry.tool_slug,
            dcc_type      = ?entry.dcc_type,
            instance_id   = ?entry.instance_id,
            session_id    = ?entry.session_id,
            request_id    = %entry.request_id,
            is_error      = entry.is_error,
            result_preview = %entry.result_preview,
            "gateway audit"
        );
    }
}

/// Middleware that records each call via an [`AuditSink`].
pub struct AuditMiddleware {
    sink: Arc<dyn AuditSink>,
}

impl Default for AuditMiddleware {
    fn default() -> Self {
        Self {
            sink: Arc::new(DefaultAuditSink),
        }
    }
}

impl AuditMiddleware {
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink }
    }
    pub fn with_default_sink() -> Self {
        Self::default()
    }
}

impl BeforeCallMiddleware for AuditMiddleware {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
        Box::pin(async move {
            let ns = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            ctx.metadata
                .insert("audit.start_time_ns".to_string(), ns.to_string());
            Ok(())
        })
    }
}

impl AfterCallMiddleware for AuditMiddleware {
    fn after_call<'a>(
        &'a self,
        ctx: &'a CallContext,
        result: &'a mut CallResult,
    ) -> MiddlewareFuture<'a, ()> {
        let duration_ms = ctx
            .metadata
            .get("audit.start_time_ns")
            .and_then(|s| s.parse::<u128>().ok())
            .and_then(|start_ns| {
                let now_ns = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos();
                Some(((now_ns.saturating_sub(start_ns)) / 1_000_000) as u64)
            });
        let entry = AuditEntry {
            timestamp: SystemTime::now(),
            method: ctx.method.clone(),
            tool_slug: ctx.tool_slug.clone(),
            dcc_type: ctx.dcc_type.clone(),
            instance_id: ctx.instance_id.clone(),
            session_id: ctx.session_id.clone(),
            request_id: ctx.request_id.clone(),
            is_error: result.is_error,
            result_preview: result.text.chars().take(256).collect(),
            duration_ms,
            trace_spans: ctx.trace_spans.clone(),
            input_payload: ctx.input_payload.clone(),
            output_payload: ctx.output_payload.clone(),
        };
        let sink = self.sink.clone();
        Box::pin(async move {
            sink.record(entry);
            Ok(())
        })
    }
}
