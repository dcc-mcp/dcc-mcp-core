//! Structured error envelope for MCP `tools/call` failures.
//!
//! When a `tools/call` request fails, the server returns a
//! [`CallToolResult`](crate::types::CallToolResult) with `is_error: true`.
//! The `text` payload inside the result is the JSON-serialised form of
//! [`DccMcpError`] so that both agents and humans can programmatically
//! identify *which layer* failed and what to do next.
//!
//! # Layers
//!
//! | Layer        | Meaning                                           |
//! |--------------|---------------------------------------------------|
//! | `gateway`    | MCP gateway routing (skill/group stubs, dispatch) |
//! | `registry`   | Tool lookup in the [`ActionRegistry`]              |
//! | `instance`   | Tool execution / handler invocation                |
//! | `subprocess` | Subprocess or bridge communication failure         |
//! | `dcc`        | DCC application returned an error                  |
//!
//! # Error codes
//!
//! Codes are UPPER_SNAKE_CASE strings (not integers) for readability.
//! Common codes: `SKILL_NOT_LOADED`, `GROUP_NOT_ACTIVATED`,
//! `ACTION_NOT_FOUND`, `NO_HANDLER`, `EXECUTION_FAILED`, `TIMEOUT`,
//! `PROCESS_NOT_READY`, `DCC_CMD_FAILED`.

use serde::{Deserialize, Serialize};

/// Structured error envelope returned inside MCP `tools/call` error responses.
///
/// Serialised as JSON into the `text` field of a `CallToolResult` so that
/// downstream consumers (agents, UIs, log aggregators) can parse it without
/// regex.
///
/// ```json
/// {
///   "layer": "gateway",
///   "code": "SKILL_NOT_LOADED",
///   "message": "Skill 'scene-tools' is not loaded.",
///   "hint": "Call load_skill with skill_name=\"scene-tools\" to register its tools.",
///   "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DccMcpError {
    /// Which architectural layer produced the error.
    ///
    /// One of: `"gateway"`, `"registry"`, `"instance"`, `"subprocess"`, `"dcc"`.
    pub layer: String,

    /// Machine-readable error code (UPPER_SNAKE_CASE).
    ///
    /// Examples: `"SKILL_NOT_LOADED"`, `"ACTION_NOT_FOUND"`, `"EXECUTION_FAILED"`.
    pub code: String,

    /// Short, human-readable error description.
    pub message: String,

    /// Actionable next step the caller can take to resolve the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    /// Unique identifier for correlating this error with server-side logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl DccMcpError {
    /// Create a new error envelope with an auto-generated `trace_id` (UUID v4).
    pub fn new(
        layer: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            layer: layer.into(),
            code: code.into(),
            message: message.into(),
            hint: None,
            trace_id: Some(uuid::Uuid::new_v4().to_string()),
        }
    }

    /// Attach an actionable hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Attach a trace ID for log correlation.
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Serialise to a JSON string suitable for embedding in a `CallToolResult`
    /// text content block.
    ///
    /// Falls back to a plain-text representation if serialisation fails
    /// (should never happen for this struct).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "[{layer}/{code}] {message}",
                layer = self.layer,
                code = self.code,
                message = self.message,
            )
        })
    }

    /// Serialise to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| self.to_json())
    }
}

impl std::fmt::Display for DccMcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{layer}/{code}] {message}",
            layer = self.layer,
            code = self.code,
            message = self.message
        )?;
        if let Some(hint) = &self.hint {
            write!(f, " — hint: {hint}")?;
        }
        Ok(())
    }
}

/// Well-known layer names for [`DccMcpError::layer`].
pub mod layers {
    /// MCP gateway routing (skill/group stubs, core tool dispatch).
    pub const GATEWAY: &str = "gateway";
    /// Tool lookup in the action registry.
    pub const REGISTRY: &str = "registry";
    /// Tool execution / handler invocation.
    pub const INSTANCE: &str = "instance";
    /// Subprocess or bridge communication failure.
    pub const SUBPROCESS: &str = "subprocess";
    /// DCC application returned an error.
    pub const DCC: &str = "dcc";
}

/// Well-known error codes for [`DccMcpError::code`].
pub mod codes {
    pub const SKILL_NOT_LOADED: &str = "SKILL_NOT_LOADED";
    pub const GROUP_NOT_ACTIVATED: &str = "GROUP_NOT_ACTIVATED";
    pub const ACTION_NOT_FOUND: &str = "ACTION_NOT_FOUND";
    pub const NO_HANDLER: &str = "NO_HANDLER";
    pub const EXECUTION_FAILED: &str = "EXECUTION_FAILED";
    pub const TIMEOUT: &str = "TIMEOUT";
    pub const PROCESS_NOT_READY: &str = "PROCESS_NOT_READY";
    pub const DCC_CMD_FAILED: &str = "DCC_CMD_FAILED";
    pub const REQUEST_CANCELLED: &str = "REQUEST_CANCELLED";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialize_roundtrip() {
        let err = DccMcpError::new("gateway", "SKILL_NOT_LOADED", "Skill 'x' is not loaded.")
            .with_hint("Call load_skill(\"x\")");

        // trace_id is auto-generated by new()
        assert!(err.trace_id.is_some());

        let json = err.to_json();
        let parsed: DccMcpError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, err);
    }

    #[test]
    fn test_error_optional_fields_omitted() {
        // Build without hint and without trace_id to test omission
        let err = DccMcpError {
            layer: "registry".to_string(),
            code: "ACTION_NOT_FOUND".to_string(),
            message: "Unknown tool: foo".to_string(),
            hint: None,
            trace_id: None,
        };
        let json = err.to_json();
        assert!(!json.contains("hint"));
        assert!(!json.contains("trace_id"));
    }

    #[test]
    fn test_new_auto_generates_trace_id() {
        let err = DccMcpError::new("gateway", "SKILL_NOT_LOADED", "Not loaded");
        assert!(err.trace_id.is_some());
        // Verify it looks like a UUID (36 chars with hyphens)
        let tid = err.trace_id.as_ref().unwrap();
        assert_eq!(tid.len(), 36);
        assert_eq!(tid.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn test_trace_ids_are_unique() {
        let e1 = DccMcpError::new("gateway", "A", "a");
        let e2 = DccMcpError::new("gateway", "A", "a");
        assert_ne!(e1.trace_id, e2.trace_id);
    }

    #[test]
    fn test_display_with_hint() {
        let err = DccMcpError::new("gateway", "SKILL_NOT_LOADED", "Not loaded")
            .with_hint("Call load_skill");
        let s = format!("{err}");
        assert!(s.contains("[gateway/SKILL_NOT_LOADED]"));
        assert!(s.contains("hint: Call load_skill"));
    }

    #[test]
    fn test_display_without_hint() {
        let err = DccMcpError::new("instance", "EXECUTION_FAILED", "Handler panicked");
        let s = format!("{err}");
        assert!(s.contains("[instance/EXECUTION_FAILED]"));
        assert!(!s.contains("hint"));
    }

    #[test]
    fn test_layer_and_code_constants() {
        assert_eq!(layers::GATEWAY, "gateway");
        assert_eq!(layers::REGISTRY, "registry");
        assert_eq!(layers::INSTANCE, "instance");
        assert_eq!(codes::SKILL_NOT_LOADED, "SKILL_NOT_LOADED");
        assert_eq!(codes::ACTION_NOT_FOUND, "ACTION_NOT_FOUND");
    }

    #[test]
    fn test_pretty_json() {
        let err =
            DccMcpError::new("dcc", "DCC_CMD_FAILED", "Command failed").with_trace_id("trace-1");
        let pretty = err.to_json_pretty();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("DCC_CMD_FAILED"));
    }
}
