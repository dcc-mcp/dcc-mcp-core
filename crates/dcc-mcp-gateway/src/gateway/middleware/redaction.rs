//! Redaction middleware — masks sensitive fields in `CallContext.args`.

use std::collections::HashSet;

use serde_json::Value;

use super::context::CallContext;
use super::error::MiddlewareError;
use super::traits::{BeforeCallMiddleware, MiddlewareFuture};

const REDACTED: &str = "[REDACTED]";

/// Middleware that replaces the value of matching keys anywhere in `ctx.args`
/// with `"[REDACTED]"`.
///
/// Matching is case-sensitive and applies recursively through nested objects
/// and arrays.
///
/// # Example
///
/// ```rust,ignore
/// let m = RedactionMiddleware::new(vec!["api_key", "token", "password"]);
/// ```
pub struct RedactionMiddleware {
    fields: HashSet<String>,
}

impl RedactionMiddleware {
    /// Create a new middleware that redacts the given field names.
    pub fn new(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            fields: fields.into_iter().map(|f| f.into()).collect(),
        }
    }

    fn redact_value(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, val) in map.iter_mut() {
                    if self.fields.contains(key) {
                        *val = Value::String(REDACTED.to_string());
                    } else {
                        self.redact_value(val);
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    self.redact_value(item);
                }
            }
            _ => {}
        }
    }
}

impl BeforeCallMiddleware for RedactionMiddleware {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
        self.redact_value(&mut ctx.args);
        Box::pin(async move { Ok::<(), MiddlewareError>(()) })
    }
}
