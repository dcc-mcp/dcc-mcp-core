use serde::{Deserialize, Serialize};

// ── FeatureFlags ───────────────────────────────────────────────────────────

/// Opt-in capability switches.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Each field is a single boolean knob; the defaults
/// are split — some default `true` because they are the documented
/// shape today (`bare_tool_names`, `enable_resources`, …) — so this
/// struct intentionally provides a hand-written `Default` impl
/// rather than `#[derive(Default)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable the opt-in lazy-actions meta-tools: ``list_actions``,
    /// ``describe_action`` and ``call_action``.
    ///
    /// When `true`, `tools/list` additionally surfaces these three
    /// meta-tools so agents with tight context budgets can drive an
    /// arbitrarily large action catalog through a single page of 3
    /// stubs instead of paging through every loaded skill's tools.
    /// Default: `false`.
    #[serde(default)]
    pub lazy_actions: bool,

    /// Publish skill-scoped tools under their **bare action name**
    /// when no collision exists on this instance (#307).
    ///
    /// When `true` (default), `tools/list` emits `execute_python`
    /// rather than `maya_scripting__execute_python` whenever the bare
    /// name is unique within the instance's loaded skills.
    /// Collisions fall back to the client-safe full
    /// `<skill>__<action>` form.
    #[serde(default = "default_true")]
    pub bare_tool_names: bool,

    /// Advertise the MCP Resources primitive (issue #350).
    #[serde(default = "default_true")]
    pub enable_resources: bool,

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    #[serde(default = "default_true")]
    pub enable_prompts: bool,

    /// Expose `artefact://` resources (issue #349).
    #[serde(default)]
    pub enable_artefact_resources: bool,

    /// Emit the `notifications/$/dcc.jobUpdated` and
    /// `notifications/$/dcc.workflowUpdated` SSE channels (issue #326).
    #[serde(default = "default_true")]
    pub enable_job_notifications: bool,

    /// Best-effort safety net for Python callers that drop a
    /// `McpServerHandle` without calling `shutdown()`.
    #[serde(default)]
    pub shutdown_on_drop: bool,

    /// Omit unloaded-skill ``__skill__*`` stubs from ``tools/list`` (#174).
    ///
    /// Discovery remains available via ``search_skills``, ``search_tools``
    /// (with ``include_unloaded_skills``), ``list_skills``, capability
    /// manifests, and gateway ``/v1/search``. Set via
    /// ``DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST`` or per-DCC
    /// ``DCC_MCP_<DCC>_EXCLUDE_STUBS_FROM_TOOLS_LIST`` (Python helper:
    /// :func:`dcc_mcp_core.resolve_tools_list_stub_policy`).
    #[serde(default)]
    pub exclude_skill_stubs_from_tools_list: bool,

    /// Omit inactive-group ``__group__*`` stubs from ``tools/list``.
    ///
    /// Loaded skills still expose enabled tools; only collapsed group stubs
    /// are hidden. Pair with ``exclude_skill_stubs_from_tools_list`` for the
    /// full token-budget win documented on Maya issue #174 / #238.
    #[serde(default)]
    pub exclude_group_stubs_from_tools_list: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            lazy_actions: false,
            bare_tool_names: true,
            enable_resources: true,
            enable_prompts: true,
            enable_artefact_resources: false,
            enable_job_notifications: true,
            shutdown_on_drop: false,
            exclude_skill_stubs_from_tools_list: false,
            exclude_group_stubs_from_tools_list: false,
        }
    }
}

/// Helper for `#[serde(default = ...)]` on the boolean fields whose
/// pre-#852 default was `true`. The function form is required because
/// serde's attribute parser does not accept inline literals here.
fn default_true() -> bool {
    true
}
