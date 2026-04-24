use dcc_mcp_models::{ExecutionMode, NextTools, ThreadAffinity, ToolAnnotations};
use serde::{Deserialize, Serialize};

/// Metadata about a registered Action (stored in Rust).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionMeta {
    /// Unique action identifier.
    pub name: String,
    /// Human-readable action description.
    pub description: String,
    /// Action category for grouping (e.g. "geometry", "pipeline").
    pub category: String,
    /// Searchable tags for discovery.
    pub tags: Vec<String>,
    /// Target DCC application (e.g. "maya", "blender").
    pub dcc: String,
    /// Semantic version string.
    pub version: String,
    /// JSON Schema for action input parameters.
    pub input_schema: serde_json::Value,
    /// JSON Schema for action output.
    pub output_schema: serde_json::Value,
    /// Optional path to the Python source file defining this action.
    pub source_file: Option<String>,
    /// Name of the skill this action belongs to (if registered from a skill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    /// Tool group this action belongs to (``""`` = always active).
    ///
    /// See [`dcc_mcp_models::SkillGroup`]; used together with `enabled` to
    /// implement progressive tool exposure via ``activate_tool_group``.
    #[serde(default)]
    pub group: String,
    /// Whether this action is currently active / callable.
    ///
    /// Tools in an inactive group are collapsed into a ``__group__<name>``
    /// stub in ``tools/list``. The dispatcher refuses to invoke disabled
    /// actions.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Host-DCC capabilities required for this action to be surfaced.
    ///
    /// When non-empty, Gateway / adapter implementations **should** hide
    /// this action from ``tools/list`` on sessions whose host DCC does not
    /// advertise every listed capability (see
    /// [``WebViewAdapter.capabilities``](crate::adapters::webview) for the
    /// pre-defined key set: ``"scene"``, ``"timeline"``, ``"selection"``,
    /// ``"undo"``, ``"render"``).
    ///
    /// The registry itself does **not** perform filtering — filtering is
    /// the responsibility of the consumer (Gateway, HTTP server, adapter).
    /// Storing the declaration here avoids a separate side-table lookup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<String>,
    /// Execution mode declared by the skill author (issue #317).
    ///
    /// `Sync` (default) or `Async`. Drives the server-derived MCP
    /// `deferredHint` annotation emitted by `tools/list`.
    #[serde(default)]
    pub execution: ExecutionMode,
    /// Optional hint about typical execution time in seconds (issue #317).
    ///
    /// Surfaces under `_meta.dcc.timeoutHintSecs` on the tool definition —
    /// never inside `annotations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_hint_secs: Option<u32>,
    /// Thread-affinity hint surfaced by the skill author (issue #332).
    ///
    /// Drives async-dispatch routing in the HTTP server:
    /// `Main` forces the tool through [`crate::DeferredExecutor`] even along
    /// the async `tools/call` path, guaranteeing the handler runs on the DCC's
    /// main thread. `Any` (default) allows execution on a Tokio worker.
    #[serde(default, skip_serializing_if = "is_default_thread_affinity")]
    pub thread_affinity: ThreadAffinity,
    /// MCP tool annotations declared by the skill author (issue #344).
    ///
    /// When present, each non-`None` hint is surfaced on the MCP
    /// `tools/list` tool definition as a spec-compliant camelCase field
    /// (`readOnlyHint`, `destructiveHint`, …). The dcc-mcp-core-specific
    /// `deferred_hint` lands in `_meta["dcc.deferred_hint"]` rather than
    /// inside the spec `annotations` map.
    #[serde(default, skip_serializing_if = "ToolAnnotations::is_empty")]
    pub annotations: ToolAnnotations,
    /// Suggested follow-up tools surfaced on `CallToolResult._meta`
    /// under `dcc.next_tools` (issue #342).
    ///
    /// Populated from the per-tool `next-tools` entry in the sibling
    /// `tools.yaml` file. Tool names must pass
    /// [`dcc_mcp_naming::validate_tool_name`]; invalid entries are
    /// dropped at skill-load time with a warning.
    #[serde(default, skip_serializing_if = "next_tools_is_empty")]
    pub next_tools: NextTools,
}

fn next_tools_is_empty(next_tools: &NextTools) -> bool {
    next_tools.on_success.is_empty() && next_tools.on_failure.is_empty()
}

fn is_default_thread_affinity(affinity: &ThreadAffinity) -> bool {
    matches!(affinity, ThreadAffinity::Any)
}

fn default_enabled() -> bool {
    true
}

impl Default for ActionMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            category: String::new(),
            tags: Vec::new(),
            dcc: String::new(),
            version: String::new(),
            input_schema: serde_json::Value::Null,
            output_schema: serde_json::Value::Null,
            source_file: None,
            skill_name: None,
            group: String::new(),
            enabled: true,
            required_capabilities: Vec::new(),
            execution: ExecutionMode::Sync,
            timeout_hint_secs: None,
            thread_affinity: ThreadAffinity::Any,
            annotations: ToolAnnotations::default(),
            next_tools: NextTools::default(),
        }
    }
}
