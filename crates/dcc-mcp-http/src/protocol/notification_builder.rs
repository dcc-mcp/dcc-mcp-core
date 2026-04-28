//! Builders for JSON-RPC 2.0 notification and request envelopes (#484).
//!
//! Six+ call sites previously hand-rolled
//! `json!({"jsonrpc":"2.0","method":"…","params":{…}})` envelopes. Drift in
//! that shape (a new top-level field, a protocol version bump, …) used to
//! require touching every site.
//!
//! These builders consolidate envelope construction so the *only* place
//! that knows the wire shape is this module:
//!
//! ```ignore
//! use crate::protocol::NotificationBuilder;
//!
//! let sse_frame = NotificationBuilder::new("notifications/tools/list_changed")
//!     .with_params(serde_json::json!({}))
//!     .as_sse_event();
//! ```
//!
//! `JsonRpcRequestBuilder` is the symmetric helper used by the gateway
//! backend client to build *requests* (with an `id`) instead of fire-and-
//! forget notifications.

use serde_json::Value;

use super::JsonRpcNotification;
use super::format_sse_event;

/// Fluent builder for a [`JsonRpcNotification`] envelope.
///
/// Use [`Self::build`] to obtain the typed notification, [`Self::to_value`]
/// to obtain the raw JSON value, or [`Self::as_sse_event`] to obtain a
/// fully-formatted SSE frame ready to push onto a session's event stream.
#[derive(Debug, Clone)]
pub struct NotificationBuilder {
    method: String,
    params: Option<Value>,
}

impl NotificationBuilder {
    /// Create a new notification with the given JSON-RPC `method`.
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            params: None,
        }
    }

    /// Attach a `params` payload to the notification.
    #[must_use]
    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }

    /// Attach an empty (`{}`) `params` object.
    ///
    /// Required by MCP for `notifications/{tools,prompts,resources}/list_changed`
    /// which the spec defines with an empty params object rather than no
    /// `params` field at all.
    #[must_use]
    pub fn with_empty_params(mut self) -> Self {
        self.params = Some(Value::Object(serde_json::Map::new()));
        self
    }

    /// Consume the builder and return a typed [`JsonRpcNotification`].
    pub fn build(self) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: self.method,
            params: self.params,
        }
    }

    /// Consume the builder and return the raw JSON value of the envelope.
    ///
    /// Equivalent to `serde_json::to_value(&self.build()).unwrap()`, kept
    /// as a dedicated helper because most call sites only need the value
    /// to feed into [`format_sse_event`].
    pub fn to_value(self) -> Value {
        serde_json::to_value(self.build()).unwrap_or(Value::Null)
    }

    /// Consume the builder and return an SSE frame string ready to push
    /// onto a session's event stream.
    ///
    /// The SSE event has no `id:` line — channel-specific event ids are
    /// not used by any current notification site.
    pub fn as_sse_event(self) -> String {
        format_sse_event(&self.build(), None)
    }
}

/// Fluent builder for a JSON-RPC 2.0 *request* envelope (with `id`).
///
/// Used by the gateway backend client to construct outgoing requests
/// without re-spelling the envelope shape inline.
#[derive(Debug, Clone)]
pub struct JsonRpcRequestBuilder {
    id: Value,
    method: String,
    params: Option<Value>,
}

impl JsonRpcRequestBuilder {
    /// Create a new request with the given `id` and JSON-RPC `method`.
    pub fn new(id: impl Into<Value>, method: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            params: None,
        }
    }

    /// Attach a `params` payload to the request.
    #[must_use]
    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }

    /// Attach an *optional* `params` payload — convenience for callers
    /// that already carry an `Option<Value>` and want to forward it.
    #[must_use]
    pub fn with_optional_params(mut self, params: Option<Value>) -> Self {
        self.params = params;
        self
    }

    /// Consume the builder and return the raw JSON value of the envelope.
    pub fn to_value(self) -> Value {
        let mut obj = serde_json::Map::with_capacity(4);
        obj.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
        obj.insert("id".to_string(), self.id);
        obj.insert("method".to_string(), Value::String(self.method));
        if let Some(p) = self.params {
            obj.insert("params".to_string(), p);
        }
        Value::Object(obj)
    }

    /// Consume the builder and return an SSE frame string ready to push
    /// onto a session's event stream.
    pub fn as_sse_event(self) -> String {
        format_sse_event(&self.to_value(), None)
    }
}
