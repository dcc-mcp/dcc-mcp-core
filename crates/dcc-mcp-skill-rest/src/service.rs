//! Core service layer for the per-DCC REST skill API (#658).
//!
//! Every handler in [`super::router`] delegates here, so this file is
//! the single place that knows how to turn a REST request into a
//! validated dispatch against an [`ActionDispatcher`].
//!
//! Three traits satisfy the Dependency-Inversion rule:
//!
//! - [`SkillCatalogSource`] — anything that can *list* skills.
//! - [`ToolInvoker`] — anything that can *invoke* one tool by name,
//!   respecting execution metadata (main-thread vs subprocess). The
//!   default impl is backed by the existing [`ActionDispatcher`] but
//!   adapters may swap in a main-thread-marshalling version.
//! - [`ContextProvider`] — exposes DCC scene/document state. Defaults
//!   to [`crate::server::LiveMeta`]-style snapshots.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use dcc_mcp_actions::dispatcher::{ActionDispatcher, DispatchError};
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_skills::SkillCatalog;

use super::errors::{ServiceError, ServiceErrorKind};

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
    /// Human-readable scope label (`"repo"`, `"user"`, ...).
    pub scope: String,
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
}

/// Payload for `POST /v1/call`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CallRequest {
    pub tool_slug: ToolSlug,
    /// Action arguments. Accepts both `params` and `arguments` field
    /// names for compatibility with the gateway REST layer (#818 phase 2)
    /// which sends `arguments` to match the MCP `tools/call` convention.
    #[serde(default, alias = "arguments")]
    #[schema(value_type = Object)]
    pub params: Value,
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
    pub input_schema: Value,
    pub loaded: bool,
    pub scope: String,
}

/// Anything that can invoke a tool by name and return its output.
///
/// The default [`DispatcherInvoker`] uses [`ActionDispatcher`]
/// synchronously. Embedders that marshal to a host main thread swap
/// in their own impl here (e.g. Maya's `DccExecutorHandle`).
pub trait ToolInvoker: Send + Sync {
    fn invoke(&self, action_name: &str, params: Value) -> Result<CallOutcome, ServiceError>;
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
}

// ── Default impls ─────────────────────────────────────────────────────

/// Wraps [`SkillCatalog`] + [`ActionDispatcher`]. Thread-safe clone.
#[derive(Clone)]
pub struct CatalogSource {
    catalog: Arc<SkillCatalog>,
}

impl CatalogSource {
    pub fn new(catalog: Arc<SkillCatalog>) -> Self {
        Self { catalog }
    }
}

impl SkillCatalogSource for CatalogSource {
    fn list_actions(&self) -> Vec<CatalogAction> {
        let mut out: Vec<CatalogAction> = Vec::new();
        let registry = self.catalog.registry();

        // Index loaded-state by skill name so we don't re-query per
        // action.
        let mut loaded_skills: std::collections::HashMap<String, (bool, String)> =
            std::collections::HashMap::new();
        self.catalog
            .for_each_loaded_metadata(|meta: &SkillMetadata| {
                loaded_skills.insert(meta.name.clone(), (true, String::new()));
            });

        // Merge with the catalog summary so non-loaded skills show up
        // too (their actions simply won't dispatch).
        let summaries = self.catalog.list_skills(None);
        let mut skill_info: std::collections::HashMap<String, (bool, String, String)> =
            std::collections::HashMap::new();
        for s in summaries {
            skill_info.insert(s.name.clone(), (s.loaded, s.scope.clone(), s.dcc.clone()));
        }

        for meta in registry.list_actions(None) {
            let skill_name = meta
                .skill_name
                .clone()
                .unwrap_or_else(|| "core".to_string());
            let (loaded, scope, _dcc) = meta
                .skill_name
                .as_ref()
                .and_then(|name| skill_info.get(name).cloned())
                .unwrap_or_else(|| {
                    // Actions registered directly on the server are not owned by a
                    // loadable skill, but they are still callable through the
                    // dispatcher. Give them a stable slug segment and treat them as
                    // loaded so the REST surface works for plain Python
                    // `registry.register(...)` + `server.register_handler(...)` users.
                    (true, "core".to_string(), meta.dcc.clone())
                });
            out.push(CatalogAction {
                action_name: meta.name,
                skill_name,
                dcc: meta.dcc,
                description: meta.description,
                tags: meta.tags,
                input_schema: meta.input_schema,
                loaded,
                scope,
            });
        }
        out
    }

    fn is_loaded(&self, skill_name: &str) -> bool {
        self.catalog.is_loaded(skill_name)
    }
}

/// Dispatches through [`ActionDispatcher::dispatch`]. Synchronous —
/// the dispatcher itself is already non-blocking except for the
/// handler.
pub struct DispatcherInvoker {
    dispatcher: Arc<ActionDispatcher>,
}

impl DispatcherInvoker {
    pub fn new(dispatcher: Arc<ActionDispatcher>) -> Self {
        Self { dispatcher }
    }
}

impl ToolInvoker for DispatcherInvoker {
    fn invoke(&self, action_name: &str, params: Value) -> Result<CallOutcome, ServiceError> {
        match self.dispatcher.dispatch(action_name, params) {
            Ok(r) => Ok(CallOutcome {
                slug: ToolSlug(r.action.clone()),
                output: r.output,
                validation_skipped: r.validation_skipped,
            }),
            Err(DispatchError::HandlerNotFound(n)) => Err(ServiceError::new(
                ServiceErrorKind::UnknownSlug,
                format!("no handler registered for '{n}'"),
            )),
            Err(DispatchError::ActionDisabled { action, group }) => Err(ServiceError::new(
                ServiceErrorKind::SkillNotLoaded,
                format!("action '{action}' is disabled (group '{group}')"),
            )
            .with_hint("call load_skill / activate the owning tool group first")),
            Err(DispatchError::ValidationFailed(m)) => {
                Err(ServiceError::new(ServiceErrorKind::InvalidParams, m))
            }
            Err(DispatchError::HandlerError(m)) => {
                Err(ServiceError::new(ServiceErrorKind::BackendError, m))
            }
            Err(DispatchError::MetadataNotFound(m)) => {
                Err(ServiceError::new(ServiceErrorKind::Internal, m))
            }
        }
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
        dispatcher: Arc<ActionDispatcher>,
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
                    let hay = format!(
                        "{} {} {} {}",
                        a.action_name.to_ascii_lowercase(),
                        a.skill_name.to_ascii_lowercase(),
                        a.description.to_ascii_lowercase(),
                        a.tags.join(" ").to_ascii_lowercase(),
                    );
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

    /// Resolve a slug to the full action record, including schema.
    pub fn describe(&self, req: &DescribeRequest) -> Result<DescribeResponse, ServiceError> {
        let action = self.resolve_slug(&req.tool_slug)?;
        let entry = action_to_entry(action.clone());
        let annotations = serde_json::json!({
            "tags": action.tags,
            "scope": action.scope,
            "loaded": action.loaded,
            "dcc": action.dcc,
        });
        Ok(DescribeResponse {
            entry,
            description: action.description,
            input_schema: if req.include_schema {
                Some(action.input_schema)
            } else {
                None
            },
            annotations,
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
        let mut outcome = self
            .invoker
            .invoke(&action.action_name, req.params.clone())?;
        // Normalise the outcome to report the slug the caller used.
        outcome.slug = req.tool_slug.clone();
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
    SkillListEntry {
        slug: ToolSlug::build(&a.dcc, &a.skill_name, &a.action_name),
        skill: a.skill_name,
        action: a.action_name,
        dcc: a.dcc,
        summary,
        loaded: a.loaded,
        scope: a.scope,
    }
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
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory test fake. Lets us drive the service without spinning
    /// up a real SkillCatalog/ActionDispatcher — keeps unit tests
    /// dependency-free.
    #[derive(Default)]
    struct FakeCatalog {
        actions: Mutex<Vec<CatalogAction>>,
    }

    impl FakeCatalog {
        fn push(&self, a: CatalogAction) {
            self.actions.lock().unwrap().push(a);
        }
    }

    impl SkillCatalogSource for FakeCatalog {
        fn list_actions(&self) -> Vec<CatalogAction> {
            self.actions.lock().unwrap().clone()
        }
        fn is_loaded(&self, name: &str) -> bool {
            self.actions
                .lock()
                .unwrap()
                .iter()
                .any(|a| a.skill_name == name && a.loaded)
        }
    }

    #[derive(Default)]
    struct FakeInvoker {
        calls: Mutex<Vec<(String, Value)>>,
        next: Mutex<Option<Result<Value, ServiceError>>>,
    }

    impl FakeInvoker {
        fn set_next(&self, r: Result<Value, ServiceError>) {
            *self.next.lock().unwrap() = Some(r);
        }
    }

    impl ToolInvoker for FakeInvoker {
        fn invoke(&self, name: &str, params: Value) -> Result<CallOutcome, ServiceError> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_owned(), params.clone()));
            let r = self.next.lock().unwrap().take().unwrap_or(Ok(Value::Null));
            r.map(|v| CallOutcome {
                slug: ToolSlug(name.to_owned()),
                output: v,
                validation_skipped: false,
            })
        }
    }

    fn sphere_action(loaded: bool) -> CatalogAction {
        CatalogAction {
            action_name: "create_sphere".into(),
            skill_name: "spheres".into(),
            dcc: "maya".into(),
            description: "Create a polygon sphere".into(),
            tags: vec!["geometry".into(), "poly".into()],
            input_schema: serde_json::json!({"type":"object"}),
            loaded,
            scope: "repo".into(),
        }
    }

    fn build_service(actions: Vec<CatalogAction>) -> (SkillRestService, Arc<FakeInvoker>) {
        let cat = Arc::new(FakeCatalog::default());
        for a in actions {
            cat.push(a);
        }
        let inv = Arc::new(FakeInvoker::default());
        let svc = SkillRestService::new(cat, inv.clone());
        (svc, inv)
    }

    #[test]
    fn slug_round_trip() {
        let s = ToolSlug::build("maya", "spheres", "create_sphere");
        let (d, sk, a) = s.parts().unwrap();
        assert_eq!((d, sk, a), ("maya", "spheres", "create_sphere"));
    }

    #[test]
    fn slug_rejects_empty_parts() {
        assert!(ToolSlug("maya..create".into()).parts().is_none());
        assert!(ToolSlug("maya.spheres".into()).parts().is_none());
        assert!(ToolSlug(".spheres.create".into()).parts().is_none());
    }

    #[test]
    fn search_returns_loaded_only_by_default() {
        let (svc, _) = build_service(vec![
            sphere_action(true),
            CatalogAction {
                action_name: "create_cube".into(),
                skill_name: "cubes".into(),
                loaded: false,
                ..sphere_action(true)
            },
        ]);
        let resp = svc.search(&SearchRequest::default());
        assert_eq!(resp.total, 1);
        assert_eq!(resp.hits[0].action, "create_sphere");
    }

    #[test]
    fn search_query_matches_description() {
        let (svc, _) = build_service(vec![sphere_action(true)]);
        let req = SearchRequest {
            query: Some("polygon".into()),
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 1);
        let req = SearchRequest {
            query: Some("quaternion".into()),
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 0);
    }

    #[test]
    fn search_dcc_filter_is_case_insensitive() {
        let (svc, _) = build_service(vec![sphere_action(true)]);
        let req = SearchRequest {
            dcc_type: Some("MAYA".into()),
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 1);
    }

    #[test]
    fn search_tags_are_anded() {
        let (svc, _) = build_service(vec![sphere_action(true)]);
        let req = SearchRequest {
            tags: vec!["geometry".into(), "poly".into()],
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 1);
        let req = SearchRequest {
            tags: vec!["geometry".into(), "rig".into()],
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 0);
    }

    #[test]
    fn search_limit_caps_hits() {
        let mut many = Vec::new();
        for i in 0..5 {
            let mut a = sphere_action(true);
            a.action_name = format!("create_{i}");
            many.push(a);
        }
        let (svc, _) = build_service(many);
        let req = SearchRequest {
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(svc.search(&req).total, 2);
    }

    #[test]
    fn describe_returns_schema_when_asked() {
        let (svc, _) = build_service(vec![sphere_action(true)]);
        let slug = ToolSlug::build("maya", "spheres", "create_sphere");
        let d = svc
            .describe(&DescribeRequest {
                tool_slug: slug.clone(),
                include_schema: true,
            })
            .unwrap();
        assert!(d.input_schema.is_some());
        let d = svc
            .describe(&DescribeRequest {
                tool_slug: slug,
                include_schema: false,
            })
            .unwrap();
        assert!(d.input_schema.is_none());
    }

    #[test]
    fn describe_unknown_slug_is_404_class() {
        let (svc, _) = build_service(vec![]);
        let err = svc
            .describe(&DescribeRequest {
                tool_slug: ToolSlug::build("maya", "missing", "tool"),
                include_schema: true,
            })
            .unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::UnknownSlug);
    }

    #[test]
    fn call_rejects_unloaded_skill() {
        let (svc, _) = build_service(vec![sphere_action(false)]);
        let err = svc
            .call(&CallRequest {
                tool_slug: ToolSlug::build("maya", "spheres", "create_sphere"),
                params: Value::Null,
            })
            .unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::SkillNotLoaded);
    }

    #[test]
    fn call_dispatches_and_normalises_slug() {
        let (svc, inv) = build_service(vec![sphere_action(true)]);
        inv.set_next(Ok(serde_json::json!({"created": 1})));
        let out = svc
            .call(&CallRequest {
                tool_slug: ToolSlug::build("maya", "spheres", "create_sphere"),
                params: serde_json::json!({"radius": 1.5}),
            })
            .unwrap();
        assert_eq!(out.slug.0, "maya.spheres.create_sphere");
        assert_eq!(out.output["created"], 1);
        let calls = inv.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "create_sphere");
        assert_eq!(calls[0].1["radius"], 1.5);
    }

    #[test]
    fn invalid_slug_format_is_bad_request() {
        let (svc, _) = build_service(vec![sphere_action(true)]);
        let err = svc
            .call(&CallRequest {
                tool_slug: ToolSlug("not-a-slug".into()),
                params: Value::Null,
            })
            .unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::BadRequest);
    }

    #[test]
    fn context_snapshot_counts_loaded_skills() {
        let (svc, _) = build_service(vec![
            sphere_action(true),
            CatalogAction {
                skill_name: "cubes".into(),
                loaded: true,
                ..sphere_action(true)
            },
            CatalogAction {
                skill_name: "ghosts".into(),
                loaded: false,
                ..sphere_action(false)
            },
        ]);
        svc.update_context(|c| c.dcc = Some("maya".into()));
        let snap = svc.context_snapshot();
        assert_eq!(snap.dcc.as_deref(), Some("maya"));
        assert_eq!(snap.action_count, 3);
        assert_eq!(snap.loaded_skill_count, 2);
    }

    /// Regression guard against token-budget bloat on /v1/search. A
    /// single hit must fit inside a strict byte budget so agents can
    /// page through hundreds of tools per turn without blowing the
    /// context window.
    #[test]
    fn search_hit_stays_under_token_budget() {
        let mut long = sphere_action(true);
        long.description = "x".repeat(5000); // absurdly long on purpose
        let (svc, _) = build_service(vec![long]);
        let resp = svc.search(&SearchRequest::default());
        let hit = &resp.hits[0];
        let serialised = serde_json::to_string(hit).unwrap();
        assert!(
            serialised.len() < crate::SEARCH_HIT_BUDGET_BYTES,
            "search hit serialised to {} bytes (>{} budget) — probable schema expansion",
            serialised.len(),
            crate::SEARCH_HIT_BUDGET_BYTES,
        );
    }
}
