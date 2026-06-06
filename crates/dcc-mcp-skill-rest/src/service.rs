//! Core service layer for the per-DCC REST skill API (#658).
//!
//! Every handler in [`super::router`] delegates here, so this file is
//! the single place that knows how to turn a REST request into a
//! validated dispatch against an [`ToolDispatcher`].
//!
//! Three traits satisfy the Dependency-Inversion rule:
//!
//! - [`SkillCatalogSource`] — anything that can *list* skills.
//! - [`ToolInvoker`] — anything that can *invoke* one tool by name,
//!   respecting execution metadata (main-thread vs subprocess). The
//!   default impl is backed by the existing [`ToolDispatcher`] but
//!   adapters may swap in a main-thread-marshalling version.
//! - [`ContextProvider`] — exposes DCC scene/document state. Defaults
//!   to [`crate::server::LiveMeta`]-style snapshots.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use dcc_mcp_actions::dispatcher::{DispatchError, ToolDispatcher, with_thread_affinity};
use dcc_mcp_actions::{
    DispatchExecutionContext, current_execution_context, with_execution_context,
};
use dcc_mcp_models::{
    CallExample, ExecutionMode, NextTools, SkillRuntimeSummary, ThreadAffinity, ToolAnnotations,
};
use dcc_mcp_skills::SkillCatalog;

use super::errors::{ServiceError, ServiceErrorKind};
use crate::search_index::{
    action_metadata, merged_search_aliases, normalise_search_values, schema_search_tokens,
    search_haystack, search_metadata,
};

// ── Requests / responses ─────────────────────────────────────────────

/// Payload for `POST /v1/search`. Every field is optional; an empty
/// request returns every action on the instance.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SearchRequest {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub dcc_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub scope: Option<String>,
    /// Only include actions belonging to loaded skills. Defaults to
    /// `true` — agents almost always want callable results only.
    #[serde(default = "default_true")]
    pub loaded_only: bool,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl Default for SearchRequest {
    /// `loaded_only = true` matches the serde default and the
    /// agent-friendly behaviour: don't surface skills that can't be
    /// invoked.
    fn default() -> Self {
        Self {
            query: None,
            dcc_type: None,
            tags: Vec::new(),
            scope: None,
            loaded_only: true,
            limit: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// A single search hit — deliberately compact.
///
/// Notice the **absence** of `input_schema`. That is the whole point:
/// a token-thrifty index keeps `/v1/search` answers tiny so agents can
/// enumerate hundreds of hits per turn without blowing the context
/// budget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct SkillListEntry {
    /// Stable tool slug: `<dcc>.<skill>.<action>`.
    pub slug: ToolSlug,
    /// Skill this action belongs to. Same as `slug.skill` but emitted
    /// separately so clients that group by skill don't need to parse
    /// the slug.
    pub skill: String,
    /// Action name inside the skill.
    pub action: String,
    /// Target DCC app (`"maya"`, `"blender"`, ...).
    pub dcc: String,
    /// One-line summary suitable for fuzzy matching.
    pub summary: String,
    /// `true` if the owning skill is currently loaded. Dispatching an
    /// unloaded slug returns `SkillNotLoaded`.
    pub loaded: bool,
    /// `true` when the action has a non-trivial `input_schema` (with
    /// `properties` or `required`). Agents should call `describe_tool`
    /// to fetch the full schema before invoking.
    #[serde(default)]
    pub has_schema: bool,
    /// Human-readable scope label (`"repo"`, `"user"`, ...).
    pub scope: String,
    /// MCP ToolAnnotations-style safety hints. Kept compact and omitted when
    /// the backend has no declared hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub annotations: Option<Value>,
    /// Execution metadata used by gateway `describe_tool` / REST clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub metadata: Option<Value>,
    /// Progressive tool groups declared by the owning skill, when known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_groups: Vec<SkillGroupState>,
    /// Machine-executable remediation for progressive loading. Present
    /// when `loaded=false` so REST clients can load the owning skill
    /// without needing MCP tool metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_step: Option<ProgressiveNextStep>,
}

/// One suggested follow-up operation for progressive loading.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ProgressiveNextStep {
    pub action: String,
    #[schema(value_type = Object)]
    pub arguments: Value,
}

/// Bounded group metadata for progressive skill activation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct SkillGroupState {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default)]
    pub default_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// Stable tool slug format used across REST and MCP.
///
/// Format: `<dcc>.<skill>.<action>`. Empty components are forbidden
/// so the serialised form is never ambiguous to split.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(transparent)]
pub struct ToolSlug(pub String);

impl ToolSlug {
    pub fn build(dcc: &str, skill: &str, action: &str) -> Self {
        Self(format!("{dcc}.{skill}.{action}"))
    }

    /// Parse into `(dcc, skill, action)`. Returns `None` when the
    /// slug does not contain exactly three non-empty components.
    #[must_use]
    pub fn parts(&self) -> Option<(&str, &str, &str)> {
        let parts: Vec<&str> = self.0.splitn(3, '.').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some((parts[0], parts[1], parts[2]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Response body of `/v1/search`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchResponse {
    pub total: usize,
    pub hits: Vec<SkillListEntry>,
}

/// Payload for `POST /v1/load_skill`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct LoadSkillRequest {
    pub skill_name: String,
}

/// Payload for `POST /v1/unload_skill`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UnloadSkillRequest {
    pub skill_name: String,
}

/// Response body for skill lifecycle operations.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SkillLifecycleResponse {
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<usize>,
}

/// Payload for `POST /v1/describe`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct DescribeRequest {
    pub tool_slug: ToolSlug,
    /// Opt in to the full `input_schema`. Defaults to `true` because
    /// an agent that calls `describe` usually wants to then call the
    /// tool — but clients that just need meta can set `false` to save
    /// tokens.
    #[serde(default = "default_true")]
    pub include_schema: bool,
}

/// Response body of `/v1/describe`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DescribeResponse {
    pub entry: SkillListEntry,
    /// Full description text, not the compact summary.
    pub description: String,
    /// Input schema — omitted when `include_schema = false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub input_schema: Option<Value>,
    /// Skill annotations (tags, category, execution metadata...).
    #[schema(value_type = Object)]
    pub annotations: Value,
    /// Execution metadata and risk hints carried outside the MCP
    /// ToolAnnotations namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub metadata: Option<Value>,
}

/// Payload for `POST /v1/call`.
///
/// The `meta` field carries request-level context forwarded by the Gateway
/// from the MCP `_meta` block.  It is injected as `params["_meta"]` before
/// the tool handler runs (after schema validation, so `additionalProperties:
/// false` tools are safe).
///
/// ## `meta` keys (bounded passthrough)
///
/// | Key | Source | Purpose |
/// |-----|--------|---------|
/// | `agent_context` | Server-derived | Caller identity (actor, agent, session) |
/// | `credential_profile` | Client | Environment tier (`"prod"`/`"staging"`/`"dev"`) |
/// | `permission_hint` | Client | `"read-only"` or `"read-write"` |
/// | `project_scope` | Client | Project identifier for data isolation |
/// | `search_id` | Client/Gateway | Telemetry correlation id |
///
/// See `docs/guide/agents-reference.md#request-level-context-passthrough-_meta----pip-520`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CallRequest {
    pub tool_slug: ToolSlug,
    /// Action arguments. Accepts both `params` and `arguments` field
    /// names for compatibility with the gateway REST layer (#818 phase 2)
    /// which sends `arguments` to match the MCP `tools/call` convention.
    #[serde(default, alias = "arguments")]
    #[schema(value_type = Object)]
    pub params: Value,
    /// Optional request-level metadata forwarded from the gateway/client.
    /// Contains allowlisted fields such as `agent_context`,
    /// `credential_profile`, `permission_hint`, `project_scope`.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub meta: Option<Value>,
}

/// Successful invocation outcome.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CallOutcome {
    pub slug: ToolSlug,
    #[schema(value_type = Object)]
    pub output: Value,
    pub validation_skipped: bool,
}

/// Snapshot returned by `/v1/context`.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ContextSnapshot {
    pub scene: Option<String>,
    pub version: Option<String>,
    pub dcc: Option<String>,
    pub display_name: Option<String>,
    pub documents: Vec<String>,
    /// Number of loaded skills.
    pub loaded_skill_count: usize,
    /// Number of registered actions.
    pub action_count: usize,
}

// ── Traits ────────────────────────────────────────────────────────────

/// Anything that can enumerate discovered skills.
pub trait SkillCatalogSource: Send + Sync {
    /// Return every registered action, flattened with its owning
    /// skill metadata when available.
    fn list_actions(&self) -> Vec<CatalogAction>;
    /// `true` if the named skill is currently loaded.
    fn is_loaded(&self, skill_name: &str) -> bool;
    /// Load one discovered skill and return the registered action names.
    fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::BadRequest,
            format!("skill loading is not supported by this catalog source: {skill_name}"),
        ))
    }
    /// Unload one loaded skill and return the number of removed actions.
    fn unload_skill(&self, skill_name: &str) -> Result<usize, ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::BadRequest,
            format!("skill unloading is not supported by this catalog source: {skill_name}"),
        ))
    }
}

/// One flattened (action, skill) pair. Everything the service layer
/// needs to build search hits, describe responses and route call
/// requests.
#[derive(Debug, Clone)]
pub struct CatalogAction {
    pub action_name: String,
    pub skill_name: String,
    pub dcc: String,
    pub description: String,
    pub tags: Vec<String>,
    pub search_aliases: Vec<String>,
    pub search_tokens: Vec<String>,
    pub input_schema: Value,
    pub loaded: bool,
    pub scope: String,
    pub annotations: ToolAnnotations,
    pub execution: ExecutionMode,
    pub timeout_hint_secs: Option<u32>,
    pub thread_affinity: ThreadAffinity,
    pub enforce_thread_affinity: bool,
    pub available_groups: Vec<SkillGroupState>,
    pub runtime: Option<SkillRuntimeSummary>,
    /// Suggested follow-up tools (`on-success` / `on-failure`). Surfaced at
    /// describe-time so agents can pre-plan recovery steps (issue #1408).
    pub next_tools: NextTools,
    /// Ready-to-copy call examples from `tools.yaml`. Surfaced in
    /// `metadata.dcc.call_examples` at describe-time (PIP-577).
    pub call_examples: Option<Vec<CallExample>>,
}

/// Anything that can invoke a tool by name and return its output.
///
/// The default [`DispatcherInvoker`] uses [`ToolDispatcher`]
/// synchronously. Embedders that marshal to a host main thread swap
/// in their own impl here (e.g. Maya's `DccExecutorHandle`).
pub trait ToolInvoker: Send + Sync {
    fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError>;
}

// ── Resource & prompt providers (#818 phase 1) ───────────────────────

/// One MCP resource entry as returned by `GET /v1/resources`.
///
/// Mirrors the spec `ResourceDefinition` shape so a gateway can pass
/// the payload straight through without re-mapping fields.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceListEntry {
    pub uri: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// One content blob as returned by `GET /v1/resources/{uri}`.
///
/// Either `text` or `blob` (base64) is set, not both.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Base64-encoded binary content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// `GET /v1/resources/{uri}` response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceReadResponse {
    pub contents: Vec<ResourceContent>,
}

/// One prompt definition as returned by `GET /v1/prompts`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PromptListEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgumentSpec>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub meta: Option<Value>,
}

/// One declared argument on a prompt.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PromptArgumentSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// One rendered message returned by `GET /v1/prompts/{name}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptContent,
}

/// Body of a single prompt message.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PromptContent {
    Text { text: String },
}

/// `GET /v1/prompts/{name}` response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PromptGetResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}

/// Anything that can list and read MCP-style resources.
///
/// Implementations live in the embedder (`dcc-mcp-http` wraps its
/// `ResourceRegistry` to satisfy this trait). Keeping the trait here
/// is a DIP boundary: the REST layer depends on the abstraction, not
/// on the concrete `dcc-mcp-http::resources::*` types.
pub trait ResourceProvider: Send + Sync {
    /// List every available resource.
    fn list(&self) -> Vec<ResourceListEntry>;
    /// Read one resource by URI.
    fn read(&self, uri: &str) -> Result<ResourceReadResponse, ServiceError>;
    /// Subscribe to resource update events on `uri`.
    ///
    /// The default implementation returns an immediately-terminating empty
    /// stream — embedders that do not implement push can leave this as-is.
    fn subscribe(&self, uri: &str) -> Result<ResourceEventStream, ServiceError> {
        let _ = uri;
        // Return an empty stream that ends immediately.
        let stream = futures::stream::empty();
        Ok(Box::pin(stream))
    }
}

/// Anything that can list MCP-style prompts and render one with
/// supplied arguments.
pub trait PromptProvider: Send + Sync {
    fn list(&self) -> Vec<PromptListEntry>;
    fn get(&self, name: &str, arguments: &Value) -> Result<PromptGetResponse, ServiceError>;
    fn diagnostics(&self) -> Option<Value> {
        None
    }
}

/// Default `ResourceProvider` returning an empty list — used when the
/// embedder has not wired anything in yet so the endpoint stays valid
/// with `200 OK` + `{ "resources": [] }` instead of 500-ing.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyResourceProvider;

impl ResourceProvider for EmptyResourceProvider {
    fn list(&self) -> Vec<ResourceListEntry> {
        Vec::new()
    }
    fn read(&self, uri: &str) -> Result<ResourceReadResponse, ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::NotFound,
            format!("resource not found: {uri}"),
        ))
    }
}

/// Default `PromptProvider` returning an empty list — symmetrical to
/// [`EmptyResourceProvider`].
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyPromptProvider;

impl PromptProvider for EmptyPromptProvider {
    fn list(&self) -> Vec<PromptListEntry> {
        Vec::new()
    }
    fn get(&self, name: &str, _arguments: &Value) -> Result<PromptGetResponse, ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::NotFound,
            format!("prompt not found: {name}"),
        ))
    }
    fn diagnostics(&self) -> Option<Value> {
        Some(serde_json::json!({
            "enabled": false,
            "prompt_count": 0,
            "notes": ["No prompt provider is configured for this REST service."]
        }))
    }
}

// ── Default impls ─────────────────────────────────────────────────────

/// Wraps [`SkillCatalog`] + [`ToolDispatcher`]. Thread-safe clone.
#[derive(Clone)]
pub struct CatalogSource {
    catalog: Arc<SkillCatalog>,
}

impl CatalogSource {
    pub fn new(catalog: Arc<SkillCatalog>) -> Self {
        Self { catalog }
    }
}

/// Build the dispatcher action name for a skill tool declaration.
///
/// Mirrors [`SkillCatalog::load_skill`] naming so `/v1/search` and
/// `/v1/describe` stay aligned with the name used after `load_skill`.
fn catalog_action_name(skill_name: &str, tool_decl: &dcc_mcp_models::ToolDeclaration) -> String {
    if tool_decl.name.contains("__") {
        return tool_decl.name.clone();
    }
    let skill_base = skill_name.replace('-', "_");
    format!("{}__{}", skill_base, tool_decl.name.replace('-', "_"))
}

impl SkillCatalogSource for CatalogSource {
    fn list_actions(&self) -> Vec<CatalogAction> {
        let mut out: Vec<CatalogAction> = Vec::new();
        let registry = self.catalog.registry();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Merge with the catalog summary so non-loaded skills show up
        // too (their actions simply won't dispatch until `load_skill`).
        let summaries = self.catalog.list_skills(None);
        let mut skill_info: std::collections::HashMap<
            String,
            (bool, String, String, Option<SkillRuntimeSummary>),
        > = std::collections::HashMap::new();
        for s in &summaries {
            skill_info.insert(
                s.name.clone(),
                (s.loaded, s.scope.clone(), s.dcc.clone(), s.runtime.clone()),
            );
        }
        let mut active_groups: std::collections::HashMap<(String, String), bool> =
            std::collections::HashMap::new();
        for (skill, group, active) in self.catalog.list_groups() {
            active_groups.insert((skill, group), active);
        }
        let mut groups_by_skill: std::collections::HashMap<String, Vec<SkillGroupState>> =
            std::collections::HashMap::new();
        for s in &summaries {
            let Some(detail) = self.catalog.get_skill_info(&s.name) else {
                continue;
            };
            let groups = detail
                .groups
                .iter()
                .filter(|group| !group.name.is_empty())
                .map(|group| SkillGroupState {
                    name: group.name.clone(),
                    description: group.description.clone(),
                    tools: group.tools.clone(),
                    default_active: group.default_active,
                    active: active_groups
                        .get(&(detail.name.clone(), group.name.clone()))
                        .copied(),
                })
                .collect::<Vec<_>>();
            if !groups.is_empty() {
                groups_by_skill.insert(s.name.clone(), groups);
            }
        }

        for meta in registry.list_actions(None) {
            let skill_name = meta
                .skill_name
                .clone()
                .unwrap_or_else(|| "core".to_string());
            let (loaded, scope, _dcc, runtime) = meta
                .skill_name
                .as_ref()
                .and_then(|name| skill_info.get(name).cloned())
                .unwrap_or_else(|| {
                    // Actions registered directly on the server are not owned by a
                    // loadable skill, but they are still callable through the
                    // dispatcher. Give them a stable slug segment and treat them as
                    // loaded so the REST surface works for plain Python
                    // `registry.register(...)` + `server.register_handler(...)` users.
                    (true, "core".to_string(), meta.dcc.clone(), None)
                });
            seen.insert(meta.name.clone());
            let search_tokens = schema_search_tokens(&meta.input_schema);
            let call_examples = self.catalog.get_skill_info(&skill_name).and_then(|detail| {
                detail
                    .tools
                    .iter()
                    .find(|t| t.name == meta.name)
                    .and_then(|t| t.call_examples.clone())
            });
            out.push(CatalogAction {
                action_name: meta.name,
                skill_name: skill_name.clone(),
                dcc: meta.dcc,
                description: meta.description,
                tags: meta.tags,
                search_aliases: normalise_search_values(meta.search_aliases, 24),
                search_tokens,
                input_schema: meta.input_schema,
                loaded,
                scope,
                annotations: meta.annotations,
                execution: meta.execution,
                timeout_hint_secs: meta.timeout_hint_secs,
                thread_affinity: meta.thread_affinity,
                enforce_thread_affinity: meta.enforce_thread_affinity,
                available_groups: groups_by_skill
                    .get(&skill_name)
                    .cloned()
                    .unwrap_or_default(),
                runtime,
                next_tools: meta.next_tools,
                call_examples,
            });
        }

        // Discovered-but-unloaded skills: expose every tools.yaml declaration
        // with its full input_schema so gateway `describe_tool` / POST
        // `/v1/describe` work before `load_skill` (issue #992 class).
        for summary in summaries {
            if self.catalog.is_loaded(&summary.name) {
                continue;
            }
            let Some(detail) = self.catalog.get_skill_info(&summary.name) else {
                continue;
            };
            for tool_decl in &detail.tools {
                let action_name = catalog_action_name(&detail.name, tool_decl);
                if !seen.insert(action_name.clone()) {
                    continue;
                }
                let description = if tool_decl.description.is_empty() {
                    format!("[{}] {}", detail.name, detail.description)
                } else {
                    tool_decl.description.clone()
                };
                let input_schema = if tool_decl.input_schema.is_null() {
                    // Try to generate schema from Python script signature so
                    // describe returns real inputSchema before load_skill
                    // (same logic as load_skill_metadata in catalog_loading.rs).
                    let skill_path = std::path::Path::new(&detail.skill_path);
                    let script_path = dcc_mcp_skills::catalog::resolve_tool_script(
                        tool_decl,
                        &detail.scripts,
                        skill_path,
                    );
                    if let Some(ref sp) = script_path {
                        dcc_mcp_skills::catalog::schema_gen::generate_input_schema(sp, None)
                            .unwrap_or_else(|| {
                                tracing::warn!(
                                    "Schema generation failed for '{}.{}', using fallback",
                                    detail.name,
                                    tool_decl.name
                                );
                                serde_json::json!({"type": "object"})
                            })
                    } else {
                        serde_json::json!({"type": "object"})
                    }
                } else {
                    tool_decl.input_schema.clone()
                };
                let search_aliases =
                    merged_search_aliases(&detail.search_aliases, &tool_decl.search_aliases);
                let search_tokens = schema_search_tokens(&input_schema);
                out.push(CatalogAction {
                    action_name,
                    skill_name: detail.name.clone(),
                    dcc: detail.dcc.clone(),
                    description,
                    tags: detail.tags.clone(),
                    search_aliases,
                    search_tokens,
                    input_schema,
                    loaded: false,
                    scope: detail.scope.clone(),
                    annotations: tool_decl.annotations.clone(),
                    execution: tool_decl.execution,
                    timeout_hint_secs: tool_decl.timeout_hint_secs,
                    thread_affinity: tool_decl.thread_affinity,
                    enforce_thread_affinity: tool_decl.enforce_thread_affinity,
                    available_groups: groups_by_skill
                        .get(&detail.name)
                        .cloned()
                        .unwrap_or_default(),
                    runtime: detail.runtime.clone(),
                    next_tools: tool_decl.next_tools.clone(),
                    call_examples: tool_decl.call_examples.clone(),
                });
            }
        }
        out
    }

    fn is_loaded(&self, skill_name: &str) -> bool {
        self.catalog.is_loaded(skill_name)
    }

    fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, ServiceError> {
        self.catalog.load_skill(skill_name).map_err(|message| {
            ServiceError::new(ServiceErrorKind::NotFound, message)
                .with_hint("call /v1/search with loaded_only=false to discover loadable skills")
        })
    }

    fn unload_skill(&self, skill_name: &str) -> Result<usize, ServiceError> {
        self.catalog.unload_skill(skill_name).map_err(|message| {
            ServiceError::new(ServiceErrorKind::NotFound, message)
                .with_hint("call /v1/skills to list currently loaded skills")
        })
    }
}

/// Dispatches through [`ToolDispatcher::dispatch`]. Synchronous —
/// the dispatcher itself is already non-blocking except for the
/// handler.
pub struct DispatcherInvoker {
    dispatcher: Arc<ToolDispatcher>,
    standalone_main_thread_execution: bool,
}

impl DispatcherInvoker {
    pub fn new(dispatcher: Arc<ToolDispatcher>) -> Self {
        Self {
            dispatcher,
            standalone_main_thread_execution: false,
        }
    }

    pub fn new_standalone_main_thread(dispatcher: Arc<ToolDispatcher>) -> Self {
        Self {
            dispatcher,
            standalone_main_thread_execution: true,
        }
    }
}

/// Map [`DispatchError`] to REST [`ServiceError`] — shared by [`DispatcherInvoker`]
/// and thread-routed HTTP invoke so `/v1/call` status codes stay aligned.
#[must_use]
pub fn dispatch_error_to_service_error(err: DispatchError) -> ServiceError {
    match err {
        DispatchError::HandlerNotFound(n) => ServiceError::new(
            ServiceErrorKind::UnknownSlug,
            format!("no handler registered for '{n}'"),
        ),
        DispatchError::ActionDisabled { action, group } => ServiceError::new(
            ServiceErrorKind::SkillNotLoaded,
            format!("action '{action}' is disabled (group '{group}')"),
        )
        .with_hint("call load_skill / activate the owning tool group first"),
        DispatchError::Vetoed {
            action,
            code,
            reason,
        } => ServiceError::new(
            ServiceErrorKind::PolicyDenied,
            format!("EVENT_VETOED: action '{action}' was vetoed ({code}): {reason}"),
        )
        .with_hint("inspect the registered EventBus before hook policy for this DCC instance")
        .with_context(serde_json::json!({
            "action": action,
            "veto_code": code,
            "veto_reason": reason,
        })),
        DispatchError::ThreadAffinityViolation {
            action,
            declared,
            actual,
        } => {
            let execution = current_execution_context();
            let hint = crate::thread_affinity_diagnostics::thread_affinity_hint(
                declared,
                actual,
                execution.and_then(|c| c.host_dispatcher_attached),
            );
            ServiceError::new(
                ServiceErrorKind::ThreadAffinityViolation,
                format!(
                    "THREAD_AFFINITY_VIOLATION: action '{action}' declared thread_affinity={declared} but ran on {actual}"
                ),
            )
            .with_hint(hint)
            .with_context(crate::thread_affinity_diagnostics::build_thread_affinity_context(
                &action, declared, actual, execution,
            ))
        }
        DispatchError::ValidationFailed(m) => ServiceError::new(ServiceErrorKind::InvalidParams, m),
        DispatchError::HandlerError(m) if m.starts_with("THREAD_AFFINITY_UNAVAILABLE:") => {
            let action = m
                .split('\'')
                .nth(1)
                .filter(|s| !s.is_empty())
                .unwrap_or("unknown");
            ServiceError::new(ServiceErrorKind::ThreadAffinityViolation, m.clone())
                .with_hint(crate::thread_affinity_diagnostics::affinity_unavailable_hint())
                .with_context(
                    crate::thread_affinity_diagnostics::build_affinity_unavailable_context(action),
                )
        }
        DispatchError::HandlerError(m) if is_host_busy_dispatch_error(&m) => {
            ServiceError::new(ServiceErrorKind::HostBusy, m).with_hint(
                "retry after a short backoff or route the call to another live DCC instance",
            )
        }
        DispatchError::HandlerError(m) if m == "CANCELLED" => {
            ServiceError::new(ServiceErrorKind::BackendError, m)
        }
        DispatchError::HandlerError(m) => ServiceError::new(ServiceErrorKind::BackendError, m),
        DispatchError::MetadataNotFound(m) => ServiceError::new(ServiceErrorKind::Internal, m),
    }
}

fn is_host_busy_dispatch_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("host-busy")
        || lower.contains("queue-overloaded")
        || lower.contains("queue overloaded")
}

impl ToolInvoker for DispatcherInvoker {
    fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError> {
        // Default REST path has no host main-thread bridge — publish that so
        // affinity diagnostics can surface host_dispatcher_attached=false (#1075).
        let exec_ctx = DispatchExecutionContext {
            host_dispatcher_attached: Some(false),
        };
        let standalone_main = self.standalone_main_thread_execution
            && self
                .dispatcher
                .registry()
                .get_action(action_name, None)
                .is_some_and(|meta| matches!(meta.thread_affinity, ThreadAffinity::Main));
        with_execution_context(exec_ctx, || {
            let dispatched = if standalone_main {
                with_thread_affinity(ThreadAffinity::Main, || {
                    self.dispatcher.dispatch(action_name, params, meta.clone())
                })
            } else {
                self.dispatcher.dispatch(action_name, params, meta.clone())
            };
            match dispatched {
                Ok(r) => Ok(CallOutcome {
                    slug: ToolSlug(r.action.clone()),
                    output: r.output,
                    validation_skipped: r.validation_skipped,
                }),
                Err(err) => Err(dispatch_error_to_service_error(err)),
            }
        })
    }
}

// ── The service ───────────────────────────────────────────────────────

/// Orchestrates `search` / `describe` / `call` on top of a
/// [`SkillCatalogSource`] and a [`ToolInvoker`]. Cheap to clone —
/// every field is an `Arc`.
#[derive(Clone)]
pub struct SkillRestService {
    catalog: Arc<dyn SkillCatalogSource>,
    invoker: Arc<dyn ToolInvoker>,
    resources: Arc<dyn ResourceProvider>,
    prompts: Arc<dyn PromptProvider>,
    jobs: Arc<dyn JobController>,
    context: Arc<RwLock<ContextSnapshot>>,
}

impl SkillRestService {
    /// Build a service from the default concrete impls.
    pub fn from_catalog_and_dispatcher(
        catalog: Arc<SkillCatalog>,
        dispatcher: Arc<ToolDispatcher>,
    ) -> Self {
        let catalog_source = Arc::new(CatalogSource::new(catalog));
        let invoker = Arc::new(DispatcherInvoker::new(dispatcher));
        Self::new(catalog_source, invoker)
    }

    /// Construct with the catalog + invoker only. Resources and prompts
    /// default to empty providers — wire real implementations in via
    /// [`Self::with_resources`] and [`Self::with_prompts`] when the
    /// embedder has them ready.
    pub fn new(catalog: Arc<dyn SkillCatalogSource>, invoker: Arc<dyn ToolInvoker>) -> Self {
        Self {
            catalog,
            invoker,
            resources: Arc::new(EmptyResourceProvider),
            prompts: Arc::new(EmptyPromptProvider),
            jobs: Arc::new(EmptyJobController),
            context: Arc::new(RwLock::new(ContextSnapshot::default())),
        }
    }

    /// Wire in a real [`ResourceProvider`]. Returns `Self` so the
    /// builder pattern composes.
    #[must_use]
    pub fn with_resources(mut self, resources: Arc<dyn ResourceProvider>) -> Self {
        self.resources = resources;
        self
    }

    /// Wire in a real [`PromptProvider`].
    #[must_use]
    pub fn with_prompts(mut self, prompts: Arc<dyn PromptProvider>) -> Self {
        self.prompts = prompts;
        self
    }

    /// Wire in a real [`JobController`] (#818 phase 1b).
    #[must_use]
    pub fn with_jobs(mut self, jobs: Arc<dyn JobController>) -> Self {
        self.jobs = jobs;
        self
    }

    /// Read-only access to the resource provider for handlers.
    pub fn resources(&self) -> &dyn ResourceProvider {
        self.resources.as_ref()
    }

    /// Read-only access to the prompt provider for handlers.
    pub fn prompts(&self) -> &dyn PromptProvider {
        self.prompts.as_ref()
    }

    /// Read-only access to the job controller for handlers.
    pub fn jobs(&self) -> &dyn JobController {
        self.jobs.as_ref()
    }

    /// Update the DCC context snapshot surfaced through `/v1/context`.
    pub fn update_context<F: FnOnce(&mut ContextSnapshot)>(&self, f: F) {
        f(&mut self.context.write());
    }

    /// Current context snapshot — never an error; missing fields are
    /// simply `None`.
    #[must_use]
    pub fn context_snapshot(&self) -> ContextSnapshot {
        let mut snap = self.context.read().clone();
        let actions = self.catalog.list_actions();
        snap.action_count = actions.len();
        snap.loaded_skill_count = actions
            .iter()
            .filter(|a| a.loaded)
            .map(|a| &a.skill_name)
            .collect::<std::collections::HashSet<_>>()
            .len();
        snap
    }

    /// Search + filter the action catalog.
    pub fn search(&self, req: &SearchRequest) -> SearchResponse {
        let actions = self.catalog.list_actions();
        let query = req
            .query
            .as_deref()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let tags_lower: Vec<String> = req.tags.iter().map(|t| t.to_ascii_lowercase()).collect();

        let mut hits: Vec<SkillListEntry> = actions
            .into_iter()
            .filter(|a| {
                if req.loaded_only && !a.loaded {
                    return false;
                }
                if let Some(d) = &req.dcc_type
                    && !d.is_empty()
                    && !a.dcc.eq_ignore_ascii_case(d)
                {
                    return false;
                }
                if let Some(scope_filter) = &req.scope
                    && !scope_filter.is_empty()
                    && !a.scope.eq_ignore_ascii_case(scope_filter)
                {
                    return false;
                }
                for t in &tags_lower {
                    if !a.tags.iter().any(|x| x.to_ascii_lowercase() == *t) {
                        return false;
                    }
                }
                if !query.is_empty() {
                    let hay = search_haystack(a);
                    if !hay.contains(&query) {
                        return false;
                    }
                }
                true
            })
            .map(action_to_entry)
            .collect();

        // Deterministic ordering: exact-name prefix matches first,
        // then alphabetic by slug. Tests rely on this.
        let q = query.clone();
        hits.sort_by(|a, b| {
            let a_prefix = !q.is_empty() && a.action.to_ascii_lowercase().starts_with(&q);
            let b_prefix = !q.is_empty() && b.action.to_ascii_lowercase().starts_with(&q);
            b_prefix
                .cmp(&a_prefix)
                .then_with(|| a.slug.0.cmp(&b.slug.0))
        });

        if let Some(lim) = req.limit {
            hits.truncate(lim);
        }

        SearchResponse {
            total: hits.len(),
            hits,
        }
    }

    /// Load a discovered skill through REST without requiring an MCP
    /// `tools/call` wrapper.
    pub fn load_skill(
        &self,
        req: &LoadSkillRequest,
    ) -> Result<SkillLifecycleResponse, ServiceError> {
        let skill_name = req.skill_name.trim();
        if skill_name.is_empty() {
            return Err(ServiceError::new(
                ServiceErrorKind::BadRequest,
                "skill_name must be a non-empty string",
            ));
        }
        let actions = self.catalog.load_skill(skill_name)?;
        Ok(SkillLifecycleResponse {
            skill_name: skill_name.to_string(),
            actions,
            removed: None,
        })
    }

    /// Unload a skill through REST without requiring an MCP wrapper.
    pub fn unload_skill(
        &self,
        req: &UnloadSkillRequest,
    ) -> Result<SkillLifecycleResponse, ServiceError> {
        let skill_name = req.skill_name.trim();
        if skill_name.is_empty() {
            return Err(ServiceError::new(
                ServiceErrorKind::BadRequest,
                "skill_name must be a non-empty string",
            ));
        }
        let removed = self.catalog.unload_skill(skill_name)?;
        Ok(SkillLifecycleResponse {
            skill_name: skill_name.to_string(),
            actions: Vec::new(),
            removed: Some(removed),
        })
    }

    /// Resolve a slug to the full action record, including schema.
    pub fn describe(&self, req: &DescribeRequest) -> Result<DescribeResponse, ServiceError> {
        let action = self.resolve_slug(&req.tool_slug)?;
        let entry = action_to_entry(action.clone());
        let annotations = describe_annotations(&action);
        let mut metadata = action_metadata(&action);
        // Surface next-tools hints at describe-time so agents can pre-plan
        // both the success path and failure recovery before calling the
        // tool (issue #1408). Mirrors the post-call `_meta["dcc.next_tools"]`
        // convention, but exposes both branches at once.
        if let Some(next_tools) = next_tools_meta_value(&action.next_tools)
            && let Some(obj) = metadata.as_object_mut()
        {
            obj.insert("dcc.next_tools".to_string(), next_tools);
        }
        let mut annotations = annotations.unwrap_or_else(|| serde_json::json!({}));
        annotations["tags"] = serde_json::json!(action.tags);
        annotations["scope"] = serde_json::json!(action.scope);
        annotations["loaded"] = serde_json::json!(action.loaded);
        annotations["dcc"] = serde_json::json!(action.dcc);
        let metadata =
            Some(metadata).filter(|value| !value.as_object().is_none_or(|m| m.is_empty()));
        Ok(DescribeResponse {
            entry,
            description: action.description,
            input_schema: if req.include_schema {
                Some(action.input_schema)
            } else {
                None
            },
            annotations,
            metadata,
        })
    }

    /// Invoke a tool by slug.
    pub fn call(&self, req: &CallRequest) -> Result<CallOutcome, ServiceError> {
        let action = self.resolve_slug(&req.tool_slug)?;
        if !action.loaded {
            return Err(ServiceError::new(
                ServiceErrorKind::SkillNotLoaded,
                format!(
                    "skill '{skill}' owning action '{action}' is not loaded",
                    skill = action.skill_name,
                    action = action.action_name,
                ),
            )
            .with_hint("call load_skill first"));
        }
        // Dispatcher registers under the action name, not the slug.
        let mut outcome =
            self.invoker
                .invoke(&action.action_name, req.params.clone(), req.meta.clone())?;
        // Normalise the outcome to report the slug the caller used.
        outcome.slug = req.tool_slug.clone();
        Ok(outcome)
    }

    /// Invoke a backend action by bare `backend_tool` name with a DCC-bucket guard.
    ///
    /// Intended for `POST /v1/dcc/{dcc_type}/call` on a single-tenant HTTP server so
    /// non-MCP clients can skip composing the dotted `tool_slug` token.
    pub fn call_backend_tool_for_dcc(
        &self,
        dcc_type: &str,
        backend_tool: &str,
        params: Value,
    ) -> Result<CallOutcome, ServiceError> {
        let slug = ToolSlug(backend_tool.to_string());
        let action = self.resolve_slug(&slug)?;
        if !action.dcc.eq_ignore_ascii_case(dcc_type) {
            return Err(ServiceError::new(
                ServiceErrorKind::BadRequest,
                format!(
                    "backend tool '{}' is registered under dcc '{}', not '{}'",
                    backend_tool, action.dcc, dcc_type
                ),
            ));
        }
        if !action.loaded {
            return Err(ServiceError::new(
                ServiceErrorKind::SkillNotLoaded,
                format!(
                    "skill '{}' owning action '{}' is not loaded",
                    action.skill_name, action.action_name,
                ),
            )
            .with_hint("call load_skill first"));
        }
        let mut outcome = self.invoker.invoke(&action.action_name, params, None)?;
        outcome.slug = ToolSlug::build(&action.dcc, &action.skill_name, &action.action_name);
        Ok(outcome)
    }

    /// Flat list view, always sorted deterministically.
    pub fn list_skills(&self, loaded_only: bool) -> Vec<SkillListEntry> {
        let mut entries: Vec<SkillListEntry> = self
            .catalog
            .list_actions()
            .into_iter()
            .filter(|a| !loaded_only || a.loaded)
            .map(action_to_entry)
            .collect();
        entries.sort_by(|a, b| a.slug.0.cmp(&b.slug.0));
        entries
    }

    fn resolve_slug(&self, slug: &ToolSlug) -> Result<CatalogAction, ServiceError> {
        // Fast path: full `<dcc>.<skill>.<action>` slug.
        if let Some((dcc, skill, action)) = slug.parts() {
            let actions = self.catalog.list_actions();
            let exact: Vec<CatalogAction> = actions
                .iter()
                .filter(|a| {
                    a.dcc.eq_ignore_ascii_case(dcc)
                        && a.skill_name.eq_ignore_ascii_case(skill)
                        && a.action_name.eq_ignore_ascii_case(action)
                })
                .cloned()
                .collect();
            return match exact.len() {
                1 => Ok(exact.into_iter().next().unwrap()),
                0 => Err(ServiceError::new(
                    ServiceErrorKind::UnknownSlug,
                    format!("no action registered for slug '{}'", slug.0),
                )
                .with_hint("call /v1/search to list available tools")),
                _ => Err(ServiceError::new(
                    ServiceErrorKind::Ambiguous,
                    format!("slug '{}' matches {} actions", slug.0, exact.len()),
                )
                .with_candidates(
                    exact
                        .iter()
                        .map(|a| ToolSlug::build(&a.dcc, &a.skill_name, &a.action_name).0)
                        .collect(),
                )),
            };
        }

        // Bare action name fallback (#818 phase 2): the gateway forwards
        // `callable_id` (bare action name) from the capability record.
        // Accept it so directly-registered actions (skill_name="core") remain
        // callable without requiring the full slug format.
        let action_name = slug.0.as_str();
        let actions = self.catalog.list_actions();
        let matching: Vec<CatalogAction> = actions
            .iter()
            .filter(|a| a.action_name.eq_ignore_ascii_case(action_name))
            .cloned()
            .collect();
        match matching.len() {
            1 => Ok(matching.into_iter().next().unwrap()),
            0 => Err(ServiceError::new(
                ServiceErrorKind::BadRequest,
                format!(
                    "invalid tool slug '{}' — expected '<dcc>.<skill>.<action>' or bare action name",
                    slug.0
                ),
            )),
            _ => Err(ServiceError::new(
                ServiceErrorKind::Ambiguous,
                format!(
                    "bare action name '{}' is ambiguous across {} registered actions; \
                     use the full '<dcc>.<skill>.<action>' slug",
                    slug.0,
                    matching.len()
                ),
            )
            .with_candidates(
                matching
                    .iter()
                    .map(|a| ToolSlug::build(&a.dcc, &a.skill_name, &a.action_name).0)
                    .collect(),
            )),
        }
    }
}

fn action_to_entry(a: CatalogAction) -> SkillListEntry {
    let summary = truncate(&a.description, 180);
    let next_step = if a.loaded {
        None
    } else {
        Some(ProgressiveNextStep {
            action: "load_skill".to_string(),
            arguments: serde_json::json!({
                "skill_name": a.skill_name.clone(),
                "dcc": a.dcc.clone(),
            }),
        })
    };
    let has_schema = a
        .input_schema
        .as_object()
        .map(|obj| {
            let props_ok = obj
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|p| !p.is_empty());
            let required_ok = obj
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|r| !r.is_empty());
            props_ok || required_ok
        })
        .unwrap_or(false);
    let annotations = safety_annotations(&a.annotations);
    let metadata = search_metadata(&a);
    SkillListEntry {
        slug: ToolSlug::build(&a.dcc, &a.skill_name, &a.action_name),
        skill: a.skill_name,
        action: a.action_name,
        dcc: a.dcc,
        summary,
        loaded: a.loaded,
        has_schema,
        scope: a.scope,
        annotations,
        metadata,
        available_groups: a.available_groups,
        next_step,
    }
}

fn describe_annotations(action: &CatalogAction) -> Option<Value> {
    safety_annotations(&action.annotations)
}

/// Build the `dcc.next_tools` describe-time hint value (issue #1408).
///
/// Unlike the post-call `_meta` slot — which carries only the branch that
/// matched the outcome — describe exposes **both** `on_success` and
/// `on_failure` so agents can plan recovery before invoking the tool.
/// Returns `None` when no follow-ups are declared so the key stays absent.
fn next_tools_meta_value(next_tools: &NextTools) -> Option<Value> {
    if next_tools.on_success.is_empty() && next_tools.on_failure.is_empty() {
        return None;
    }
    let mut map = serde_json::Map::new();
    if !next_tools.on_success.is_empty() {
        map.insert(
            "on_success".to_string(),
            serde_json::json!(next_tools.on_success),
        );
    }
    if !next_tools.on_failure.is_empty() {
        map.insert(
            "on_failure".to_string(),
            serde_json::json!(next_tools.on_failure),
        );
    }
    Some(Value::Object(map))
}

fn safety_annotations(annotations: &ToolAnnotations) -> Option<Value> {
    let mut out = serde_json::Map::new();
    if let Some(title) = &annotations.title {
        out.insert("title".to_string(), Value::String(title.clone()));
    }
    if let Some(read_only) = annotations.read_only_hint {
        out.insert("readOnlyHint".to_string(), Value::Bool(read_only));
    }
    if let Some(destructive) = annotations.destructive_hint {
        out.insert("destructiveHint".to_string(), Value::Bool(destructive));
    }
    if let Some(idempotent) = annotations.idempotent_hint {
        out.insert("idempotentHint".to_string(), Value::Bool(idempotent));
    }
    if let Some(open_world) = annotations.open_world_hint {
        out.insert("openWorldHint".to_string(), Value::Bool(open_world));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_owned();
    }
    s.chars().take(n).collect::<String>() + "…"
}

// ── Job lifecycle & SSE streaming (#818 phase 1b) ─────────────────────
//
// These types + traits let the gateway phase 2 switch from MCP
// `notifications/progress` to `GET /v1/jobs/{id}/events` SSE.
//
// Design:
//   JobController (trait) — opaque handle to the embedder's job store.
//   JobEvent (enum)       — the typed event variants written to the SSE stream.
//   EventStream           — the concrete stream type handed to axum's Sse.
//
// DIP: service.rs owns the trait and the enum; dcc-mcp-http wires its
// concrete dispatcher-backed implementation in phase 2. EmptyJobController
// is the default, returning NotFound for every operation.

use std::pin::Pin;

use futures::Stream;

/// One event emitted on the `GET /v1/jobs/{id}/events` SSE stream.
///
/// Serialised as `{ "type": "...", ... }` (kebab-case discriminant).
/// Clients should ignore unknown types for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum JobEvent {
    /// Incremental progress update (before the result is available).
    Progress {
        /// Completion ratio `[0.0, 1.0]`. `None` when the total is
        /// unknown.
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<f64>,
        /// Current step out of `total` steps, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
        /// Human-readable status message.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Partial output available before the tool has finished.
    Partial {
        /// Any JSON value — tool-specific payload.
        content: Value,
    },
    /// Tool finished successfully.
    Done { result: CallOutcome },
    /// Tool finished with an error.
    Error { error: ServiceError },
}

/// Pinned, Send + Sync stream of `JobEvent`s for axum's `Sse::new`.
///
/// `Infallible` error type follows the axum SSE convention: the
/// stream itself never errors — any failure is modelled as a
/// `JobEvent::Error` value.
pub type EventStream =
    Pin<Box<dyn Stream<Item = Result<JobEvent, std::convert::Infallible>> + Send + Sync + 'static>>;

/// Anything that can track and surface running job events.
///
/// Default: [`EmptyJobController`].
pub trait JobController: Send + Sync {
    /// Subscribe to events for `job_id`. Returns a stream that yields
    /// events until the job is done (terminal `Done` or `Error`) or the
    /// subscription is dropped.
    ///
    /// Returns `Err(NotFound)` when the job id is not known.
    fn subscribe(&self, job_id: &str) -> Result<EventStream, ServiceError>;

    /// Cancel a running job. Returns `Ok(())` if the signal was sent,
    /// `Err(NotFound)` if the job does not exist.
    fn cancel(&self, job_id: &str) -> Result<(), ServiceError>;
}

/// Always returns `NotFound`. Suitable for embedders that do not yet
/// expose async jobs through the REST surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyJobController;

impl JobController for EmptyJobController {
    fn subscribe(&self, job_id: &str) -> Result<EventStream, ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::NotFound,
            format!("job not found: {job_id}"),
        ))
    }
    fn cancel(&self, job_id: &str) -> Result<(), ServiceError> {
        Err(ServiceError::new(
            ServiceErrorKind::NotFound,
            format!("job not found: {job_id}"),
        ))
    }
}

// ── Resource event stream (#818 phase 1b) ────────────────────────────

/// One event emitted on the `GET /v1/resources/{uri}/events` SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ResourceEvent {
    /// The resource content has been updated. Clients should re-read.
    Updated { uri: String },
    /// The resource has been removed from the server.
    Removed { uri: String },
}

/// Pinned stream for resource events.
pub type ResourceEventStream = Pin<
    Box<dyn Stream<Item = Result<ResourceEvent, std::convert::Infallible>> + Send + Sync + 'static>,
>;

// ── SkillRestService wires job controller ─────────────────────────────

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
