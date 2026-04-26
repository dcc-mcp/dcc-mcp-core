//! Session-scoped dynamic tool registration (issue #462).
//!
//! AI agents can register ephemeral tools at runtime — without writing SKILL.md
//! files or restarting the server. Tools are session-scoped: only the session
//! that created a tool can see and call it, and the tool is automatically
//! discarded when the session expires.
//!
//! # MCP tools exposed
//!
//! | Tool                 | Description |
//! |----------------------|-------------|
//! | `register_tool`      | Register a new [`ToolSpec`] and get back a session-unique tool name. |
//! | `deregister_tool`    | Remove a previously registered dynamic tool. |
//! | `list_dynamic_tools` | List all dynamic tools for the calling session. |
//!
//! # Execution model
//!
//! Dynamic tool code is executed by the server's [`DeferredExecutor`] (the DCC
//! main thread) when present. If no executor is wired (unit tests / pure-HTTP
//! mode) the call returns an error explaining that in-process execution is
//! unavailable.
//!
//! # Naming
//!
//! Every registered tool receives a collision-resistant name:
//! `dyn__{original_name}_{random6}` where `random6` is a 6-character
//! alphanumeric suffix.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::{McpTool, McpToolAnnotations};

/// Default TTL for a dynamic tool if not specified in the spec.
const DEFAULT_TOOL_TTL_SECS: u64 = 3600;

/// Maximum code size for a dynamic tool (256 KiB).
const MAX_CODE_BYTES: usize = 256 * 1024;

/// The name prefix for all dynamic tools, making them easy to identify.
pub const DYNAMIC_TOOL_PREFIX: &str = "dyn__";

// ── ToolSpec ─────────────────────────────────────────────────────────────────

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

// ── DynamicToolEntry ──────────────────────────────────────────────────────────

/// A registered dynamic tool tracked by [`SessionDynamicTools`].
#[derive(Debug, Clone)]
pub struct DynamicToolEntry {
    /// The assigned tool name (prefixed, collision-safe).
    pub tool_name: String,
    /// Original spec as provided by the agent.
    pub spec: ToolSpec,
    /// When the tool was registered.
    pub registered_at: Instant,
    /// When the tool expires (registered_at + TTL).
    pub expires_at: Instant,
}

impl DynamicToolEntry {
    fn new(tool_name: String, spec: ToolSpec) -> Self {
        let ttl = Duration::from_secs(spec.ttl_secs.unwrap_or(DEFAULT_TOOL_TTL_SECS));
        let now = Instant::now();
        Self {
            tool_name,
            spec,
            registered_at: now,
            expires_at: now + ttl,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// Build the [`McpTool`] descriptor for `tools/list`.
    pub fn to_mcp_tool(&self) -> McpTool {
        let input_schema = if let Some(props) = &self.spec.parameters {
            json!({
                "type": "object",
                "properties": props
            })
        } else {
            json!({ "type": "object", "properties": {} })
        };

        let description = format!(
            "{}\n\n[Dynamic tool — session-scoped, auto-expires]",
            self.spec.description
        );

        McpTool {
            name: self.tool_name.clone(),
            description,
            input_schema,
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some(self.spec.name.clone()),
                read_only_hint: Some(self.spec.read_only_hint),
                destructive_hint: Some(self.spec.destructive_hint),
                idempotent_hint: Some(false),
                // Mark as open-world to signal AI-generated provenance.
                open_world_hint: Some(true),
                deferred_hint: Some(false),
            }),
            meta: {
                let mut m = serde_json::Map::new();
                m.insert("dcc.dynamic_tool".to_string(), json!(true));
                m.insert(
                    "dcc.dynamic_tool_language".to_string(),
                    json!(self.spec.language),
                );
                m.insert(
                    "dcc.dynamic_tool_timeout_sec".to_string(),
                    json!(self.spec.timeout_sec),
                );
                Some(m)
            },
        }
    }
}

// ── SessionDynamicTools ────────────────────────────────────────────────────────

/// Per-session dynamic tool registry.
///
/// Stored inside each [`crate::session::McpSession`] and keyed by the
/// assigned tool name.
#[derive(Debug, Default)]
pub struct SessionDynamicTools {
    tools: HashMap<String, DynamicToolEntry>,
}

impl SessionDynamicTools {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new tool. Returns `(tool_name, expires_in_secs)`.
    pub fn register(&mut self, spec: ToolSpec) -> Result<(String, u64), DynamicToolError> {
        validate_spec(&spec)?;
        let tool_name = generate_tool_name(&spec.name);
        let ttl = spec.ttl_secs.unwrap_or(DEFAULT_TOOL_TTL_SECS);
        let entry = DynamicToolEntry::new(tool_name.clone(), spec);
        self.tools.insert(tool_name.clone(), entry);
        Ok((tool_name, ttl))
    }

    /// Remove a tool by its assigned name. Returns `true` if found and removed.
    pub fn deregister(&mut self, tool_name: &str) -> bool {
        self.tools.remove(tool_name).is_some()
    }

    /// Look up a tool by assigned name. Returns `None` if expired or not found.
    pub fn get(&self, tool_name: &str) -> Option<&DynamicToolEntry> {
        self.tools.get(tool_name).filter(|e| !e.is_expired())
    }

    /// Iterate over all non-expired dynamic tools.
    pub fn iter_active(&self) -> impl Iterator<Item = &DynamicToolEntry> {
        self.tools.values().filter(|e| !e.is_expired())
    }

    /// Remove expired entries. Called lazily on `tools/list` and `tools/call`.
    pub fn evict_expired(&mut self) {
        self.tools.retain(|_, e| !e.is_expired());
    }

    /// Build `McpTool` descriptors for all non-expired tools.
    pub fn to_mcp_tools(&mut self) -> Vec<McpTool> {
        self.evict_expired();
        self.tools.values().map(|e| e.to_mcp_tool()).collect()
    }

    /// Number of registered (including possibly expired) tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum DynamicToolError {
    #[error("tool name is empty or contains invalid characters (allowed: a-z A-Z 0-9 _ -)")]
    InvalidName,
    #[error("tool description is empty")]
    EmptyDescription,
    #[error("code body is empty")]
    EmptyCode,
    #[error("code exceeds maximum size of {MAX_CODE_BYTES} bytes")]
    CodeTooLarge,
    #[error("unsupported language {0:?}; only 'python' is supported")]
    UnsupportedLanguage(String),
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn validate_spec(spec: &ToolSpec) -> Result<(), DynamicToolError> {
    if spec.name.is_empty()
        || !spec
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(DynamicToolError::InvalidName);
    }
    if spec.description.trim().is_empty() {
        return Err(DynamicToolError::EmptyDescription);
    }
    if spec.code.trim().is_empty() {
        return Err(DynamicToolError::EmptyCode);
    }
    if spec.code.len() > MAX_CODE_BYTES {
        return Err(DynamicToolError::CodeTooLarge);
    }
    if spec.language != "python" {
        return Err(DynamicToolError::UnsupportedLanguage(spec.language.clone()));
    }
    Ok(())
}

/// Generate a collision-resistant tool name: `dyn__{name}_{random6}`.
fn generate_tool_name(base: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Use timestamp + counter as a simple random-enough suffix (no external rand dep).
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let suffix = format!("{nanos:06x}");
    // Take last 6 hex chars for a compact suffix.
    let suffix = &suffix[suffix.len().saturating_sub(6)..];
    format!("{DYNAMIC_TOOL_PREFIX}{base}_{suffix}")
}

// ── MCP tool schema builders ──────────────────────────────────────────────────

/// Build the static `register_tool` MCP tool descriptor.
pub fn build_register_tool_descriptor() -> McpTool {
    McpTool {
        name: "register_tool".to_string(),
        description: "Dynamically register a new session-scoped tool from a ToolSpec.\n\n\
            When to use: When you need a one-off helper that executes DCC code on demand \
            and you want it to appear in tools/list only for this session.\n\n\
            How to use:\n\
            - Provide tool_spec with at minimum name, description, and code.\n\
            - The returned tool_name is the callable identifier — use it in tools/call.\n\
            - Tool expires when the session ends or after ttl_secs (default 3600 s)."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tool_spec": {
                    "type": "object",
                    "description": "ToolSpec definition.",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Tool name: [a-zA-Z0-9_-], max 64 chars."
                        },
                        "description": {
                            "type": "string",
                            "description": "What the tool does (≤500 chars)."
                        },
                        "code": {
                            "type": "string",
                            "description": "Python code body. Use params dict for inputs."
                        },
                        "language": {
                            "type": "string",
                            "enum": ["python"],
                            "default": "python",
                            "description": "Execution language (only python supported)."
                        },
                        "parameters": {
                            "type": "object",
                            "description": "JSON Schema properties map for tool inputs."
                        },
                        "timeout_sec": {
                            "type": "integer",
                            "default": 30,
                            "description": "Hard execution timeout in seconds."
                        },
                        "read_only_hint": {
                            "type": "boolean",
                            "default": true,
                            "description": "Hint: tool does not mutate DCC state."
                        },
                        "destructive_hint": {
                            "type": "boolean",
                            "default": false,
                            "description": "Hint: tool makes irreversible changes."
                        },
                        "ttl_secs": {
                            "type": "integer",
                            "description": "How long the tool lives (seconds). Default 3600."
                        }
                    },
                    "required": ["name", "description", "code"]
                }
            },
            "required": ["tool_spec"]
        }),
        output_schema: None,
        annotations: Some(McpToolAnnotations {
            title: Some("Register Dynamic Tool".to_string()),
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(false),
            open_world_hint: Some(true),
            deferred_hint: Some(false),
        }),
        meta: None,
    }
}

/// Build the static `deregister_tool` MCP tool descriptor.
pub fn build_deregister_tool_descriptor() -> McpTool {
    McpTool {
        name: "deregister_tool".to_string(),
        description: "Remove a dynamically registered tool from this session.\n\n\
            When to use: After the agent no longer needs a tool registered via register_tool.\n\n\
            How to use:\n\
            - Pass the tool_name returned by register_tool.\n\
            - Returns success even if the tool has already expired."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Assigned tool name returned by register_tool (starts with dyn__)."
                }
            },
            "required": ["tool_name"]
        }),
        output_schema: None,
        annotations: Some(McpToolAnnotations {
            title: Some("Deregister Dynamic Tool".to_string()),
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            deferred_hint: Some(false),
        }),
        meta: None,
    }
}

/// Build the static `list_dynamic_tools` MCP tool descriptor.
pub fn build_list_dynamic_tools_descriptor() -> McpTool {
    McpTool {
        name: "list_dynamic_tools".to_string(),
        description: "List all non-expired dynamic tools registered by this session.\n\n\
            When to use: To audit which session-scoped tools are currently available, \
            or to find their exact names before calling or deregistering them.\n\n\
            How to use: No parameters required."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: None,
        annotations: Some(McpToolAnnotations {
            title: Some("List Dynamic Tools".to_string()),
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            deferred_hint: Some(false),
        }),
        meta: None,
    }
}

// ── Handler implementations ────────────────────────────────────────────────────

/// Handle `register_tool` call. Returns a JSON-serialisable result value.
pub fn handle_register_tool(
    session_dynamic_tools: &mut SessionDynamicTools,
    params: &Value,
) -> Value {
    let spec: ToolSpec = match params
        .get("tool_spec")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(s) => s,
        None => {
            return json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Missing or invalid tool_spec parameter" }]
            });
        }
    };

    match session_dynamic_tools.register(spec) {
        Ok((tool_name, ttl)) => {
            json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&json!({
                        "success": true,
                        "tool_name": tool_name,
                        "expires_in_sec": ttl,
                        "message": format!("Tool '{tool_name}' registered. Call it via tools/call.")
                    })).unwrap_or_default()
                }]
            })
        }
        Err(e) => {
            json!({
                "isError": true,
                "content": [{ "type": "text", "text": format!("Failed to register tool: {e}") }]
            })
        }
    }
}

/// Handle `deregister_tool` call.
pub fn handle_deregister_tool(
    session_dynamic_tools: &mut SessionDynamicTools,
    params: &Value,
) -> Value {
    let tool_name = match params.get("tool_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Missing tool_name parameter" }]
            });
        }
    };

    let removed = session_dynamic_tools.deregister(&tool_name);
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "success": true,
                "removed": removed,
                "tool_name": tool_name
            })).unwrap_or_default()
        }]
    })
}

/// Handle `list_dynamic_tools` call.
pub fn handle_list_dynamic_tools(session_dynamic_tools: &mut SessionDynamicTools) -> Value {
    let tools: Vec<Value> = session_dynamic_tools
        .iter_active()
        .map(|e| {
            json!({
                "tool_name": e.tool_name,
                "original_name": e.spec.name,
                "description": e.spec.description,
                "language": e.spec.language,
                "read_only_hint": e.spec.read_only_hint,
                "destructive_hint": e.spec.destructive_hint,
                "expires_in_sec": e.expires_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
            })
        })
        .collect();

    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "dynamic_tools": tools,
                "count": tools.len()
            })).unwrap_or_default()
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_string(),
            description: "A test tool".to_string(),
            code: "return {'ok': True}".to_string(),
            language: "python".to_string(),
            parameters: None,
            dcc: None,
            timeout_sec: 30,
            read_only_hint: true,
            destructive_hint: false,
            ttl_secs: None,
        }
    }

    #[test]
    fn test_register_and_retrieve() {
        let mut reg = SessionDynamicTools::new();
        let spec = make_spec("my_tool");
        let (name, ttl) = reg.register(spec).unwrap();
        assert!(name.starts_with("dyn__my_tool_"));
        assert_eq!(ttl, DEFAULT_TOOL_TTL_SECS);
        assert!(reg.get(&name).is_some());
    }

    #[test]
    fn test_deregister() {
        let mut reg = SessionDynamicTools::new();
        let (name, _) = reg.register(make_spec("tool_a")).unwrap();
        assert!(reg.deregister(&name));
        assert!(reg.get(&name).is_none());
        // Idempotent: deregister again returns false
        assert!(!reg.deregister(&name));
    }

    #[test]
    fn test_invalid_name_rejected() {
        let mut reg = SessionDynamicTools::new();
        let mut spec = make_spec("valid");
        spec.name = "bad name!".to_string();
        assert!(reg.register(spec).is_err());
    }

    #[test]
    fn test_empty_code_rejected() {
        let mut reg = SessionDynamicTools::new();
        let mut spec = make_spec("valid");
        spec.code = "   ".to_string();
        assert!(reg.register(spec).is_err());
    }

    #[test]
    fn test_to_mcp_tool() {
        let mut reg = SessionDynamicTools::new();
        let (name, _) = reg.register(make_spec("helper")).unwrap();
        let tools = reg.to_mcp_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, name);
    }
}
