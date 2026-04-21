//! SkillMetadata — parsed from SKILL.md frontmatter.
//!
//! Supports three skill standards simultaneously:
//!
//! - **agentskills.io / Anthropic Skills**: `name`, `description`, `license`,
//!   `compatibility`, `metadata`, `allowed-tools`
//! - **ClawHub / OpenClaw**: `version`, `metadata.openclaw.*` (requires, install,
//!   primaryEnv, emoji, homepage, os, always, skillKey)
//! - **dcc-mcp-core extensions**: `dcc`, `tags`, `tools`, `depends`, `scripts`
//!
//! The same SKILL.md file can satisfy all three formats simultaneously.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── ExecutionMode ─────────────────────────────────────────────────────────

/// How a tool is expected to execute with respect to request/response latency.
///
/// Authors declare `execution` in SKILL.md. The MCP server derives the
/// `deferredHint` annotation from this value (per MCP 2025-03-26 the hint
/// is server-set — end users should not set it directly). See issue #317.
///
/// ```yaml
/// tools:
///   - name: render_frames
///     execution: async          # sync | async ; default sync
///     timeout_hint_secs: 600    # optional u32
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Returns quickly — callers expect a synchronous reply.
    #[default]
    Sync,
    /// May take long enough that clients should treat the call as deferred.
    /// Surfaces as `deferredHint: true` on the MCP tool annotation.
    Async,
}

impl ExecutionMode {
    /// Whether this mode should surface as a deferred hint in MCP tool
    /// annotations.
    #[must_use]
    pub fn is_deferred(self) -> bool {
        matches!(self, Self::Async)
    }
}

// ── ThreadAffinity (issue #332) ───────────────────────────────────────────

/// Where a tool is allowed to execute.
///
/// Skill authors declare `thread-affinity` in SKILL.md / `tools.yaml` for tools
/// that must run on the DCC application's main thread (e.g. anything that
/// touches `maya.cmds`, `bpy.ops`, `hou.*`, `pymxs.runtime`).
///
/// The HTTP server reads this value at dispatch time — main-affined tools are
/// routed through [`DeferredExecutor`] even when the caller used the async
/// `tools/call` path (#318). `Any` (default) tools execute on a Tokio worker.
///
/// This mirrors [`dcc_mcp_process::dispatcher::ThreadAffinity`] with the
/// `Named` variant dropped — named threads are an adapter concern that never
/// travels through the skill-metadata layer.
///
/// Examples:
///
/// ```rust
/// use dcc_mcp_models::ThreadAffinity;
///
/// assert_eq!(ThreadAffinity::default(), ThreadAffinity::Any);
/// let v = serde_json::to_string(&ThreadAffinity::Main).unwrap();
/// assert_eq!(v, "\"main\"");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadAffinity {
    /// No constraint — the tool may run on any worker thread.
    #[default]
    Any,
    /// Must run on the DCC application's main thread.
    Main,
}

impl ThreadAffinity {
    /// Parse a case-insensitive affinity string — returns `None` for unknown
    /// values so callers can decide between defaulting and rejecting.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "any" | "" => Some(Self::Any),
            "main" => Some(Self::Main),
            _ => None,
        }
    }

    /// Human-readable lowercase tag suitable for MCP `_meta` surfaces.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Main => "main",
        }
    }

    /// Whether the tool must run on the DCC main thread.
    #[must_use]
    pub fn is_main(self) -> bool {
        matches!(self, Self::Main)
    }
}

impl std::fmt::Display for ThreadAffinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── ToolAnnotations ───────────────────────────────────────────────────────

/// MCP tool behavioural annotations declared in the sibling `tools.yaml`
/// file (or the SKILL.md `tools:` list).
///
/// This mirrors the spec-defined `ToolAnnotations` object from MCP
/// 2025-03-26 — all fields are optional, missing fields stay `None`.
/// The one dcc-mcp-core-specific extension is `deferred_hint`, which is
/// surfaced in the tool declaration's `_meta` slot (never inside the
/// spec-standard `annotations` map — see issue #344).
///
/// ```yaml
/// tools:
///   - name: delete_keyframes
///     annotations:
///       read_only_hint: false
///       destructive_hint: true
///       idempotent_hint: true
///       open_world_hint: false
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAnnotations {
    /// Human-readable display title for the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether this tool only reads data (no side effects).
    #[serde(
        default,
        rename = "read_only_hint",
        alias = "readOnlyHint",
        alias = "read-only-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub read_only_hint: Option<bool>,

    /// Whether this tool may cause irreversible destructive changes.
    #[serde(
        default,
        rename = "destructive_hint",
        alias = "destructiveHint",
        alias = "destructive-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub destructive_hint: Option<bool>,

    /// Whether repeated calls with the same args produce the same result.
    #[serde(
        default,
        rename = "idempotent_hint",
        alias = "idempotentHint",
        alias = "idempotent-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub idempotent_hint: Option<bool>,

    /// Whether the tool may interact with external, open-world systems.
    #[serde(
        default,
        rename = "open_world_hint",
        alias = "openWorldHint",
        alias = "open-world-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub open_world_hint: Option<bool>,

    /// dcc-mcp-core extension — signals that the tool declaration is a
    /// deferred stub (full schema arrives on `load_skill`).  Surfaces in
    /// `_meta["dcc.deferred_hint"]`, **not** in the spec `annotations`
    /// field.
    #[serde(
        default,
        rename = "deferred_hint",
        alias = "deferredHint",
        alias = "deferred-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub deferred_hint: Option<bool>,
}

impl ToolAnnotations {
    /// Return `true` when every hint field is `None` — used to decide
    /// whether to emit an `annotations:` object at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.read_only_hint.is_none()
            && self.destructive_hint.is_none()
            && self.idempotent_hint.is_none()
            && self.open_world_hint.is_none()
            && self.deferred_hint.is_none()
    }

    /// Same as [`Self::is_empty`] but ignores the `deferred_hint`
    /// extension (which lives outside the spec `annotations` map).
    #[must_use]
    pub fn is_spec_empty(&self) -> bool {
        self.title.is_none()
            && self.read_only_hint.is_none()
            && self.destructive_hint.is_none()
            && self.idempotent_hint.is_none()
            && self.open_world_hint.is_none()
    }
}

// ── ToolDeclaration ───────────────────────────────────────────────────────

/// Declaration of a tool provided by a skill, parsed from SKILL.md frontmatter.
///
/// Unlike `ActionMeta`, this is a lightweight declaration that can be discovered
/// without loading the skill's scripts. It carries enough information for agents
/// to decide whether to load a skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolDeclaration", eq, from_py_object)
)]
pub struct ToolDeclaration {
    /// Tool name (unique within the skill).
    #[serde(default)]
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// JSON Schema for input parameters (as serde_json::Value).
    #[serde(default)]
    pub input_schema: serde_json::Value,

    /// JSON Schema for output (as serde_json::Value).
    #[serde(default, skip_serializing_if = "is_null_value")]
    pub output_schema: serde_json::Value,

    /// Whether this tool only reads data (no side effects).
    #[serde(default)]
    pub read_only: bool,

    /// Whether this tool may cause destructive changes.
    #[serde(default)]
    pub destructive: bool,

    /// Whether calling this tool with the same args always produces the same result.
    #[serde(default)]
    pub idempotent: bool,

    /// Whether this declaration should be surfaced as deferred in discovery-oriented UIs.
    ///
    /// Supports both `defer-loading` and `defer_loading` in SKILL.md frontmatter.
    #[serde(default, rename = "defer-loading", alias = "defer_loading")]
    pub defer_loading: bool,

    /// Explicit path to the script that implements this tool.
    ///
    /// If empty, the catalog will try to find a matching script by name.
    ///
    /// Example in SKILL.md:
    /// ```yaml
    /// tools:
    ///   - name: create_mesh
    ///     source_file: scripts/create.py
    /// ```
    #[serde(default)]
    pub source_file: String,

    /// Suggested follow-up tools for progressive discovery (issue #143).
    ///
    /// Agents can use this to chain tool calls without pre-training.
    ///
    /// ```yaml
    /// tools:
    ///   - name: export_fbx
    ///     next-tools:
    ///       on-success: [validate_naming, inspect_usd]
    ///       on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
    /// ```
    #[serde(default, rename = "next-tools", alias = "next_tools")]
    pub next_tools: NextTools,

    /// Tool group this declaration belongs to (progressive exposure).
    ///
    /// Empty string ``""`` means the tool is always active (default group).
    /// Non-empty values reference a :struct:`SkillGroup` declared in the
    /// skill's `groups:` list. Tools in an inactive group are hidden behind
    /// a ``__group__<skill>__<name>`` stub in ``tools/list`` until the agent
    /// calls ``activate_tool_group``.
    #[serde(default)]
    pub group: String,

    /// Execution mode — `sync` (default) or `async`.
    ///
    /// Drives the server-derived `deferredHint` annotation on the MCP tool
    /// definition. See [`ExecutionMode`] and issue #317.
    #[serde(default)]
    pub execution: ExecutionMode,

    /// Optional hint about typical execution time in seconds.
    ///
    /// When set, surfaces under the tool's `_meta.dcc.timeoutHintSecs` in
    /// `tools/list` (never inside `annotations`). Clients may use this to
    /// size their own request timeouts.
    #[serde(
        default,
        rename = "timeout_hint_secs",
        alias = "timeout-hint-secs",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_hint_secs: Option<u32>,

    /// Thread-affinity hint — either `any` (default) or `main` (issue #332).
    ///
    /// When `main`, the HTTP server routes this tool through
    /// [`DeferredExecutor`] even along the async-dispatch path (#318) so the
    /// DCC's main-thread-only APIs (`maya.cmds`, `bpy.ops`, `hou.*`) see a
    /// safe execution context. `any` tools execute on a Tokio worker.
    #[serde(
        default,
        rename = "thread-affinity",
        alias = "thread_affinity",
        skip_serializing_if = "is_default_affinity"
    )]
    pub thread_affinity: ThreadAffinity,

    /// Reject the legacy user-level `deferred: true` flag with a clear error.
    ///
    /// `deferredHint` is server-set per MCP 2025-03-26; skill authors must
    /// use `execution: async` instead. Always deserialises to `None`; the
    /// presence of the key triggers a custom-deserialiser error.
    #[serde(default, skip_serializing)]
    pub _deferred_guard: Option<()>,

    /// MCP tool annotations declared in the sibling `tools.yaml` file.
    ///
    /// Issue #344 — supports two forms in the YAML source:
    ///
    /// 1. Canonical nested map:
    ///    ```yaml
    ///    tools:
    ///      - name: delete_keyframes
    ///        annotations:
    ///          read_only_hint: false
    ///          destructive_hint: true
    ///    ```
    /// 2. Shorthand top-level hint keys (backward compatibility):
    ///    ```yaml
    ///    tools:
    ///      - name: get_keyframes
    ///        read_only_hint: true
    ///        idempotent_hint: true
    ///    ```
    ///
    /// When both forms are present for the same tool, the nested
    /// `annotations:` map wins entirely (whole-map replacement, not
    /// per-field merge).
    #[serde(default, skip_serializing_if = "ToolAnnotations::is_empty")]
    pub annotations: ToolAnnotations,
}

// ── ToolDeclaration custom deserializer (issue #344) ──────────────────────
//
// We deserialize via an intermediate "wire" struct so we can:
//   * reject the legacy top-level `deferred:` field with a clear error,
//   * fold the shorthand hint keys (`read_only_hint`, `destructive_hint`,
//     `idempotent_hint`, `open_world_hint`, `deferred_hint`) that sit
//     directly on the tool entry into `ToolAnnotations`,
//   * honour the canonical nested `annotations:` map when present — and
//     have it win whole-map over the shorthand form.
impl<'de> serde::Deserialize<'de> for ToolDeclaration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Wire {
            name: String,
            description: String,
            input_schema: serde_json::Value,
            #[serde(default)]
            output_schema: serde_json::Value,
            read_only: bool,
            destructive: bool,
            idempotent: bool,
            #[serde(rename = "defer-loading", alias = "defer_loading")]
            defer_loading: bool,
            source_file: String,
            #[serde(rename = "next-tools", alias = "next_tools")]
            next_tools: NextTools,
            group: String,
            execution: ExecutionMode,
            #[serde(rename = "timeout_hint_secs", alias = "timeout-hint-secs")]
            timeout_hint_secs: Option<u32>,
            #[serde(rename = "thread-affinity", alias = "thread_affinity")]
            thread_affinity: ThreadAffinity,

            /// Legacy user-level `deferred:` flag — rejected below.
            #[serde(rename = "deferred")]
            deferred: Option<serde_json::Value>,

            /// Canonical nested annotations map (wins when present).
            #[serde(default)]
            annotations: Option<ToolAnnotations>,

            // Shorthand hint keys that sit directly on the tool entry
            // (backward compatibility). Accept snake_case, camelCase and
            // kebab-case for each.
            #[serde(
                default,
                rename = "read_only_hint",
                alias = "readOnlyHint",
                alias = "read-only-hint"
            )]
            read_only_hint: Option<bool>,
            #[serde(
                default,
                rename = "destructive_hint",
                alias = "destructiveHint",
                alias = "destructive-hint"
            )]
            destructive_hint: Option<bool>,
            #[serde(
                default,
                rename = "idempotent_hint",
                alias = "idempotentHint",
                alias = "idempotent-hint"
            )]
            idempotent_hint: Option<bool>,
            #[serde(
                default,
                rename = "open_world_hint",
                alias = "openWorldHint",
                alias = "open-world-hint"
            )]
            open_world_hint: Option<bool>,
            #[serde(
                default,
                rename = "deferred_hint",
                alias = "deferredHint",
                alias = "deferred-hint"
            )]
            deferred_hint: Option<bool>,
        }

        let w = Wire::deserialize(deserializer)?;

        if w.deferred.is_some() {
            return Err(D::Error::custom(
                "`deferred` is not a user-level SKILL.md field — it is server-derived per \
                 MCP 2025-03-26. Declare `execution: async` instead (see issue #317).",
            ));
        }

        // Build the final annotations: nested map wins entirely; otherwise
        // promote the shorthand keys.
        let annotations = if let Some(nested) = w.annotations {
            nested
        } else {
            ToolAnnotations {
                title: None,
                read_only_hint: w.read_only_hint,
                destructive_hint: w.destructive_hint,
                idempotent_hint: w.idempotent_hint,
                open_world_hint: w.open_world_hint,
                deferred_hint: w.deferred_hint,
            }
        };

        Ok(Self {
            name: w.name,
            description: w.description,
            input_schema: w.input_schema,
            output_schema: w.output_schema,
            read_only: w.read_only,
            destructive: w.destructive,
            idempotent: w.idempotent,
            defer_loading: w.defer_loading,
            source_file: w.source_file,
            next_tools: w.next_tools,
            group: w.group,
            execution: w.execution,
            timeout_hint_secs: w.timeout_hint_secs,
            thread_affinity: w.thread_affinity,
            _deferred_guard: None,
            annotations,
        })
    }
}

/// Suggested next tools for a successful or failed tool call (issue #143).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextTools {
    /// Tool names to suggest after a successful invocation.
    #[serde(default, rename = "on-success", alias = "on_success")]
    pub on_success: Vec<String>,

    /// Tool names to suggest after a failed invocation.
    #[serde(default, rename = "on-failure", alias = "on_failure")]
    pub on_failure: Vec<String>,
}

// ── SkillGroup ─────────────────────────────────────────────────────────────

/// Declaration of a tool group within a skill (progressive exposure).
///
/// A group bundles multiple tools behind a single stub entry in ``tools/list``
/// so agents only pay the context cost for the tools they actually use.
///
/// ```yaml
/// groups:
///   - name: uv-editing
///     description: UV-space operations
///     default-active: false
///     tools: [unwrap, layout_uvs, transfer_uvs]
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillGroup", eq, from_py_object)
)]
pub struct SkillGroup {
    /// Group identifier — unique within the skill (kebab-case recommended).
    #[serde(default)]
    pub name: String,

    /// Human-readable summary of what the group offers.
    #[serde(default)]
    pub description: String,

    /// Names of tools belonging to this group.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Whether this group is active by default when the skill is loaded.
    #[serde(default, rename = "default-active", alias = "default_active")]
    pub default_active: bool,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillGroup {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), tools=Vec::<String>::new(), default_active=false))]
    fn new(name: String, description: String, tools: Vec<String>, default_active: bool) -> Self {
        Self {
            name,
            description,
            tools,
            default_active,
        }
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }

    #[getter]
    fn tools(&self) -> Vec<String> {
        self.tools.clone()
    }

    #[getter]
    fn default_active(&self) -> bool {
        self.default_active
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillGroup(name={:?}, tools={}, default_active={})",
            self.name,
            self.tools.len(),
            self.default_active
        )
    }
}

fn is_null_value(v: &serde_json::Value) -> bool {
    v.is_null()
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolDeclaration {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), input_schema=None, output_schema=None, read_only=false, destructive=false, idempotent=false, defer_loading=false, source_file="".to_string(), group="".to_string(), execution="sync".to_string(), timeout_hint_secs=None, thread_affinity="any".to_string()))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        input_schema: Option<String>,
        output_schema: Option<String>,
        read_only: bool,
        destructive: bool,
        idempotent: bool,
        defer_loading: bool,
        source_file: String,
        group: String,
        execution: String,
        timeout_hint_secs: Option<u32>,
        thread_affinity: String,
    ) -> pyo3::PyResult<Self> {
        let input_schema = input_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({"type": "object"}));
        let output_schema = output_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        let execution = match execution.as_str() {
            "sync" => ExecutionMode::Sync,
            "async" => ExecutionMode::Async,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "execution must be 'sync' or 'async' (got {other:?})",
                )));
            }
        };
        let thread_affinity = ThreadAffinity::parse(&thread_affinity).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "thread_affinity must be 'any' or 'main' (got {thread_affinity:?})"
            ))
        })?;
        Ok(Self {
            name,
            description,
            input_schema,
            output_schema,
            read_only,
            destructive,
            idempotent,
            defer_loading,
            source_file,
            next_tools: NextTools::default(),
            group,
            execution,
            timeout_hint_secs,
            thread_affinity,
            _deferred_guard: None,
            annotations: ToolAnnotations::default(),
        })
    }

    #[getter]
    fn execution(&self) -> &'static str {
        match self.execution {
            ExecutionMode::Sync => "sync",
            ExecutionMode::Async => "async",
        }
    }

    #[setter]
    fn set_execution(&mut self, value: String) -> pyo3::PyResult<()> {
        self.execution = match value.as_str() {
            "sync" => ExecutionMode::Sync,
            "async" => ExecutionMode::Async,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "execution must be 'sync' or 'async' (got {other:?})",
                )));
            }
        };
        Ok(())
    }

    #[getter]
    fn timeout_hint_secs(&self) -> Option<u32> {
        self.timeout_hint_secs
    }

    #[setter]
    fn set_timeout_hint_secs(&mut self, value: Option<u32>) {
        self.timeout_hint_secs = value;
    }

    #[getter]
    fn group(&self) -> &str {
        &self.group
    }

    #[setter]
    fn set_group(&mut self, value: String) {
        self.group = value;
    }

    fn __repr__(&self) -> String {
        format!("ToolDeclaration(name={:?})", self.name)
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[setter]
    fn set_name(&mut self, value: String) {
        self.name = value;
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }

    #[setter]
    fn set_description(&mut self, value: String) {
        self.description = value;
    }

    /// Returns input_schema as a JSON string.
    #[getter]
    fn input_schema(&self) -> String {
        self.input_schema.to_string()
    }

    /// Set input_schema from a JSON string.
    #[setter]
    fn set_input_schema(&mut self, value: String) {
        self.input_schema =
            serde_json::from_str(&value).unwrap_or(serde_json::json!({"type": "object"}));
    }

    /// Returns output_schema as a JSON string (empty string if null).
    #[getter]
    fn output_schema(&self) -> String {
        if self.output_schema.is_null() {
            String::new()
        } else {
            self.output_schema.to_string()
        }
    }

    /// Set output_schema from a JSON string.
    #[setter]
    fn set_output_schema(&mut self, value: String) {
        self.output_schema = if value.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&value).unwrap_or(serde_json::Value::Null)
        };
    }

    #[getter]
    fn read_only(&self) -> bool {
        self.read_only
    }

    #[setter]
    fn set_read_only(&mut self, value: bool) {
        self.read_only = value;
    }

    #[getter]
    fn destructive(&self) -> bool {
        self.destructive
    }

    #[setter]
    fn set_destructive(&mut self, value: bool) {
        self.destructive = value;
    }

    #[getter]
    fn idempotent(&self) -> bool {
        self.idempotent
    }

    #[setter]
    fn set_idempotent(&mut self, value: bool) {
        self.idempotent = value;
    }

    #[getter]
    fn defer_loading(&self) -> bool {
        self.defer_loading
    }

    #[setter]
    fn set_defer_loading(&mut self, value: bool) {
        self.defer_loading = value;
    }

    #[getter]
    fn source_file(&self) -> &str {
        &self.source_file
    }

    #[setter]
    fn set_source_file(&mut self, value: String) {
        self.source_file = value;
    }

    /// Return the declared MCP tool annotations as a Python dict.
    ///
    /// Keys use MCP-spec camelCase (`readOnlyHint`, `destructiveHint`,
    /// `idempotentHint`, `openWorldHint`). The dcc-mcp-core-specific
    /// `deferredHint` key is included when set (it lives in `_meta` on
    /// `tools/list`, but is exposed here for convenience).
    /// Missing fields are omitted entirely.
    #[getter]
    fn annotations(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use pyo3::types::PyDict;
        let d = PyDict::new(py);
        if let Some(v) = &self.annotations.title {
            d.set_item("title", v)?;
        }
        if let Some(v) = self.annotations.read_only_hint {
            d.set_item("readOnlyHint", v)?;
        }
        if let Some(v) = self.annotations.destructive_hint {
            d.set_item("destructiveHint", v)?;
        }
        if let Some(v) = self.annotations.idempotent_hint {
            d.set_item("idempotentHint", v)?;
        }
        if let Some(v) = self.annotations.open_world_hint {
            d.set_item("openWorldHint", v)?;
        }
        if let Some(v) = self.annotations.deferred_hint {
            d.set_item("deferredHint", v)?;
        }
        Ok(d.into_any().unbind())
    }

    /// Set the tool annotations from a Python mapping.
    ///
    /// Accepts both snake_case (`read_only_hint`) and camelCase
    /// (`readOnlyHint`) keys.  Unknown keys are ignored.  Pass `None` or
    /// an empty dict to clear.
    #[setter]
    fn set_annotations(
        &mut self,
        py: pyo3::Python<'_>,
        value: Option<Py<PyAny>>,
    ) -> pyo3::PyResult<()> {
        use pyo3::types::PyDict;
        let Some(obj) = value else {
            self.annotations = ToolAnnotations::default();
            return Ok(());
        };
        let bound = obj.bind(py);
        if bound.is_none() {
            self.annotations = ToolAnnotations::default();
            return Ok(());
        }
        let d = bound.cast::<PyDict>().map_err(|_| {
            pyo3::exceptions::PyTypeError::new_err("annotations must be a dict or None")
        })?;

        fn get_bool(d: &pyo3::Bound<'_, PyDict>, keys: &[&str]) -> pyo3::PyResult<Option<bool>> {
            for k in keys {
                if let Some(v) = d.get_item(k)? {
                    if v.is_none() {
                        return Ok(None);
                    }
                    return Ok(Some(v.extract::<bool>()?));
                }
            }
            Ok(None)
        }
        fn get_str(d: &pyo3::Bound<'_, PyDict>, keys: &[&str]) -> pyo3::PyResult<Option<String>> {
            for k in keys {
                if let Some(v) = d.get_item(k)? {
                    if v.is_none() {
                        return Ok(None);
                    }
                    return Ok(Some(v.extract::<String>()?));
                }
            }
            Ok(None)
        }

        self.annotations = ToolAnnotations {
            title: get_str(d, &["title"])?,
            read_only_hint: get_bool(d, &["read_only_hint", "readOnlyHint"])?,
            destructive_hint: get_bool(d, &["destructive_hint", "destructiveHint"])?,
            idempotent_hint: get_bool(d, &["idempotent_hint", "idempotentHint"])?,
            open_world_hint: get_bool(d, &["open_world_hint", "openWorldHint"])?,
            deferred_hint: get_bool(d, &["deferred_hint", "deferredHint"])?,
        };
        Ok(())
    }

    /// Suggested follow-up tools for this declaration (issue #342).
    ///
    /// Returns ``None`` when neither ``on-success`` nor ``on-failure``
    /// was declared. Otherwise returns a dict with string-list values
    /// under the ``"on_success"`` / ``"on_failure"`` keys.
    #[getter]
    fn next_tools<'py>(
        &self,
        py: Python<'py>,
    ) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::types::PyDict>>> {
        if self.next_tools.on_success.is_empty() && self.next_tools.on_failure.is_empty() {
            return Ok(None);
        }
        let d = pyo3::types::PyDict::new(py);
        d.set_item("on_success", self.next_tools.on_success.clone())?;
        d.set_item("on_failure", self.next_tools.on_failure.clone())?;
        Ok(Some(d))
    }

    /// Set ``next_tools`` from a dict with optional ``on_success`` /
    /// ``on_failure`` list-of-string values. Passing ``None`` clears
    /// both lists.
    #[setter]
    fn set_next_tools(
        &mut self,
        value: Option<&pyo3::Bound<'_, pyo3::types::PyAny>>,
    ) -> pyo3::PyResult<()> {
        use pyo3::types::PyDict;
        let Some(v) = value else {
            self.next_tools = NextTools::default();
            return Ok(());
        };
        if v.is_none() {
            self.next_tools = NextTools::default();
            return Ok(());
        }
        let dict = v.downcast::<PyDict>().map_err(|_| {
            pyo3::exceptions::PyTypeError::new_err(
                "next_tools must be a dict with optional on_success/on_failure list keys, or None",
            )
        })?;
        let on_success: Vec<String> = dict
            .get_item("on_success")
            .ok()
            .flatten()
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let on_failure: Vec<String> = dict
            .get_item("on_failure")
            .ok()
            .flatten()
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        self.next_tools = NextTools {
            on_success,
            on_failure,
        };
        Ok(())
    }
}

impl std::fmt::Display for ToolDeclaration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolDeclaration({})", self.name)
    }
}

// ── SkillPolicy ───────────────────────────────────────────────────────────

/// Invocation policy declared in the SKILL.md frontmatter.
///
/// Controls how AI agents may invoke this skill.
///
/// ```yaml
/// policy:
///   allow_implicit_invocation: false   # default: true
///   products: ["maya", "houdini"]      # empty = all products
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillPolicy {
    /// When `false`, the skill must be explicitly loaded via `load_skill`
    /// before any of its tools can be called.  Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_implicit_invocation: Option<bool>,

    /// Restricts this skill to specific DCC products (case-insensitive).
    /// An empty list means the skill is available for all products.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<String>,
}

impl SkillPolicy {
    /// Returns `true` if implicit invocation is allowed (default when absent).
    pub fn is_implicit_invocation_allowed(&self) -> bool {
        self.allow_implicit_invocation.unwrap_or(true)
    }

    /// Returns `true` if this skill is available for the given DCC product.
    /// Empty `products` list means available for all.
    pub fn matches_product(&self, product: &str) -> bool {
        self.products.is_empty()
            || self
                .products
                .iter()
                .any(|p| p.eq_ignore_ascii_case(product))
    }
}

// ── SkillDependencies ─────────────────────────────────────────────────────

/// Category of an external dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillDependencyType {
    /// Requires a running MCP server.
    #[default]
    Mcp,
    /// Requires an environment variable to be set.
    EnvVar,
    /// Requires a binary to be present on `$PATH`.
    Bin,
}

impl std::fmt::Display for SkillDependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mcp => write!(f, "mcp"),
            Self::EnvVar => write!(f, "env_var"),
            Self::Bin => write!(f, "bin"),
        }
    }
}

/// A single external dependency declared by a skill.
///
/// ```yaml
/// external_deps:
///   tools:
///     - type: mcp
///       value: "render-server"
///       description: "Needs the render MCP server"
///       transport: stdio
///       command: "python -m render_mcp"
///     - type: env_var
///       value: "MAYA_LICENSE_KEY"
///       description: "Maya license key must be set"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillDependency {
    /// Dependency category.
    #[serde(default, rename = "type")]
    pub dep_type: SkillDependencyType,

    /// Identifier: server name, env-var name, or binary name.
    #[serde(default)]
    pub value: String,

    /// Human-readable explanation shown when dependency is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MCP transport (`"stdio"`, `"http"`, …) — only for `Mcp` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,

    /// Command to launch the MCP server — only for `Mcp`/`stdio` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// URL of the MCP server — only for `Mcp`/`http` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// External dependency declarations for a skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillDependencies {
    /// List of external tool / server / environment dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<SkillDependency>,
}

impl SkillDependencies {
    /// `true` when no dependencies are declared.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Iterator over `Mcp`-type dependencies.
    pub fn mcp_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::Mcp)
    }

    /// Iterator over `EnvVar`-type dependencies.
    pub fn env_var_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::EnvVar)
    }

    /// Iterator over `Bin`-type dependencies.
    pub fn bin_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::Bin)
    }
}

// ── SkillMetadata ─────────────────────────────────────────────────────────

/// Metadata parsed from a SKILL.md frontmatter.
///
/// Supports all three skill standards:
///
/// ## Minimal (agentskills.io compatible)
/// ```yaml
/// ---
/// name: my-skill
/// description: What it does and when to use it.
/// ---
/// ```
///
/// ## Full (all standards)
/// ```yaml
/// ---
/// name: maya-bevel
/// description: Bevel tools for Maya polygon modeling.
/// # agentskills.io standard
/// license: MIT
/// compatibility: Maya 2022+, Python 3.7+
/// allowed-tools: Bash Read
/// metadata:
///   author: studio-name
///   category: modeling
/// # ClawHub / OpenClaw
/// version: "1.0.0"
/// metadata:
///   openclaw:
///     requires:
///       bins: [maya]
///     emoji: "🎨"
///     homepage: https://example.com
/// # dcc-mcp-core extensions
/// dcc: maya
/// tags: [modeling, polygon]
/// tools:
///   - name: bevel
///     description: Apply bevel to selected edges
/// ---
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillMetadata", from_py_object)
)]
pub struct SkillMetadata {
    /// Skill identifier — lowercase, hyphens only.
    /// Must match the parent directory name (agentskills.io requirement).
    pub name: String,

    /// Human-readable description of what the skill does and when to use it.
    /// Shown in skill discovery results. Keep under 1024 chars.
    #[serde(default)]
    pub description: String,

    // ── agentskills.io / Anthropic Skills standard fields ─────────────
    /// SPDX license identifier or short license description.
    /// Example: `"MIT"`, `"Apache-2.0"`, `"Proprietary"`
    #[serde(default)]
    pub license: String,

    /// Environment and dependency requirements for this skill.
    /// Example: `"Python 3.7+, Maya 2022+"`, `"Requires docker and git"`
    /// Keep under 500 chars.
    #[serde(default)]
    pub compatibility: String,

    /// Pre-approved tools this skill may use (agentskills.io `allowed-tools`).
    /// Space-delimited in SKILL.md YAML, stored as Vec<String> here.
    ///
    /// This is distinct from `tools` (MCP tool declarations):
    /// - `allowed-tools`: permission whitelist for agent capabilities (e.g. `["Bash", "Read"]`)
    /// - `tools`: MCP tool definitions with schemas
    ///
    /// Supports both space-delimited strings and YAML lists:
    /// ```yaml
    /// allowed-tools: Bash Read Write
    /// # or:
    /// allowed-tools: [Bash, Read, Write]
    /// ```
    #[serde(
        default,
        rename = "allowed-tools",
        alias = "allowed_tools",
        deserialize_with = "deserialize_allowed_tools"
    )]
    pub allowed_tools: Vec<String>,

    /// Arbitrary metadata key-value pairs.
    ///
    /// Used by both agentskills.io (flat KV strings) and ClawHub (`openclaw.*`
    /// nested structure). Stored as a JSON value to support both:
    ///
    /// ```yaml
    /// # agentskills.io flat style
    /// metadata:
    ///   author: studio-name
    ///   category: modeling
    ///
    /// # ClawHub nested style
    /// metadata:
    ///   openclaw:
    ///     requires:
    ///       bins: [ffmpeg]
    ///     emoji: "🎬"
    /// ```
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,

    // ── dcc-mcp-core extension fields ─────────────────────────────────
    /// Target DCC application (e.g. "maya", "blender", "houdini", "python").
    #[serde(default = "default_dcc")]
    pub dcc: String,

    /// Searchable tags for skill discovery.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Short search hint for lightweight skill discovery.
    ///
    /// Used by `search_skills` to match against without loading full tool schemas.
    /// Should be a comma-separated list of keywords or a short phrase, e.g.:
    /// `"polygon modeling, bevel, extrude, mesh editing"`
    ///
    /// Falls back to `description` if not set.
    #[serde(default, rename = "search-hint", alias = "search_hint")]
    pub search_hint: String,

    /// MCP tool declarations — defines the tools this skill exposes.
    ///
    /// Accepts both simple names and full declarations:
    /// ```yaml
    /// tools: ["bevel", "extrude"]
    /// # or with full schema:
    /// tools:
    ///   - name: bevel
    ///     description: Apply bevel to edges
    ///     source_file: scripts/bevel.py
    /// ```
    #[serde(default, deserialize_with = "deserialize_tool_declarations")]
    pub tools: Vec<ToolDeclaration>,

    /// Semantic version string.
    #[serde(default = "default_version")]
    pub version: String,

    /// Skill dependencies — names of other skills this skill requires.
    #[serde(default)]
    pub depends: Vec<String>,

    // ── Runtime-populated fields (not in YAML) ─────────────────────────
    /// Script files discovered in the `scripts/` subdirectory.
    /// Populated at load time, not from SKILL.md frontmatter.
    #[serde(default)]
    pub scripts: Vec<String>,

    /// Absolute path to the skill's root directory.
    /// Populated at load time.
    #[serde(default)]
    pub skill_path: String,

    /// Markdown files discovered in the `metadata/` subdirectory.
    /// Populated at load time.
    #[serde(default)]
    pub metadata_files: Vec<String>,

    // ── dcc-mcp-core: progressive discovery extensions ─────────────────
    /// Invocation policy declared in SKILL.md frontmatter.
    ///
    /// Controls whether the skill may be loaded implicitly and which
    /// DCC products it is available for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<SkillPolicy>,

    /// External dependencies declared in SKILL.md frontmatter.
    ///
    /// Declares required MCP servers, environment variables, or binaries
    /// that must be available for this skill to function correctly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_deps: Option<SkillDependencies>,

    /// Declared tool groups for progressive exposure (see [`SkillGroup`]).
    ///
    /// When a tool declares a group name that is not present in this list,
    /// the catalog auto-inserts an inactive placeholder group at load time.
    #[serde(default)]
    pub groups: Vec<SkillGroup>,

    /// Names of legacy top-level extension fields detected while parsing
    /// this skill's SKILL.md (issue #356).
    ///
    /// Populated by the loader, not by serde. When empty the skill uses the
    /// agentskills.io-compliant `metadata.dcc-mcp.*` form exclusively; when
    /// non-empty the skill still relies on deprecated top-level extension
    /// keys. See [`SkillMetadata::is_spec_compliant`].
    #[serde(default, skip_serializing, skip_deserializing)]
    pub legacy_extension_fields: Vec<String>,

    /// Sibling-file reference for the MCP prompts primitive (issues #351, #355).
    ///
    /// Set from `metadata.dcc-mcp.prompts` in SKILL.md frontmatter. The value
    /// is a path relative to the skill root — either a single YAML file that
    /// contains a top-level `prompts:` (and optional `workflows:`) list, or a
    /// glob (`prompts/*.prompt.yaml`) that enumerates one file per prompt.
    ///
    /// Parsing is deferred until the MCP server handles a `prompts/list` or
    /// `prompts/get` call, so a skill with 50 prompt files pays zero cost at
    /// scan / load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts_file: Option<String>,
}

impl SkillMetadata {
    /// Access the `metadata.openclaw` section if present (ClawHub format).
    ///
    /// Returns `None` if this skill doesn't have ClawHub metadata.
    pub fn openclaw_metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_object().and_then(|m| {
            m.get("openclaw")
                .or_else(|| m.get("clawdbot"))
                .or_else(|| m.get("clawdis"))
        })
    }

    /// Get required environment variables declared by this skill (ClawHub).
    pub fn required_env_vars(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|r| r.get("env"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get required binaries declared by this skill (ClawHub).
    pub fn required_bins(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|r| r.get("bins"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get the primary credential environment variable (ClawHub `primaryEnv`).
    pub fn primary_env(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("primaryEnv"))
            .and_then(|v| v.as_str())
    }

    /// Get the emoji display for this skill (ClawHub).
    pub fn emoji(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("emoji"))
            .and_then(|v| v.as_str())
    }

    /// Get the homepage URL for this skill (ClawHub).
    pub fn homepage(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("homepage"))
            .and_then(|v| v.as_str())
    }

    /// Whether this skill is always active (no explicit load needed) (ClawHub `always`).
    pub fn always_active(&self) -> bool {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("always"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Get OS restrictions for this skill (ClawHub `os`).
    pub fn os_restrictions(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("os"))
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get flat metadata key-value pairs (agentskills.io style).
    ///
    /// Returns only top-level string values, ignoring nested objects (like `openclaw`).
    pub fn flat_metadata(&self) -> HashMap<&str, &str> {
        self.metadata
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns `true` iff no legacy top-level extension fields were used
    /// when this skill's SKILL.md was parsed.
    ///
    /// Spec-compliant skills declare all dcc-mcp-specific keys under the
    /// `metadata.dcc-mcp.*` namespace (agentskills.io v1.0). Legacy skills
    /// declared them as top-level YAML fields (`dcc`, `tags`, `tools`, …).
    /// See issue #356.
    pub fn is_spec_compliant(&self) -> bool {
        self.legacy_extension_fields.is_empty()
    }

    /// Returns true if this skill has any validation warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // name: lowercase + hyphens only, max 64 chars
        if self.name.len() > 64 {
            warnings.push(format!(
                "name '{}' exceeds 64 chars (agentskills.io limit)",
                self.name
            ));
        }
        if self.name.starts_with('-') || self.name.ends_with('-') {
            warnings.push(format!(
                "name '{}' must not start or end with a hyphen",
                self.name
            ));
        }
        if self.name.contains("--") {
            warnings.push(format!(
                "name '{}' must not contain consecutive hyphens",
                self.name
            ));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            warnings.push(format!(
                "name '{}' should be lowercase letters, digits, and hyphens only",
                self.name
            ));
        }

        // description: max 1024 chars (agentskills.io)
        if self.description.len() > 1024 {
            warnings.push(format!(
                "description length {} exceeds 1024 chars (agentskills.io limit)",
                self.description.len()
            ));
        }

        // compatibility: max 500 chars (agentskills.io)
        if self.compatibility.len() > 500 {
            warnings.push(format!(
                "compatibility length {} exceeds 500 chars (agentskills.io limit)",
                self.compatibility.len()
            ));
        }

        warnings
    }
}

// ── Deserializers ─────────────────────────────────────────────────────────

/// Deserialize `allowed-tools` from either a space-delimited string or a YAML list.
///
/// Handles:
/// - `allowed-tools: "Bash Read Write"` → `["Bash", "Read", "Write"]`
/// - `allowed-tools: [Bash, Read, Write]` → `["Bash", "Read", "Write"]`
fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct AllowedToolsVisitor;

    impl<'de> Visitor<'de> for AllowedToolsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a space-delimited string or a sequence of tool names")
        }

        // `allowed-tools: "Bash Read Write"` or `allowed-tools: "Bash(git:*) Read"`
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        fn visit_string<E: de::Error>(self, s: String) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        // `allowed-tools: [Bash, Read, Write]`
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut tools = Vec::new();
            while let Some(v) = seq.next_element::<String>()? {
                tools.push(v);
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_any(AllowedToolsVisitor)
}

/// Custom deserializer for `tools` — accepts both string names and full objects.
fn deserialize_tool_declarations<'de, D>(deserializer: D) -> Result<Vec<ToolDeclaration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct ToolDeclarationsVisitor;

    impl<'de> Visitor<'de> for ToolDeclarationsVisitor {
        type Value = Vec<ToolDeclaration>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "a sequence of tool name strings or tool declaration objects"
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut tools = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match &value {
                    serde_json::Value::String(s) => {
                        tools.push(ToolDeclaration {
                            name: s.clone(),
                            ..Default::default()
                        });
                    }
                    serde_json::Value::Object(_) => {
                        let decl: ToolDeclaration =
                            serde_json::from_value(value).map_err(de::Error::custom)?;
                        tools.push(decl);
                    }
                    _ => {
                        return Err(de::Error::custom(
                            "each tool must be a string name or a declaration object",
                        ));
                    }
                }
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_seq(ToolDeclarationsVisitor)
}

fn default_dcc() -> String {
    DEFAULT_DCC.to_string()
}

fn is_default_affinity(affinity: &ThreadAffinity) -> bool {
    matches!(affinity, ThreadAffinity::Any)
}

fn default_version() -> String {
    DEFAULT_VERSION.to_string()
}

// ── Python bindings ───────────────────────────────────────────────────────

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillMetadata {
    #[new]
    #[pyo3(signature = (
        name,
        description = "".to_string(),
        tools = vec![],
        dcc = DEFAULT_DCC.to_string(),
        tags = vec![],
        search_hint = "".to_string(),
        scripts = vec![],
        skill_path = "".to_string(),
        version = DEFAULT_VERSION.to_string(),
        depends = vec![],
        metadata_files = vec![],
        license = "".to_string(),
        compatibility = "".to_string(),
        allowed_tools = vec![],
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        tools: Vec<ToolDeclaration>,
        dcc: String,
        tags: Vec<String>,
        search_hint: String,
        scripts: Vec<String>,
        skill_path: String,
        version: String,
        depends: Vec<String>,
        metadata_files: Vec<String>,
        license: String,
        compatibility: String,
        allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            name,
            description,
            tools,
            dcc,
            tags,
            search_hint,
            scripts,
            skill_path,
            version,
            depends,
            metadata_files,
            license,
            compatibility,
            allowed_tools,
            metadata: serde_json::Value::Null,
            policy: None,
            external_deps: None,
            groups: Vec::new(),
            legacy_extension_fields: Vec::new(),
            prompts_file: None,
        }
    }

    fn __repr__(&self) -> String {
        format!("SkillMetadata(name={:?}, dcc={:?})", self.name, self.dcc)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __eq__(&self, other: &SkillMetadata) -> bool {
        self == other
    }

    // ── Simple field getters/setters ───────────────────────────────────

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }

    #[getter]
    fn dcc(&self) -> &str {
        &self.dcc
    }
    #[setter]
    fn set_dcc(&mut self, v: String) {
        self.dcc = v;
    }

    #[getter]
    fn version(&self) -> &str {
        &self.version
    }
    #[setter]
    fn set_version(&mut self, v: String) {
        self.version = v;
    }

    #[getter]
    fn license(&self) -> &str {
        &self.license
    }
    #[setter]
    fn set_license(&mut self, v: String) {
        self.license = v;
    }

    #[getter]
    fn compatibility(&self) -> &str {
        &self.compatibility
    }
    #[setter]
    fn set_compatibility(&mut self, v: String) {
        self.compatibility = v;
    }

    #[getter]
    fn skill_path(&self) -> &str {
        &self.skill_path
    }
    #[setter]
    fn set_skill_path(&mut self, v: String) {
        self.skill_path = v;
    }

    #[getter]
    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }
    #[setter]
    fn set_tags(&mut self, v: Vec<String>) {
        self.tags = v;
    }

    #[getter]
    fn search_hint(&self) -> &str {
        &self.search_hint
    }
    #[setter]
    fn set_search_hint(&mut self, v: String) {
        self.search_hint = v;
    }

    #[getter]
    fn scripts(&self) -> Vec<String> {
        self.scripts.clone()
    }
    #[setter]
    fn set_scripts(&mut self, v: Vec<String>) {
        self.scripts = v;
    }

    #[getter]
    fn depends(&self) -> Vec<String> {
        self.depends.clone()
    }
    #[setter]
    fn set_depends(&mut self, v: Vec<String>) {
        self.depends = v;
    }

    #[getter]
    fn metadata_files(&self) -> Vec<String> {
        self.metadata_files.clone()
    }
    #[setter]
    fn set_metadata_files(&mut self, v: Vec<String>) {
        self.metadata_files = v;
    }

    #[getter]
    fn allowed_tools(&self) -> Vec<String> {
        self.allowed_tools.clone()
    }
    #[setter]
    fn set_allowed_tools(&mut self, v: Vec<String>) {
        self.allowed_tools = v;
    }

    #[getter]
    fn tools(&self) -> Vec<ToolDeclaration> {
        self.tools.clone()
    }
    #[setter]
    fn set_tools(&mut self, v: Vec<ToolDeclaration>) {
        self.tools = v;
    }

    #[getter]
    fn groups(&self) -> Vec<SkillGroup> {
        self.groups.clone()
    }
    #[setter]
    fn set_groups(&mut self, v: Vec<SkillGroup>) {
        self.groups = v;
    }

    // ── metadata field: JSON value exposed as Python dict ──────────────

    /// Returns metadata as a Python dict.
    #[getter]
    fn metadata(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        let val = if self.metadata.is_null() {
            serde_json::json!({})
        } else {
            self.metadata.clone()
        };
        json_value_to_pyobject(py, &val)
    }

    /// Set metadata from a Python dict (serialized to JSON internally).
    #[setter]
    fn set_metadata(&mut self, py: pyo3::Python<'_>, value: Py<PyAny>) -> pyo3::PyResult<()> {
        use dcc_mcp_utils::py_json::py_any_to_json_value;
        self.metadata = py_any_to_json_value(value.bind(py))
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    // ── policy / external_deps ────────────────────────────────────────

    /// Returns the invocation policy serialised as a JSON string, or `None`.
    ///
    /// Parse with `json.loads(skill.policy)` in Python.
    #[getter]
    fn policy(&self) -> Option<String> {
        self.policy
            .as_ref()
            .and_then(|p| serde_json::to_string(p).ok())
    }

    /// Set the invocation policy from a JSON string (or `None` to clear).
    #[setter]
    fn set_policy(&mut self, value: Option<String>) {
        self.policy = value.and_then(|s| serde_json::from_str::<SkillPolicy>(&s).ok());
    }

    /// Returns `true` if implicit invocation is allowed for this skill.
    #[pyo3(name = "is_implicit_invocation_allowed")]
    fn py_is_implicit_invocation_allowed(&self) -> bool {
        self.policy
            .as_ref()
            .map(|p| p.is_implicit_invocation_allowed())
            .unwrap_or(true)
    }

    /// Returns `true` if this skill is available for the given DCC product.
    #[pyo3(name = "matches_product")]
    fn py_matches_product(&self, product: String) -> bool {
        self.policy
            .as_ref()
            .map(|p| p.matches_product(&product))
            .unwrap_or(true)
    }

    /// Returns the external dependencies serialised as a JSON string, or `None`.
    ///
    /// Parse with `json.loads(skill.external_deps)` in Python.
    #[getter]
    fn external_deps(&self) -> Option<String> {
        self.external_deps
            .as_ref()
            .and_then(|d| serde_json::to_string(d).ok())
    }

    /// Set external dependencies from a JSON string (or `None` to clear).
    #[setter]
    fn set_external_deps(&mut self, value: Option<String>) {
        self.external_deps = value.and_then(|s| serde_json::from_str::<SkillDependencies>(&s).ok());
    }

    // ── ClawHub convenience methods ────────────────────────────────────

    /// Required environment variables (ClawHub `metadata.openclaw.requires.env`).
    #[pyo3(name = "required_env_vars")]
    fn py_required_env_vars(&self) -> Vec<String> {
        SkillMetadata::required_env_vars(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Required binaries (ClawHub `metadata.openclaw.requires.bins`).
    #[pyo3(name = "required_bins")]
    fn py_required_bins(&self) -> Vec<String> {
        SkillMetadata::required_bins(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Primary credential env var (ClawHub `primaryEnv`).
    #[pyo3(name = "primary_env")]
    fn py_primary_env(&self) -> Option<String> {
        SkillMetadata::primary_env(self).map(String::from)
    }

    /// Emoji display (ClawHub).
    #[pyo3(name = "emoji")]
    fn py_emoji(&self) -> Option<String> {
        SkillMetadata::emoji(self).map(String::from)
    }

    /// Homepage URL (ClawHub).
    #[pyo3(name = "homepage")]
    fn py_homepage(&self) -> Option<String> {
        SkillMetadata::homepage(self).map(String::from)
    }

    /// Validate spec constraints. Returns a list of warning strings.
    #[pyo3(name = "validate")]
    fn py_validate(&self) -> Vec<String> {
        SkillMetadata::validate(self)
    }

    /// Returns ``True`` iff this skill uses the agentskills.io-compliant
    /// ``metadata.dcc-mcp.*`` form exclusively (no legacy top-level
    /// extension keys).  See issue #356.
    #[pyo3(name = "is_spec_compliant")]
    fn py_is_spec_compliant(&self) -> bool {
        SkillMetadata::is_spec_compliant(self)
    }

    /// Names of legacy top-level extension fields that were observed when
    /// parsing this skill's SKILL.md.  Empty list ⇒ spec-compliant.
    #[getter]
    fn legacy_extension_fields(&self) -> Vec<String> {
        self.legacy_extension_fields.clone()
    }
}

impl std::fmt::Display for SkillMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{} ({})", self.name, self.version, self.dcc)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deserialization / defaults ──────────────────────────────────────────────

    #[test]
    fn test_skill_metadata_deserialize() {
        let json = r#"{
            "name": "test-skill",
            "description": "A test skill",
            "dcc": "maya",
            "tags": ["geometry", "creation"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.tags, vec!["geometry", "creation"]);
        assert_eq!(meta.version, DEFAULT_VERSION);
        assert!(meta.depends.is_empty());
        assert!(meta.metadata_files.is_empty());
        assert!(meta.license.is_empty());
        assert!(meta.compatibility.is_empty());
        assert!(meta.allowed_tools.is_empty());
        assert!(meta.metadata.is_null());
    }

    #[test]
    fn test_agentskills_standard_fields() {
        let json = r#"{
            "name": "pdf-tools",
            "description": "Extract text from PDFs. Use when working with PDF files.",
            "license": "MIT",
            "compatibility": "Requires Python 3.9+",
            "allowed-tools": "Bash Read Write",
            "metadata": {"author": "studio", "category": "documents"}
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.license, "MIT");
        assert_eq!(meta.compatibility, "Requires Python 3.9+");
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Write"]);
        let flat = meta.flat_metadata();
        assert_eq!(flat.get("author"), Some(&"studio"));
        assert_eq!(flat.get("category"), Some(&"documents"));
    }

    #[test]
    fn test_allowed_tools_yaml_list() {
        let json = r#"{
            "name": "test",
            "allowed-tools": ["Bash", "Read", "Edit"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Edit"]);
    }

    #[test]
    fn test_allowed_tools_alias() {
        let json = r#"{"name": "test", "allowed_tools": ["Bash"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.allowed_tools, vec!["Bash"]);
    }

    #[test]
    fn test_clawhub_metadata_openclaw() {
        let yaml_json = r#"{
            "name": "ffmpeg-media",
            "description": "Media conversion via FFmpeg",
            "version": "1.0.0",
            "metadata": {
                "openclaw": {
                    "requires": {
                        "bins": ["ffmpeg", "ffprobe"],
                        "env": ["FFMPEG_PATH"]
                    },
                    "primaryEnv": "FFMPEG_PATH",
                    "emoji": "🎬",
                    "homepage": "https://ffmpeg.org",
                    "os": ["linux", "macos"],
                    "always": false
                }
            }
        }"#;
        let meta: SkillMetadata = serde_json::from_str(yaml_json).unwrap();
        assert_eq!(meta.required_bins(), vec!["ffmpeg", "ffprobe"]);
        assert_eq!(meta.required_env_vars(), vec!["FFMPEG_PATH"]);
        assert_eq!(meta.primary_env(), Some("FFMPEG_PATH"));
        assert_eq!(meta.emoji(), Some("🎬"));
        assert_eq!(meta.homepage(), Some("https://ffmpeg.org"));
        assert_eq!(meta.os_restrictions(), vec!["linux", "macos"]);
        assert!(!meta.always_active());
    }

    #[test]
    fn test_clawhub_metadata_alias_clawdbot() {
        let json = r#"{
            "name": "test",
            "metadata": {
                "clawdbot": {
                    "emoji": "🦀"
                }
            }
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.emoji(), Some("🦀"));
    }

    #[test]
    fn test_all_three_standards_combined() {
        let json = r#"{
            "name": "maya-bevel",
            "description": "Bevel tools for Maya. Use when beveling polygon edges.",
            "license": "MIT",
            "compatibility": "Maya 2022+, Python 3.7+",
            "allowed-tools": "Bash Read",
            "metadata": {
                "author": "studio",
                "openclaw": {
                    "requires": {"bins": ["maya"]},
                    "emoji": "🎨"
                }
            },
            "dcc": "maya",
            "version": "2.0.0",
            "tags": ["modeling", "polygon"],
            "tools": [
                {"name": "bevel", "description": "Apply bevel to edges"}
            ]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        // agentskills.io fields
        assert_eq!(meta.license, "MIT");
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read"]);
        // ClawHub fields
        assert_eq!(meta.required_bins(), vec!["maya"]);
        assert_eq!(meta.emoji(), Some("🎨"));
        // flat metadata
        assert_eq!(meta.flat_metadata().get("author"), Some(&"studio"));
        // dcc-mcp-core extensions
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.tools[0].name, "bevel");
    }

    #[test]
    fn test_validate_name_constraints() {
        let valid = SkillMetadata {
            name: "my-skill-v2".to_string(),
            ..Default::default()
        };
        assert!(valid.validate().is_empty());

        let too_long = SkillMetadata {
            name: "a".repeat(65),
            ..Default::default()
        };
        assert!(!too_long.validate().is_empty());

        let starts_hyphen = SkillMetadata {
            name: "-bad".to_string(),
            ..Default::default()
        };
        assert!(!starts_hyphen.validate().is_empty());

        let uppercase = SkillMetadata {
            name: "MySkill".to_string(),
            ..Default::default()
        };
        assert!(!uppercase.validate().is_empty());
    }

    #[test]
    fn test_skill_metadata_with_depends() {
        let json = r#"{
            "name": "pipeline",
            "depends": ["geometry-tools", "usd-tools"]
        }"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.depends, vec!["geometry-tools", "usd-tools"]);
    }

    #[test]
    fn test_skill_metadata_display() {
        let meta = SkillMetadata {
            name: "my-skill".to_string(),
            version: "2.0.0".to_string(),
            dcc: "maya".to_string(),
            ..Default::default()
        };
        assert_eq!(meta.to_string(), "my-skill v2.0.0 (maya)");
    }

    #[test]
    fn test_skill_metadata_default_values() {
        let meta = SkillMetadata {
            name: "minimal".to_string(),
            ..Default::default()
        };
        assert_eq!(meta.name, "minimal");
        assert!(meta.tools.is_empty());
        assert!(meta.scripts.is_empty());
        assert!(meta.tags.is_empty());
        assert!(meta.license.is_empty());
        assert!(meta.allowed_tools.is_empty());
    }

    #[test]
    fn test_skill_metadata_serde_round_trip() {
        let meta = SkillMetadata {
            name: "full-skill".to_string(),
            description: "A full skill".to_string(),
            license: "MIT".to_string(),
            compatibility: "Python 3.7+".to_string(),
            allowed_tools: vec!["Bash".to_string(), "Read".to_string()],
            metadata: serde_json::json!({"author": "test"}),
            tools: vec![
                ToolDeclaration {
                    name: "create_mesh".to_string(),
                    ..Default::default()
                },
                ToolDeclaration {
                    name: "delete_mesh".to_string(),
                    ..Default::default()
                },
            ],
            dcc: "blender".to_string(),
            tags: vec!["modeling".to_string()],
            search_hint: "mesh, modeling, geometry".to_string(),
            scripts: vec!["init.py".to_string()],
            skill_path: "/skills/full".to_string(),
            version: "1.2.3".to_string(),
            depends: vec!["base-skill".to_string()],
            metadata_files: vec!["help.md".to_string()],
            policy: None,
            external_deps: None,
            groups: Vec::new(),
            legacy_extension_fields: Vec::new(),
            prompts_file: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: SkillMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn test_skill_metadata_tools_list() {
        let json =
            r#"{"name": "tools-skill", "tools": ["mesh_bevel", "mesh_extrude", "mesh_inset"]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools.len(), 3);
        assert_eq!(meta.tools[0].name, "mesh_bevel");
        assert_eq!(meta.tools[1].name, "mesh_extrude");
        assert_eq!(meta.tools[2].name, "mesh_inset");
    }

    #[test]
    fn test_tool_declaration_full_object() {
        let json = r#"{"name": "tools-skill", "tools": [{"name": "bevel", "description": "Bevel edges", "read_only": false, "destructive": true, "idempotent": true}]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools.len(), 1);
        assert_eq!(meta.tools[0].name, "bevel");
        assert_eq!(meta.tools[0].description, "Bevel edges");
        assert!(!meta.tools[0].read_only);
        assert!(meta.tools[0].destructive);
        assert!(meta.tools[0].idempotent);
    }

    #[test]
    fn test_skill_metadata_deserialize_all_dccs() {
        for dcc in &["maya", "blender", "houdini", "3dsmax", "unreal", "unity"] {
            let json = format!(r#"{{"name": "test", "dcc": "{dcc}"}}"#);
            let meta: SkillMetadata = serde_json::from_str(&json).unwrap();
            assert_eq!(&meta.dcc, dcc);
        }
    }

    #[test]
    fn test_tool_declaration_next_tools() {
        // Test next-tools deserialization (issue #143)
        let json = r#"{"name": "pipeline-skill", "tools": [{
            "name": "export_fbx",
            "description": "Export to FBX",
            "next-tools": {
                "on-success": ["validate_naming", "inspect_usd"],
                "on-failure": ["dcc_diagnostics__screenshot", "dcc_diagnostics__audit_log"]
            }
        }]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools.len(), 1);
        assert_eq!(meta.tools[0].name, "export_fbx");
        assert_eq!(
            meta.tools[0].next_tools.on_success,
            vec!["validate_naming", "inspect_usd"]
        );
        assert_eq!(
            meta.tools[0].next_tools.on_failure,
            vec!["dcc_diagnostics__screenshot", "dcc_diagnostics__audit_log"]
        );
    }

    #[test]
    fn test_tool_declaration_next_tools_alias() {
        // Test next_tools (underscore) alias
        let json = r#"{"name": "skill", "tools": [{
            "name": "my_tool",
            "next_tools": {
                "on_success": ["tool_a"],
                "on_failure": ["tool_b"]
            }
        }]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools[0].next_tools.on_success, vec!["tool_a"]);
        assert_eq!(meta.tools[0].next_tools.on_failure, vec!["tool_b"]);
    }

    // ── ExecutionMode (issue #317) ──────────────────────────────────────

    #[test]
    fn test_execution_mode_default_is_sync() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Sync);
        assert!(!ExecutionMode::default().is_deferred());
    }

    #[test]
    fn test_execution_mode_is_deferred() {
        assert!(!ExecutionMode::Sync.is_deferred());
        assert!(ExecutionMode::Async.is_deferred());
    }

    #[test]
    fn test_execution_mode_serde_round_trip() {
        let s = serde_json::to_string(&ExecutionMode::Sync).unwrap();
        assert_eq!(s, "\"sync\"");
        let a = serde_json::to_string(&ExecutionMode::Async).unwrap();
        assert_eq!(a, "\"async\"");
        let back: ExecutionMode = serde_json::from_str("\"async\"").unwrap();
        assert_eq!(back, ExecutionMode::Async);
    }

    #[test]
    fn test_tool_declaration_execution_async() {
        let json = r#"{"name": "s", "tools": [
            {"name": "render", "execution": "async", "timeout_hint_secs": 600}
        ]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools[0].execution, ExecutionMode::Async);
        assert_eq!(meta.tools[0].timeout_hint_secs, Some(600));
    }

    #[test]
    fn test_tool_declaration_execution_default_sync() {
        // Absence of `execution` → Sync, timeout_hint_secs → None.
        let json = r#"{"name": "s", "tools": [{"name": "quick"}]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tools[0].execution, ExecutionMode::Sync);
        assert_eq!(meta.tools[0].timeout_hint_secs, None);
    }

    #[test]
    fn test_tool_declaration_rejects_deferred_field() {
        let json = r#"{"name": "s", "tools": [{"name": "t", "deferred": true}]}"#;
        let err = serde_json::from_str::<SkillMetadata>(json).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("execution: async") || msg.contains("deferred"),
            "error must point to execution: async — got: {msg}",
        );
    }

    #[test]
    fn test_tool_declaration_rejects_unknown_execution() {
        let json = r#"{"name": "s", "tools": [{"name": "t", "execution": "background"}]}"#;
        let err = serde_json::from_str::<SkillMetadata>(json).unwrap_err();
        assert!(err.to_string().contains("background") || err.to_string().contains("execution"));
    }

    #[test]
    fn test_tool_declaration_next_tools_default_empty() {
        // Without next-tools, defaults are empty
        let json = r#"{"name": "skill", "tools": [{"name": "simple_tool"}]}"#;
        let meta: SkillMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.tools[0].next_tools.on_success.is_empty());
        assert!(meta.tools[0].next_tools.on_failure.is_empty());
    }
}
