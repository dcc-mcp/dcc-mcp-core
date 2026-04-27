//! ToolResult — unified result type for all tool executions.
//!
//! Plain Rust struct; PyO3 bindings live in `crate::python::action_result`.

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyclass_enum};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// RTK-inspired: limit context depth and array size to reduce token consumption
fn compact_json_value(
    value: &serde_json::Value,
    depth: usize,
    max_depth: usize,
) -> serde_json::Value {
    if depth >= max_depth {
        return serde_json::Value::String("...".to_string());
    }
    match value {
        serde_json::Value::Array(arr) => {
            // Limit array to first 10 elements
            let limited = arr
                .iter()
                .take(10)
                .map(|v| compact_json_value(v, depth + 1, max_depth))
                .collect();
            serde_json::Value::Array(limited)
        }
        serde_json::Value::Object(obj) => {
            // Limit object depth to 3 levels
            let limited = obj
                .iter()
                .take(10)
                .map(|(k, v)| (k.clone(), compact_json_value(v, depth + 1, max_depth)))
                .collect();
            serde_json::Value::Object(limited)
        }
        other => other.clone(),
    }
}

// ── Serialization format ─────────────────────────────────────────────────────

/// Supported serialization formats for `ToolResult`.
///
/// The default is [`SerializeFormat::Json`] (UTF-8 text, human-readable).
/// [`SerializeFormat::MsgPack`] produces compact binary (MessagePack via `rmp-serde`)
/// and is suitable for high-throughput or binary transport scenarios.
///
/// # Future extensibility
/// Additional formats (e.g. CBOR, Bincode) can be added as new variants without
/// breaking the existing API.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SerializeFormat", eq, eq_int, from_py_object)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerializeFormat {
    /// JSON (default): UTF-8 text, human-readable, widely compatible.
    #[default]
    Json,
    /// MessagePack: compact binary encoding via `rmp-serde`.
    MsgPack,
}

/// Internal Rust data representation (serde-friendly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionResultModelData {
    /// Whether the action completed successfully.
    pub success: bool,
    /// Human-readable result or error summary.
    pub message: String,
    /// Optional prompt/hint for the next user action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Optional error message when `success` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Arbitrary key-value context data (e.g. traceback, error_type).
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

// Manual impl: `success` defaults to `true` (unlike `bool::default()` which is `false`),
// matching the Python `ToolResult.__new__` signature.
impl Default for ActionResultModelData {
    fn default() -> Self {
        Self {
            success: true,
            message: String::new(),
            prompt: None,
            error: None,
            context: HashMap::new(),
        }
    }
}

impl ActionResultModelData {
    /// Create a success result with context.
    #[must_use]
    pub fn success(
        message: String,
        prompt: Option<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            success: true,
            message,
            prompt,
            error: None,
            context,
        }
    }

    /// Create a failure result with context.
    #[must_use]
    pub fn failure(
        message: String,
        error: Option<String>,
        prompt: Option<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            success: false,
            message,
            prompt,
            error,
            context,
        }
    }

    /// Serialize to bytes using the specified format.
    ///
    /// Returns `Err(String)` if serialization fails (should never happen for
    /// well-formed data).
    pub fn to_bytes(&self, fmt: SerializeFormat) -> Result<Vec<u8>, String> {
        match fmt {
            SerializeFormat::Json => serde_json::to_vec(self).map_err(|e| e.to_string()),
            SerializeFormat::MsgPack => rmp_serde::to_vec_named(self).map_err(|e| e.to_string()),
        }
    }

    /// Deserialize from bytes using the specified format.
    pub fn from_bytes(data: &[u8], fmt: SerializeFormat) -> Result<Self, String> {
        match fmt {
            SerializeFormat::Json => serde_json::from_slice(data).map_err(|e| e.to_string()),
            SerializeFormat::MsgPack => rmp_serde::from_slice(data).map_err(|e| e.to_string()),
        }
    }

    /// Convenience: serialize to a JSON string.
    /// Convenience: serialize to a JSON string.
    pub fn to_json_string(&self) -> Result<String, String> {
        // RTK-inspired: compact context to reduce token consumption
        let mut compacted = self.clone();
        compacted.context = compacted
            .context
            .iter()
            .map(|(k, v)| (k.clone(), compact_json_value(v, 0, 3)))
            .collect();
        serde_json::to_string(&compacted).map_err(|e| e.to_string())
    }

    /// Convenience: deserialize from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

/// Python-facing ToolResult.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "ToolResult", eq, from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionResultModel {
    pub(crate) inner: ActionResultModelData,
}

impl ActionResultModel {
    /// Create a `ToolResult` from raw data.
    #[must_use]
    pub fn from_data(data: ActionResultModelData) -> Self {
        Self { inner: data }
    }

    /// Access the underlying data.
    #[must_use]
    pub fn data(&self) -> &ActionResultModelData {
        &self.inner
    }
}

impl std::fmt::Display for ActionResultModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.inner.success {
            write!(f, "Success: {}", self.inner.message)
        } else {
            write!(
                f,
                "Error: {}",
                self.inner.error.as_deref().unwrap_or(&self.inner.message)
            )
        }
    }
}

// ── Factory functions live in `crate::python::action_result`. ──

#[cfg(test)]
#[path = "action_result_tests.rs"]
mod tests;
