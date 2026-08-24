//! ToolResult — unified result type for all tool executions.
//!
//! Plain Rust struct; PyO3 bindings live in `crate::python::action_result`.

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyclass_enum};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Adapter-owned job discovered inside the terminal result of a Core job.
///
/// New adapters should return the explicit ``adapter_job`` or
/// ``adapter_job_id`` shape. ``context.job_id`` remains supported because
/// released adapters historically returned pollable operation ids there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedAdapterJob {
    pub job_id: String,
    pub source: &'static str,
}

/// Find an adapter-owned job id in a completed tool result.
///
/// The caller supplies the Core job id so a handler that echoes the outer id
/// is never misclassified as a second operation. This helper deliberately
/// inspects only one tool-result object; transport wrappers must be removed by
/// the caller before invoking it.
#[must_use]
pub fn linked_adapter_job_from_result(
    result: &serde_json::Value,
    core_job_id: &str,
) -> Option<LinkedAdapterJob> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("/adapter_job/job_id", "result.adapter_job.job_id"),
        ("/adapter_job_id", "result.adapter_job_id"),
        ("/context/adapter_job_id", "result.context.adapter_job_id"),
        ("/context/job_id", "result.context.job_id"),
        ("/job_id", "result.job_id"),
    ];

    CANDIDATES.iter().find_map(|(pointer, source)| {
        let job_id = result.pointer(pointer)?.as_str()?.trim();
        (!job_id.is_empty() && job_id != core_job_id).then(|| LinkedAdapterJob {
            job_id: job_id.to_string(),
            source,
        })
    })
}

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

/// Rust data representation (serde-friendly).
///
/// The public field layout is intentionally stable for downstream struct
/// literals. Additive top-level envelope fields such as post-condition evidence
/// and Python metadata are stored by [`ActionResultModel`] rather than adding a
/// source-breaking field here.
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
    /// Stable machine-readable error code when `success` is `false`.
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

#[derive(Serialize)]
struct ActionResultWireRef<'a> {
    #[serde(flatten)]
    data: &'a ActionResultModelData,
    #[serde(skip_serializing_if = "Option::is_none")]
    postcondition: Option<&'a HashMap<String, serde_json::Value>>,
    #[serde(rename = "_meta", skip_serializing_if = "HashMap::is_empty")]
    meta: &'a HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct ActionResultWireOwned {
    #[serde(flatten)]
    data: ActionResultModelData,
    #[serde(default)]
    postcondition: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "_meta", default)]
    meta: HashMap<String, serde_json::Value>,
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
    pub(crate) postcondition: Option<HashMap<String, serde_json::Value>>,
    pub(crate) meta: HashMap<String, serde_json::Value>,
}

impl ActionResultModel {
    /// Create a `ToolResult` from raw data.
    #[must_use]
    pub fn from_data(data: ActionResultModelData) -> Self {
        Self {
            inner: data,
            postcondition: None,
            meta: HashMap::new(),
        }
    }

    /// Create a `ToolResult` from raw data and top-level metadata.
    #[must_use]
    pub fn from_data_with_meta(
        data: ActionResultModelData,
        meta: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            inner: data,
            postcondition: None,
            meta,
        }
    }

    /// Create a `ToolResult` with top-level post-condition evidence and metadata.
    pub fn from_data_with_envelope(
        data: ActionResultModelData,
        postcondition: Option<HashMap<String, serde_json::Value>>,
        meta: HashMap<String, serde_json::Value>,
    ) -> Result<Self, String> {
        Self::validate_postcondition(postcondition.as_ref())?;
        Ok(Self {
            inner: data,
            postcondition,
            meta,
        })
    }

    pub(crate) fn validate_postcondition(
        postcondition: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(), String> {
        let Some(verified) = postcondition.and_then(|value| value.get("verified")) else {
            return Ok(());
        };
        if verified.is_boolean() {
            return Ok(());
        }
        Err("'postcondition.verified' field must be a boolean".to_string())
    }

    /// Access the underlying data.
    #[must_use]
    pub fn data(&self) -> &ActionResultModelData {
        &self.inner
    }

    /// Access top-level `_meta` values without mixing them into context.
    #[must_use]
    pub fn meta(&self) -> &HashMap<String, serde_json::Value> {
        &self.meta
    }

    /// Access top-level post-condition evidence without mixing it into context.
    #[must_use]
    pub fn postcondition(&self) -> Option<&HashMap<String, serde_json::Value>> {
        self.postcondition.as_ref()
    }

    /// Serialize the complete model, including top-level `_meta`.
    pub fn to_bytes(&self, fmt: SerializeFormat) -> Result<Vec<u8>, String> {
        let wire = ActionResultWireRef {
            data: &self.inner,
            postcondition: self.postcondition.as_ref(),
            meta: &self.meta,
        };
        match fmt {
            SerializeFormat::Json => serde_json::to_vec(&wire).map_err(|e| e.to_string()),
            SerializeFormat::MsgPack => rmp_serde::to_vec_named(&wire).map_err(|e| e.to_string()),
        }
    }

    /// Deserialize the complete model, including top-level `_meta`.
    pub fn from_bytes(data: &[u8], fmt: SerializeFormat) -> Result<Self, String> {
        let wire: ActionResultWireOwned = match fmt {
            SerializeFormat::Json => serde_json::from_slice(data).map_err(|e| e.to_string())?,
            SerializeFormat::MsgPack => rmp_serde::from_slice(data).map_err(|e| e.to_string())?,
        };
        Self::from_data_with_envelope(wire.data, wire.postcondition, wire.meta)
    }

    /// Serialize to compact JSON while applying the historical context limit.
    pub fn to_json_string(&self) -> Result<String, String> {
        let mut compacted = self.clone();
        compacted.inner.context = compacted
            .inner
            .context
            .iter()
            .map(|(k, v)| (k.clone(), compact_json_value(v, 0, 3)))
            .collect();
        String::from_utf8(compacted.to_bytes(SerializeFormat::Json)?).map_err(|e| e.to_string())
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
