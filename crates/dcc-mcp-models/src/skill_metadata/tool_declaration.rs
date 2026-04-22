fn is_default_affinity(affinity: &ThreadAffinity) -> bool {
    matches!(affinity, ThreadAffinity::Any)
}

use serde::{Deserialize, Serialize};

use super::{ExecutionMode, ThreadAffinity};

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use pyo3::types::{PyAnyMethods, PyDictMethods};

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

    /// DCC capabilities required by this tool (issue #354).
    ///
    /// Freeform string tags (e.g. `"usd"`, `"scene.mutate"`,
    /// `"filesystem.read"`). At server startup each DCC adapter advertises
    /// the capabilities it actually provides via
    /// `McpHttpConfig::declared_capabilities`; any tool whose requirements
    /// are not fully covered is still surfaced in `tools/list` but decorated
    /// with `_meta.dcc.missing_capabilities` and fails `tools/call` with a
    /// `-32001 capability_missing` JSON-RPC error.
    ///
    /// Declared per-tool in the sibling `tools.yaml` (no new top-level
    /// SKILL.md frontmatter keys, per issue #356):
    ///
    /// ```yaml
    /// tools:
    ///   - name: import_usd
    ///     required_capabilities: [usd, scene.mutate, filesystem.read]
    /// ```
    #[serde(
        default,
        rename = "required_capabilities",
        alias = "required-capabilities",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub required_capabilities: Vec<String>,
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

            #[serde(
                default,
                rename = "required_capabilities",
                alias = "required-capabilities"
            )]
            required_capabilities: Vec<String>,
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
            required_capabilities: w.required_capabilities,
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
            required_capabilities: Vec::new(),
        })
    }

    /// Declared DCC capabilities required for this tool (issue #354).
    ///
    /// Returns freeform string tags; an empty list means the tool has no
    /// capability prerequisites beyond what any DCC adapter provides.
    #[getter]
    fn required_capabilities(&self) -> Vec<String> {
        self.required_capabilities.clone()
    }

    #[setter]
    fn set_required_capabilities(&mut self, value: Vec<String>) {
        self.required_capabilities = value;
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
        let dict = v.cast::<PyDict>().map_err(|_| {
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
