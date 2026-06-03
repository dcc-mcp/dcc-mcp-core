//! Audit middleware — records every tool call to a structured log.

use std::sync::Arc;
use std::time::SystemTime;

use super::context::{CallContext, CallResult};
use super::governance::MiddlewareGovernanceControl;
use super::traits::{AfterCallMiddleware, BeforeCallMiddleware, MiddlewareFuture};
use crate::gateway::admin::trace::{
    AgentContext, LlmUsage, TokenTelemetry, TraceContext, TracePayload, TraceSpan,
};

/// A single audit record produced for each tool call.
///
/// One entry is emitted per `tools/call` after the dispatch handler
/// has resolved the tool and produced a result. The fields mirror
/// [`super::CallContext`] plus outcome metadata so downstream sinks
/// (the admin Calls tab, structured `tracing::info!` logs, custom
/// SIEM forwarders) can render a complete one-row view of the call.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// Wall-clock timestamp at which the call entered the handler.
    pub started_at: SystemTime,
    /// Wall-clock timestamp at which the audit record was sealed —
    /// `SystemTime::now()` from the after-call hook, *not* the call
    /// start (that is reflected via `duration_ms`).
    pub timestamp: SystemTime,
    /// JSON-RPC method name (e.g. `"tools/call"`), copied verbatim
    /// from the originating request.
    pub method: String,
    /// Resolved capability slug, if the dispatch handler matched the
    /// call to a `CapabilityRecord`. `None` for unresolved tools.
    pub tool_slug: Option<String>,
    /// DCC type targeted by the call (`"maya"`, `"blender"`, …),
    /// `None` for gateway-local tools.
    pub dcc_type: Option<String>,
    /// Stringified instance UUID of the resolved backend.
    pub instance_id: Option<String>,
    /// Originating MCP `Mcp-Session-Id` header, if any.
    pub session_id: Option<String>,
    /// Transport surface that produced the request (`mcp`, `rest`, ...).
    pub transport: Option<String>,
    /// Optional client-supplied agent/caller context for admin telemetry.
    pub agent_context: Option<AgentContext>,
    /// Stable request id used to correlate this entry with traces
    /// and the client's JSON-RPC `id`.
    pub request_id: String,
    /// End-to-end trace context for this call.
    pub trace_context: TraceContext,
    /// `true` when the dispatch handler returned an MCP-level error
    /// (`isError == true` or transport failure).
    pub is_error: bool,
    /// First 256 chars of the response text — bounded so audit logs
    /// stay cheap to ship and never leak full payloads. The full
    /// response lives in `output_payload`.
    pub result_preview: String,
    /// Total handler latency in milliseconds, derived from the
    /// `audit.start_time_ns` metadata stamped by the before-call
    /// hook. `None` when the before-call hook did not run (e.g. the
    /// chain was short-circuited).
    pub duration_ms: Option<u64>,
    /// Phase 2: waterfall of timing spans collected by the handler.
    pub trace_spans: Vec<TraceSpan>,
    /// Phase 2: captured input payload after before-middleware redaction.
    pub input_payload: Option<TracePayload>,
    /// Phase 2: captured output payload (bounded).
    pub output_payload: Option<TracePayload>,
    /// Token accounting for the client-visible response, when known.
    pub token_accounting: Option<TokenTelemetry>,
    /// Optional upstream LLM billing token counts, when supplied.
    pub llm_usage: Option<LlmUsage>,
}

/// Sink that receives completed [`AuditEntry`] records.
///
/// Implementations are wrapped in `Arc<dyn AuditSink>` and shared
/// across every concurrent call, so they must be cheap to clone the
/// trait object reference and free of synchronous I/O on the hot
/// path. The default [`DefaultAuditSink`] just emits a `tracing::info!`
/// log; the admin UI ships
/// [`AdminAuditSink`](crate::gateway::admin::AdminAuditSink) which
/// fans entries into a bounded ring buffer.
pub trait AuditSink: Send + Sync {
    /// Persist `entry`. Called from the after-call middleware hook;
    /// the implementation must not block — long-running sinks should
    /// hand off to a background task.
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
            transport     = ?entry.transport,
            agent_id      = ?entry.agent_context.as_ref().and_then(|ctx| ctx.agent_id.as_deref()),
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
    /// Build an `AuditMiddleware` that hands every entry to `sink`.
    /// Use this when the operator has a custom sink (admin ring
    /// buffer, SIEM forwarder, …) — pass [`Arc::new(MySink)`] to
    /// share the sink across the chain.
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink }
    }
    /// Convenience for tests and minimal embeddings: build an
    /// `AuditMiddleware` backed by [`DefaultAuditSink`], which logs
    /// each entry via `tracing::info!`.
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

    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        Some(MiddlewareGovernanceControl::new(
            "audit",
            "observe",
            "Stamps request timing metadata before dispatch.",
        ))
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
            started_at: ctx.started_at,
            timestamp: SystemTime::now(),
            method: ctx.method.clone(),
            tool_slug: ctx.tool_slug.clone(),
            dcc_type: ctx.dcc_type.clone(),
            instance_id: ctx.instance_id.clone(),
            session_id: ctx.session_id.clone(),
            transport: ctx.transport.clone(),
            agent_context: ctx.agent_context.clone(),
            request_id: ctx.request_id.clone(),
            trace_context: ctx.trace_context.clone(),
            is_error: result.is_error,
            result_preview: result.text.chars().take(256).collect(),
            duration_ms,
            trace_spans: ctx.trace_spans.clone(),
            input_payload: ctx.input_payload.clone(),
            output_payload: ctx.output_payload.clone(),
            token_accounting: ctx.token_accounting.clone(),
            llm_usage: ctx.llm_usage.as_ref().and_then(|v| {
                let prompt = v.get("prompt_tokens").and_then(|v| v.as_u64());
                let completion = v.get("completion_tokens").and_then(|v| v.as_u64());
                let total = v.get("total_tokens").and_then(|v| v.as_u64());
                let model = v.get("model").and_then(|v| v.as_str()).map(str::to_string);
                if prompt.is_none() && completion.is_none() && total.is_none() {
                    return None;
                }
                Some(LlmUsage {
                    prompt_tokens: prompt,
                    completion_tokens: completion,
                    total_tokens: total,
                    model,
                })
            }),
        };
        let sink = self.sink.clone();
        Box::pin(async move {
            sink.record(entry);
            Ok(())
        })
    }

    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        Some(MiddlewareGovernanceControl::new(
            "audit",
            "observe",
            "Records bounded request outcome rows for Admin and durable audit sinks.",
        ))
    }
}
