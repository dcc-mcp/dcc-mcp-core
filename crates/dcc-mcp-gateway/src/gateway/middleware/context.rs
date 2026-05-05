//! CallContext and CallResult — data passed through the middleware chain.

use serde_json::Value;
use std::collections::HashMap;

/// Context for one gateway `tools/call` invocation.
///
/// Passed (mutably) through every [`super::BeforeCallMiddleware`] before the
/// call is dispatched, and available (read-only alongside `CallResult`) to
/// every [`super::AfterCallMiddleware`] afterwards.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// MCP method name, e.g. `"tools/call"`.
    pub method: String,
    /// Gateway tool name or slug (e.g. `"call_tool"`, `"list_skills"`).
    pub tool_slug: Option<String>,
    /// DCC type of the target backend (e.g. `"maya"`, `"blender"`).
    pub dcc_type: Option<String>,
    /// Instance ID of the target backend.
    pub instance_id: Option<String>,
    /// MCP session identifier (from `Mcp-Session-Id` header).
    pub session_id: Option<String>,
    /// Unique request identifier (JSON-RPC `id` serialised to string).
    pub request_id: String,
    /// Tool arguments. Middlewares may inspect or redact fields in-place.
    pub args: Value,
    /// Free-form key-value store for middleware-to-middleware communication.
    ///
    /// Middlewares can stash data here (e.g. a quota bucket key, a trace ID)
    /// and read it back in subsequent middlewares without coupling to each other.
    pub metadata: HashMap<String, String>,
}

impl CallContext {
    /// Create a minimal context for the given method and request ID.
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
        }
    }

    /// Convenience builder — sets `tool_slug`.
    pub fn with_tool_slug(mut self, slug: impl Into<String>) -> Self {
        self.tool_slug = Some(slug.into());
        self
    }

    /// Convenience builder — sets `session_id`.
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }
}

/// Result of a gateway tool call, passed to [`super::AfterCallMiddleware`].
#[derive(Debug, Clone)]
pub struct CallResult {
    /// Text body of the response (`CallToolResult.content[0].text`).
    pub text: String,
    /// Whether the result represents an error (`CallToolResult.isError`).
    pub is_error: bool,
}

impl CallResult {
    /// Construct from the `(text, is_error)` tuple returned by `route_tools_call`.
    pub fn from_tuple(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            is_error,
        }
    }

    /// Destructure back into the `(text, is_error)` form used by the gateway.
    pub fn into_tuple(self) -> (String, bool) {
        (self.text, self.is_error)
    }
}
