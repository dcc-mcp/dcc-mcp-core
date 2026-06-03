//! CallContext and CallResult — data passed through the middleware chain.

use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;

use crate::gateway::admin::trace::{
    AgentContext, TokenTelemetry, TraceContext, TracePayload, TraceSpan,
};

/// Context for one gateway `tools/call` invocation.
///
/// Built by the gateway dispatch handler and passed through the
/// [`super::MiddlewareChain`]. The context carries everything a
/// before/after middleware needs to make routing, auditing, or
/// admission decisions: the JSON-RPC method, the resolved tool
/// (slug + DCC + instance), the originating session, and the raw
/// argument value.
///
/// Middlewares may mutate `metadata` to pass small key/value hints to
/// later stages, but the routing fields (`tool_slug`, `dcc_type`,
/// `instance_id`) are owned by the dispatch handler and should not be
/// rewritten by middleware.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// JSON-RPC method name as it arrived from the client (e.g.
    /// `"tools/call"`). Preserved verbatim so audit sinks can
    /// distinguish the dynamic-capability verbs from the legacy
    /// `tools/list` fan-out.
    pub method: String,
    /// Resolved capability slug (`<dcc>.<id8>.<tool>`) once the
    /// dispatch handler has matched the call to a `CapabilityRecord`.
    /// `None` for calls that never resolve (e.g. unknown tool).
    pub tool_slug: Option<String>,
    /// DCC type of the resolved capability (e.g. `"maya"`). `None`
    /// when the call does not target a backend (gateway-local tool).
    pub dcc_type: Option<String>,
    /// Stringified instance UUID of the resolved backend, if any.
    pub instance_id: Option<String>,
    /// MCP `Mcp-Session-Id` header. `None` for stateless callers
    /// (REST clients) that did not negotiate a session.
    pub session_id: Option<String>,
    /// Stable per-call identifier used to correlate trace spans,
    /// audit records, and the client's JSON-RPC `id`.
    pub request_id: String,
    /// End-to-end trace context propagated across gateway and backend hops.
    pub trace_context: TraceContext,
    /// Raw argument value as received from the client. Middlewares
    /// must treat this as untrusted input and avoid logging it
    /// verbatim — use [`input_payload`](Self::input_payload) which
    /// has already been bounded and redacted.
    pub args: Value,
    /// Free-form metadata carried between middleware stages. Use a
    /// short, stable key (e.g. `"admission.reason"`) when emitting
    /// audit signals.
    pub metadata: HashMap<String, String>,
    /// Phase 2: per-call dispatch trace spans, populated by the handler.
    pub trace_spans: Vec<TraceSpan>,
    /// Phase 2: captured input payload after before-middleware redaction.
    pub input_payload: Option<TracePayload>,
    /// Phase 2: captured output payload (response content).
    pub output_payload: Option<TracePayload>,
    /// Token accounting for the response that will be visible to the client.
    pub token_accounting: Option<TokenTelemetry>,
    /// Phase 2: wall-clock timestamp when the call entered the handler.
    pub started_at: SystemTime,
    /// Optional client-supplied agent/caller context for admin telemetry.
    pub agent_context: Option<AgentContext>,
    /// Transport surface that produced this call (`mcp`, `rest`, ...).
    pub transport: Option<String>,
    /// Optional upstream LLM billing token counts from `x-dcc-mcp-llm-usage`.
    pub llm_usage: Option<Value>,
}

impl CallContext {
    /// Construct a fresh `CallContext` for the given method and
    /// request id, capturing `args` verbatim.
    ///
    /// All optional routing fields default to `None`; the dispatch
    /// handler is expected to populate them as it resolves the tool.
    /// `started_at` is set to `SystemTime::now()` so the trace
    /// waterfall measures the full handler latency.
    pub fn new(method: impl Into<String>, request_id: impl Into<String>, args: Value) -> Self {
        let request_id = request_id.into();
        Self {
            method: method.into(),
            tool_slug: None,
            dcc_type: None,
            instance_id: None,
            session_id: None,
            request_id: request_id.clone(),
            trace_context: TraceContext {
                trace_id: uuid::Uuid::new_v4().simple().to_string(),
                request_id,
                span_id: Some(
                    uuid::Uuid::new_v4()
                        .simple()
                        .to_string()
                        .chars()
                        .take(16)
                        .collect(),
                ),
                parent_span_id: None,
                parent_request_id: None,
                trace_flags: Some("00".to_string()),
                trace_state: None,
            },
            args,
            metadata: HashMap::new(),
            trace_spans: Vec::new(),
            input_payload: None,
            output_payload: None,
            token_accounting: None,
            llm_usage: None,
            started_at: SystemTime::now(),
            agent_context: None,
            transport: None,
        }
    }

    /// Builder: attach the resolved capability slug.
    pub fn with_tool_slug(mut self, slug: impl Into<String>) -> Self {
        self.tool_slug = Some(slug.into());
        self
    }

    /// Builder: attach resolved backend routing metadata.
    pub fn with_backend(
        mut self,
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        self.dcc_type = Some(dcc_type.into());
        self.instance_id = Some(instance_id.into());
        self
    }

    /// Builder: attach the originating MCP session id.
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Builder: attach optional agent/caller telemetry context.
    pub fn with_agent_context(mut self, context: Option<AgentContext>) -> Self {
        if self.session_id.is_none() {
            self.session_id = context.as_ref().and_then(|ctx| ctx.session_id.clone());
        }
        self.agent_context = context;
        self
    }

    /// Builder: attach trace context parsed from request headers or JSON-RPC id.
    pub fn with_trace_context(mut self, context: TraceContext) -> Self {
        self.request_id = context.request_id.clone();
        if let Some(agent_context) = &mut self.agent_context {
            if agent_context.trace_id.is_none() {
                agent_context.trace_id = Some(context.trace_id.clone());
            }
            if agent_context.parent_request_id.is_none() {
                agent_context.parent_request_id = context.parent_request_id.clone();
            }
        }
        self.trace_context = context;
        self
    }

    /// Builder: attach the transport surface.
    pub fn with_transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    /// Phase 2: append a timing span to the trace waterfall.
    pub fn push_span(&mut self, span: TraceSpan) {
        self.trace_spans.push(span);
    }
}

/// Result of a gateway tool call, passed to [`super::AfterCallMiddleware`].
///
/// `text` mirrors the MCP `content[0].text` field of the response and
/// `is_error` mirrors the JSON-RPC `isError` flag — together they let
/// audit sinks render a one-line outcome without re-parsing the
/// response envelope.
#[derive(Debug, Clone)]
pub struct CallResult {
    /// Human-readable response body. Already bounded by the dispatch
    /// handler so audit sinks can persist it without an extra cap.
    pub text: String,
    /// `true` when the response was an MCP-level error
    /// (`isError == true` or transport failure). Used by audit sinks
    /// to colour-code outcomes without re-parsing `text`.
    pub is_error: bool,
}

impl CallResult {
    /// Construct a `CallResult` from the handler's `(text, is_error)`
    /// tuple — the canonical shape backends return today.
    pub fn from_tuple(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            is_error,
        }
    }

    /// Convert back to the `(text, is_error)` tuple that the dispatch
    /// handler ultimately serialises into the JSON-RPC response.
    pub fn into_tuple(self) -> (String, bool) {
        (self.text, self.is_error)
    }
}
