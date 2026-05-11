//! Session-scoped dynamic tool wire types (issue #852).
//!
//! Runtime state for dynamic tools lives in `dcc-mcp-http-server`; this module
//! carries the agent-provided tool specification so Python bindings and config
//! tooling can parse the wire shape without depending on server runtime code.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A session-scoped tool definition provided by an AI agent.
///
/// `ToolSpec` describes a tool's metadata and the code that should run when
/// the tool is called. The server executes `code` inside the DCC's Python
/// interpreter (or another interpreter matching `language`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Human-readable tool name. Must be `[a-zA-Z0-9_-]+`, max 64 chars.
    pub name: String,
    /// What the tool does (≤500 chars for MCP compliance).
    pub description: String,
    /// The code body to execute. Receives `params` as a dict-like namespace.
    pub code: String,
    /// Execution language. Currently only `"python"` is supported.
    #[serde(default = "default_language")]
    pub language: String,
    /// JSON Schema `properties` object for the tool's inputs (optional).
    #[serde(default)]
    pub parameters: Option<Value>,
    /// If set, only run this tool when the server's DCC type matches (e.g. `"maya"`).
    #[serde(default)]
    pub dcc: Option<String>,
    /// Hard execution timeout in seconds (default 30).
    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
    /// Hint: does the tool avoid mutating scene state?
    #[serde(default = "default_read_only")]
    pub read_only_hint: bool,
    /// Hint: does the tool make irreversible changes?
    #[serde(default)]
    pub destructive_hint: bool,
    /// Optional TTL override for how long this tool lives (seconds).
    #[serde(default)]
    pub ttl_secs: Option<u64>,
}

fn default_language() -> String {
    "python".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_read_only() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_spec_minimal_body_uses_runtime_defaults() {
        let spec: ToolSpec = serde_json::from_value(json!({
            "name": "make_cube",
            "description": "Create a cube",
            "code": "cmds.polyCube()"
        }))
        .unwrap();
        assert_eq!(spec.language, "python");
        assert_eq!(spec.timeout_sec, 30);
        assert!(spec.read_only_hint);
        assert!(!spec.destructive_hint);
        assert!(spec.parameters.is_none());
        assert!(spec.dcc.is_none());
        assert!(spec.ttl_secs.is_none());
    }

    #[test]
    fn tool_spec_round_trips_full_body() {
        let spec = ToolSpec {
            name: "paint_mask".to_owned(),
            description: "Paint a Photoshop mask".to_owned(),
            code: "run_tool(params)".to_owned(),
            language: "python".to_owned(),
            parameters: Some(json!({"radius": {"type": "number"}})),
            dcc: Some("photoshop".to_owned()),
            timeout_sec: 45,
            read_only_hint: false,
            destructive_hint: true,
            ttl_secs: Some(600),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: ToolSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, spec.name);
        assert_eq!(back.description, spec.description);
        assert_eq!(back.code, spec.code);
        assert_eq!(back.dcc, spec.dcc);
        assert_eq!(back.timeout_sec, spec.timeout_sec);
        assert_eq!(back.read_only_hint, spec.read_only_hint);
        assert_eq!(back.destructive_hint, spec.destructive_hint);
        assert_eq!(back.ttl_secs, spec.ttl_secs);
    }
}
