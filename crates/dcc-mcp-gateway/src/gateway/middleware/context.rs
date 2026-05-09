//! CallContext and CallResult — data passed through the middleware chain.

use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;

use crate::gateway::admin::trace::{TracePayload, TraceSpan};

/// Context for one gateway `tools/call` invocation.
#[derive(Debug, Clone)]
pub struct CallContext {
    pub method: String,
    pub tool_slug: Option<String>,
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub request_id: String,
    pub args: Value,
    pub metadata: HashMap<String, String>,
    /// Phase 2: per-call dispatch trace spans, populated by the handler.
    pub trace_spans: Vec<TraceSpan>,
    /// Phase 2: captured input payload (args, bounded and pre-redacted).
    pub input_payload: Option<TracePayload>,
    /// Phase 2: captured output payload (response content).
    pub output_payload: Option<TracePayload>,
    /// Phase 2: wall-clock timestamp when the call entered the handler.
    pub started_at: SystemTime,
}

impl CallContext {
    pub fn new(method: impl Into<String>, request_id: impl Into<String>, args: Value) -> Self {
        Self {
            method: method.into(),
            tool_slug: None,
            dcc_type: None,
            instance_id: None,
            session_id: None,
            request_id: request_id.into(),
            args,
            metadata: HashMap::new(),
            trace_spans: Vec::new(),
            input_payload: None,
            output_payload: None,
            started_at: SystemTime::now(),
        }
    }

    pub fn with_tool_slug(mut self, slug: impl Into<String>) -> Self {
        self.tool_slug = Some(slug.into());
        self
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Phase 2: append a timing span to the trace waterfall.
    pub fn push_span(&mut self, span: TraceSpan) {
        self.trace_spans.push(span);
    }
}

/// Result of a gateway tool call, passed to [`super::AfterCallMiddleware`].
#[derive(Debug, Clone)]
pub struct CallResult {
    pub text: String,
    pub is_error: bool,
}

impl CallResult {
    pub fn from_tuple(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            is_error,
        }
    }

    pub fn into_tuple(self) -> (String, bool) {
        (self.text, self.is_error)
    }
}
