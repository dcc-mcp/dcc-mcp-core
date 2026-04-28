fn is_default_affinity(affinity: &ThreadAffinity) -> bool {
    matches!(affinity, ThreadAffinity::Any)
}

use serde::{Deserialize, Serialize};

use super::{ExecutionMode, ThreadAffinity};

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

// PyO3 bindings for these types live in `crate::python::tool_declaration`.

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
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "ToolDeclaration", eq, from_py_object)
)]
// `#[derive(PyWrapper)]` (#528 M3.4) emits the trivial String / bool /
// Option<u32> / Vec<String> getters and setters as a sibling
// `#[pymethods]` block. Custom logic (`input_schema` / `output_schema`
// JSON round-trip, `execution` enum<->str, `annotations` and
// `next_tools` dict marshalling, the curated `__repr__`) stays
// hand-written in `crate::python::tool_declaration`.
#[cfg_attr(
    feature = "python-bindings",
    derive(dcc_mcp_pybridge::derive::PyWrapper)
)]
#[cfg_attr(
    feature = "python-bindings",
    py_wrapper(fields(
        name: String => [get(by_str), set],
        description: String => [get(by_str), set],
        read_only: bool => [get, set],
        destructive: bool => [get, set],
        idempotent: bool => [get, set],
        defer_loading: bool => [get, set],
        source_file: String => [get(by_str), set],
        group: String => [get(by_str), set],
        timeout_hint_secs: Option<u32> => [get, set],
        required_capabilities: Vec<String> => [get(clone), set],
    ))
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
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillGroup", eq, from_py_object)
)]
// All four fields are read-only on the Python side (constructed via
// `SkillGroup(...)` and never mutated in place) so the macro emits
// getters only. `__repr__` stays hand-written because it picks `tools.len()`
// rather than the full vector.
#[cfg_attr(
    feature = "python-bindings",
    derive(dcc_mcp_pybridge::derive::PyWrapper)
)]
#[cfg_attr(
    feature = "python-bindings",
    py_wrapper(fields(
        name: String => [get(by_str)],
        description: String => [get(by_str)],
        tools: Vec<String> => [get(clone)],
        default_active: bool => [get],
    ))
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

fn is_null_value(v: &serde_json::Value) -> bool {
    v.is_null()
}

impl std::fmt::Display for ToolDeclaration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolDeclaration({})", self.name)
    }
}
