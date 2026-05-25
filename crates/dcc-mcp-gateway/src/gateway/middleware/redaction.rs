//! Redaction middleware — masks sensitive fields in `CallContext.args`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::context::CallContext;
use super::error::MiddlewareError;
use super::governance::MiddlewareGovernanceControl;
use super::traits::{BeforeCallMiddleware, MiddlewareFuture};
use serde_json::json;

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
    redacted_total: AtomicU64,
}

impl RedactionMiddleware {
    /// Create a new middleware that redacts the given field names.
    pub fn new(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            fields: fields.into_iter().map(|f| f.into()).collect(),
            redacted_total: AtomicU64::new(0),
        }
    }

    fn redact_value(&self, value: &mut Value) -> u64 {
        match value {
            Value::Object(map) => {
                let mut count = 0;
                for (key, val) in map.iter_mut() {
                    if self.fields.contains(key) {
                        *val = Value::String(REDACTED.to_string());
                        count += 1;
                    } else {
                        count += self.redact_value(val);
                    }
                }
                count
            }
            Value::Array(arr) => {
                let mut count = 0;
                for item in arr.iter_mut() {
                    count += self.redact_value(item);
                }
                count
            }
            _ => 0,
        }
    }
}

impl BeforeCallMiddleware for RedactionMiddleware {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
        let count = self.redact_value(&mut ctx.args);
        if count > 0 {
            self.redacted_total.fetch_add(count, Ordering::Relaxed);
            ctx.metadata.insert(
                "redaction.redacted_field_count".to_string(),
                count.to_string(),
            );
        }
        Box::pin(async move { Ok::<(), MiddlewareError>(()) })
    }

    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        let mut fields: Vec<String> = self.fields.iter().cloned().collect();
        fields.sort();
        Some(
            MiddlewareGovernanceControl::new(
                "redaction",
                "mutate",
                format!(
                    "Redacts {} configured field name(s) before dispatch.",
                    fields.len()
                ),
            )
            .with_config(json!({
                "fields": fields,
                "replacement": REDACTED,
                "redacted_total": self.redacted_total.load(Ordering::Relaxed),
            })),
        )
    }
}
