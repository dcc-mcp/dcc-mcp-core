//! Wire types for optional adapter debug-session discovery.
//!
//! Core deliberately does not depend on a debugger implementation. These
//! descriptors let adapters expose attach metadata for `debugpy`, `pydevd`,
//! native debuggers, or host-defined debugging backends in a consistent shape.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Debugger availability/listening state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugSessionStatus {
    /// The adapter does not expose debugging for this runtime.
    #[default]
    Unavailable,
    /// Debugging is supported but not currently listening.
    Available,
    /// The debugger is listening for an attach client.
    Listening,
    /// A debugger client is currently connected.
    ClientConnected,
    /// The adapter attempted to expose debugging but hit an error.
    Error,
}

/// Path mapping hint for attach-based debuggers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugPathMapping {
    /// Path as seen by the local IDE/client.
    pub local_root: String,
    /// Path as seen by the DCC runtime.
    pub remote_root: String,
}

/// Optional debug attach descriptor published by a DCC adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugSessionDescriptor {
    /// Debugger kind, such as `debugpy`, `pydevd`, `native`, or adapter-defined.
    pub debugger_kind: String,
    /// Current debugger state.
    pub status: DebugSessionStatus,
    /// Host name or IP address for attach-based debuggers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// TCP port for attach-based debuggers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Process/runtime identity suitable for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// OS process id when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    /// Optional path mappings for IDE attach configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_mappings: Vec<DebugPathMapping>,
    /// Optional log resource URI or filesystem-independent location hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_uri: Option<String>,
    /// Adapter-supplied setup guidance, especially for unavailable states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_instructions: Option<String>,
    /// Adapter-defined diagnostic flags and extra fields.
    #[serde(default)]
    pub metadata: Value,
}

impl DebugSessionDescriptor {
    /// Build an unavailable descriptor with actionable setup guidance.
    #[must_use]
    pub fn unavailable(
        debugger_kind: impl Into<String>,
        setup_instructions: impl Into<String>,
    ) -> Self {
        Self {
            debugger_kind: debugger_kind.into(),
            status: DebugSessionStatus::Unavailable,
            host: None,
            port: None,
            runtime: None,
            process_id: None,
            path_mappings: Vec::new(),
            log_uri: None,
            setup_instructions: Some(setup_instructions.into()),
            metadata: Value::Null,
        }
    }

    /// Build a listening attach descriptor.
    #[must_use]
    pub fn listening(debugger_kind: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            debugger_kind: debugger_kind.into(),
            status: DebugSessionStatus::Listening,
            host: Some(host.into()),
            port: Some(port),
            runtime: None,
            process_id: None,
            path_mappings: Vec::new(),
            log_uri: None,
            setup_instructions: None,
            metadata: Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_descriptor_carries_setup_guidance() {
        let descriptor = DebugSessionDescriptor::unavailable(
            "debugpy",
            "Install the adapter debug extra and restart the DCC.",
        );
        let value = serde_json::to_value(&descriptor).unwrap();
        assert_eq!(value["status"], "unavailable");
        assert!(
            value["setup_instructions"]
                .as_str()
                .unwrap()
                .contains("Install")
        );
    }

    #[test]
    fn listening_descriptor_round_trips() {
        let mut descriptor = DebugSessionDescriptor::listening("native", "127.0.0.1", 9000);
        descriptor.status = DebugSessionStatus::ClientConnected;
        descriptor.metadata = serde_json::json!({"adapter": "houdini"});
        descriptor.path_mappings.push(DebugPathMapping {
            local_root: "C:/project".to_owned(),
            remote_root: "/mnt/project".to_owned(),
        });

        let encoded = serde_json::to_string(&descriptor).unwrap();
        let decoded: DebugSessionDescriptor = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, descriptor);
        assert_eq!(decoded.metadata["adapter"], "houdini");
    }
}
