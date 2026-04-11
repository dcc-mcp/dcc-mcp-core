//! SkillCatalog — progressive skill discovery, loading, and unloading.
//!
//! Manages a catalog of discovered skills and their load state. Skills are
//! discovered via `SkillScanner`/`SkillWatcher`, and their tools are registered
//! into `ActionRegistry` on demand via `load_skill` / `unload_skill`.
//!
//! # Architecture
//!
//! ```text
//! [SKILL.md files] --scan--> SkillEntry(Discovered) --load_skill--> SkillEntry(Loaded)
//!                                    │                                    │
//!                              find_skills()                     tools in ActionRegistry
//!                              list_skills()                     + tools/list_changed notification
//!                              get_skill_info()
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use dcc_mcp_skills::catalog::SkillCatalog;
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//!
//! let registry = Arc::new(ActionRegistry::new());
//! let catalog = SkillCatalog::new(registry);
//!
//! // Discover skills from standard paths
//! catalog.discover(None, Some("maya"));
//!
//! // Search for skills
//! let results = catalog.find_skills(Some("modeling"), &[], Some("maya"));
//!
//! // Load a skill — registers its tools in ActionRegistry
//! let loaded = catalog.load_skill("modeling-bevel");
//! ```

pub mod execute;
pub mod types;

pub use types::{SkillDetail, SkillEntry, SkillState, SkillSummary};

use execute::{execute_script, resolve_tool_script};

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::{DashMap, DashSet};
use dcc_mcp_actions::{
    ActionDispatcher,
    registry::{ActionMeta, ActionRegistry},
};
use dcc_mcp_models::SkillMetadata;
use std::sync::Arc;

use crate::loader;

// ── SkillCatalog ──

/// Manages discovered skills and their progressive loading.
///
/// Thread-safe: all state is stored in `DashMap` / `DashSet`.
///
/// When a dispatcher is attached (via [`SkillCatalog::with_dispatcher`]),
/// loading a skill also registers a subprocess-based handler for each
/// action — enabling the Skills-First workflow where agents never need to
/// register handlers manually.
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillCatalog"))]
pub struct SkillCatalog {
    /// All discovered skill entries, keyed by skill name.
    entries: DashMap<String, SkillEntry>,
    /// Set of skill names currently loaded.
    loaded: DashSet<String>,
    /// Reference to ActionRegistry for registering/unregistering tools.
    registry: Arc<ActionRegistry>,
    /// Optional dispatcher for auto-registering script handlers on load.
    dispatcher: Option<Arc<ActionDispatcher>>,
}

impl std::fmt::Debug for SkillCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let discovered = self
            .entries
            .iter()
            .filter(|e| e.value().state == SkillState::Discovered)
            .count();
        let loaded = self.loaded.len();
        f.debug_struct("SkillCatalog")
            .field("discovered", &discovered)
            .field("loaded", &loaded)
            .field("total", &self.entries.len())
            .finish()
    }
}

impl SkillCatalog {
    /// Create a new, empty catalog backed by the given registry.
    ///
    /// Without a dispatcher, `load_skill` only registers action metadata.
    /// Use [`with_dispatcher`](Self::with_dispatcher) to also auto-register
    /// script handlers for the Skills-First workflow.
    pub fn new(registry: Arc<ActionRegistry>) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: None,
        }
    }

    /// Create a catalog with an attached dispatcher for Skills-First execution.
    ///
    /// When a dispatcher is attached, calling `load_skill` automatically
    /// registers a subprocess-based handler for every script in the skill.
    /// Agents can then call `tools/call` and have scripts actually execute.
    pub fn new_with_dispatcher(
        registry: Arc<ActionRegistry>,
        dispatcher: Arc<ActionDispatcher>,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: Some(dispatcher),
        }
    }

    /// Attach a dispatcher after construction (builder-style).
    pub fn with_dispatcher(mut self, dispatcher: Arc<ActionDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Discover skills from the standard scan paths.
    ///
    /// Uses `scan_and_load_lenient` internally so skills with missing
    /// dependencies are skipped rather than causing an error.
    ///
    /// Returns the number of newly discovered skills.
    pub fn discover(&self, extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> usize {
        let result = match loader::scan_and_load_lenient(extra_paths, dcc_name) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("SkillCatalog: discovery failed: {e}");
                return 0;
            }
        };

        let mut new_count = 0;
        for skill in result.skills {
            let name = skill.name.clone();
            // Only insert if not already known (don't overwrite loaded state)
            if !self.entries.contains_key(&name) {
                self.entries.insert(
                    name,
                    SkillEntry {
                        metadata: skill,
                        state: SkillState::Discovered,
                        registered_actions: Vec::new(),
                    },
                );
                new_count += 1;
            }
        }

        if !result.skipped.is_empty() {
            tracing::debug!("SkillCatalog: skipped {} directories", result.skipped.len());
        }

        tracing::info!(
            "SkillCatalog: discovered {} new skill(s), total {}",
            new_count,
            self.entries.len()
        );
        new_count
    }

    /// Add a single skill to the catalog (e.g. from SkillWatcher).
    ///
    /// If the skill is already in the catalog and loaded, it is not replaced.
    /// If it exists but is discovered, the metadata is updated.
    pub fn add_skill(&self, metadata: SkillMetadata) {
        let name = metadata.name.clone();
        if let Some(mut entry) = self.entries.get_mut(&name) {
            // Only update metadata if not loaded
            if entry.state != SkillState::Loaded {
                entry.metadata = metadata;
                entry.state = SkillState::Discovered;
            }
        } else {
            self.entries.insert(
                name,
                SkillEntry {
                    metadata,
                    state: SkillState::Discovered,
                    registered_actions: Vec::new(),
                },
            );
        }
    }

    /// Load a skill by name — registers its tools into ActionRegistry and,
    /// if a dispatcher is attached, auto-registers script execution handlers.
    ///
    /// This is the Skills-First path: agents can call `load_skill` and then
    /// immediately use `tools/call` without any extra handler registration.
    ///
    /// **Script lookup order** for each action:
    /// 1. `ToolDeclaration.source_file` (explicit mapping)
    /// 2. `scripts/<tool_name>.<ext>` (name-matched script)
    /// 3. The first script in the skill if only one script exists
    /// 4. No handler registered (tool visible but not executable)
    ///
    /// Returns the list of action names that were registered, or an error
    /// description if the skill could not be loaded.
    pub fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, String> {
        // Check if already loaded
        if self.loaded.contains(skill_name) {
            let actions = self
                .entries
                .get(skill_name)
                .map(|e| e.registered_actions.clone())
                .unwrap_or_default();
            return Ok(actions);
        }

        // Get the skill entry
        let metadata = {
            self.entries
                .get(skill_name)
                .map(|e| e.metadata.clone())
                .ok_or_else(|| format!("Skill '{skill_name}' not found in catalog"))
        }?;

        // Register tools from the skill
        let mut registered = Vec::new();
        let skill_base = metadata.name.replace('-', "_");
        let skill_path = std::path::Path::new(&metadata.skill_path);

        for tool_decl in &metadata.tools {
            let action_name = if tool_decl.name.contains("__") {
                tool_decl.name.clone()
            } else {
                format!("{}__{}", skill_base, tool_decl.name.replace('-', "_"))
            };

            // Resolve the script that backs this tool declaration
            let script_path = resolve_tool_script(tool_decl, &metadata.scripts, skill_path);

            let meta = ActionMeta {
                name: action_name.clone(),
                description: if tool_decl.description.is_empty() {
                    format!("[{}] {}", metadata.name, metadata.description)
                } else {
                    tool_decl.description.clone()
                },
                category: metadata.tags.first().cloned().unwrap_or_default(),
                tags: metadata.tags.clone(),
                dcc: metadata.dcc.clone(),
                version: metadata.version.clone(),
                input_schema: if tool_decl.input_schema.is_null() {
                    serde_json::json!({"type": "object"})
                } else {
                    tool_decl.input_schema.clone()
                },
                output_schema: tool_decl.output_schema.clone(),
                source_file: script_path.clone(),
                skill_name: Some(skill_name.to_string()),
            };

            self.registry.register_action(meta);

            // Auto-register subprocess handler if dispatcher is attached
            if let (Some(dispatcher), Some(sp)) = (&self.dispatcher, script_path) {
                let sp_owned = sp.clone();
                let name_clone = action_name.clone();
                dispatcher
                    .register_handler(&name_clone, move |params| execute_script(&sp_owned, params));
            }

            registered.push(action_name);
        }

        // Script-only path: no explicit tool declarations → one action per script
        if metadata.tools.is_empty() {
            for script_path in &metadata.scripts {
                let stem = std::path::Path::new(script_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let action_name = format!("{}__{}", skill_base, stem.replace('-', "_"));

                let meta = ActionMeta {
                    name: action_name.clone(),
                    description: format!("[{}] {}", metadata.name, metadata.description),
                    category: metadata.tags.first().cloned().unwrap_or_default(),
                    tags: metadata.tags.clone(),
                    dcc: metadata.dcc.clone(),
                    version: metadata.version.clone(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: serde_json::Value::Null,
                    source_file: Some(script_path.clone()),
                    skill_name: Some(skill_name.to_string()),
                };

                self.registry.register_action(meta);

                // Auto-register handler
                if let Some(dispatcher) = &self.dispatcher {
                    let sp = script_path.clone();
                    let name_clone = action_name.clone();
                    dispatcher
                        .register_handler(&name_clone, move |params| execute_script(&sp, params));
                }

                registered.push(action_name);
            }
        }

        // Update catalog state
        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Loaded;
            entry.registered_actions = registered.clone();
        }
        self.loaded.insert(skill_name.to_string());

        tracing::info!(
            "SkillCatalog: loaded skill '{}' ({} tools registered, handlers: {})",
            skill_name,
            registered.len(),
            if self.dispatcher.is_some() {
                "auto"
            } else {
                "none"
            }
        );

        Ok(registered)
    }

    /// Load multiple skills at once.
    ///
    /// Returns a map of skill_name -> Ok(action_names) or Err(error_msg).
    pub fn load_skills(
        &self,
        skill_names: &[String],
    ) -> std::collections::HashMap<String, Result<Vec<String>, String>> {
        let mut results = std::collections::HashMap::new();
        for name in skill_names {
            results.insert(name.clone(), self.load_skill(name));
        }
        results
    }

    /// Unload a skill — removes its tools from ActionRegistry and dispatcher.
    ///
    /// Returns the number of actions that were unregistered.
    pub fn unload_skill(&self, skill_name: &str) -> Result<usize, String> {
        if !self.loaded.contains(skill_name) {
            return Err(format!("Skill '{skill_name}' is not loaded"));
        }

        // Collect action names before unregistering
        let action_names: Vec<String> = self
            .entries
            .get(skill_name)
            .map(|e| e.registered_actions.clone())
            .unwrap_or_default();

        // Remove handlers from dispatcher
        if let Some(dispatcher) = &self.dispatcher {
            for name in &action_names {
                dispatcher.remove_handler(name);
            }
        }

        let count = self.registry.unregister_skill(skill_name);

        // Update catalog state
        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Discovered;
            entry.registered_actions.clear();
        }
        self.loaded.remove(skill_name);

        tracing::info!(
            "SkillCatalog: unloaded skill '{}' ({} tools removed)",
            skill_name,
            count
        );

        Ok(count)
    }

    /// Search for skills matching the given criteria.
    ///
    /// Query matches against: `name`, `description`, `search_hint`, and `tool_names`.
    /// All filters are AND-ed together. Empty/None filters match everything.
    pub fn find_skills(
        &self,
        query: Option<&str>,
        tags: &[&str],
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        self.entries
            .iter()
            .filter(|entry| {
                let meta = &entry.value().metadata;

                // Query filter: match name, description, search_hint, and tool names
                if let Some(q) = query {
                    if !q.is_empty() {
                        let q_lower = q.to_lowercase();
                        let hint = if meta.search_hint.is_empty() {
                            &meta.description
                        } else {
                            &meta.search_hint
                        };
                        let matched = meta.name.to_lowercase().contains(&q_lower)
                            || meta.description.to_lowercase().contains(&q_lower)
                            || hint.to_lowercase().contains(&q_lower)
                            || meta
                                .tools
                                .iter()
                                .any(|t| t.name.to_lowercase().contains(&q_lower));
                        if !matched {
                            return false;
                        }
                    }
                }

                // Tags filter: skill must contain ALL requested tags
                if !tags.is_empty() {
                    for tag in tags {
                        if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                            return false;
                        }
                    }
                }

                // DCC filter
                if let Some(dcc_filter) = dcc {
                    if !dcc_filter.is_empty() && !meta.dcc.eq_ignore_ascii_case(dcc_filter) {
                        return false;
                    }
                }

                true
            })
            .map(|entry| skill_entry_to_summary(entry.value()))
            .collect()
    }

    /// List all skills with their load status.
    pub fn list_skills(&self, status: Option<&str>) -> Vec<SkillSummary> {
        self.entries
            .iter()
            .filter(|entry| {
                let state = &entry.value().state;
                match status {
                    Some("loaded") => state == &SkillState::Loaded,
                    Some("unloaded") | Some("discovered") => state == &SkillState::Discovered,
                    Some("error") => matches!(state, SkillState::Error(_)),
                    _ => true, // "all" or None
                }
            })
            .map(|entry| skill_entry_to_summary(entry.value()))
            .collect()
    }

    /// Get detailed information about a specific skill.
    pub fn get_skill_info(&self, skill_name: &str) -> Option<SkillDetail> {
        self.entries.get(skill_name).map(|entry| {
            let e = entry.value();
            SkillDetail {
                name: e.metadata.name.clone(),
                description: e.metadata.description.clone(),
                tags: e.metadata.tags.clone(),
                dcc: e.metadata.dcc.clone(),
                version: e.metadata.version.clone(),
                depends: e.metadata.depends.clone(),
                skill_path: e.metadata.skill_path.clone(),
                scripts: e.metadata.scripts.clone(),
                tools: e.metadata.tools.clone(),
                state: e.state.to_string(),
                registered_actions: e.registered_actions.clone(),
            }
        })
    }

    /// Get the number of skills in the catalog.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the number of loaded skills.
    #[must_use]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Check whether a specific skill is loaded.
    #[must_use]
    pub fn is_loaded(&self, skill_name: &str) -> bool {
        self.loaded.contains(skill_name)
    }

    /// Remove a skill from the catalog entirely.
    ///
    /// If the skill is loaded, it is unloaded first.
    pub fn remove_skill(&self, skill_name: &str) -> bool {
        if self.loaded.contains(skill_name) {
            let _ = self.unload_skill(skill_name);
        }
        self.entries.remove(skill_name).is_some()
    }

    /// Clear all skills from the catalog.
    ///
    /// Loaded skills are unloaded first.
    pub fn clear(&self) {
        let loaded_names: Vec<String> = self.loaded.iter().map(|r| r.key().clone()).collect();
        for name in loaded_names {
            let _ = self.unload_skill(&name);
        }
        self.entries.clear();
    }

    /// Get a reference to the underlying ActionRegistry.
    pub fn registry(&self) -> &Arc<ActionRegistry> {
        &self.registry
    }

    /// Get a reference to the attached dispatcher, if any.
    pub fn dispatcher(&self) -> Option<&Arc<ActionDispatcher>> {
        self.dispatcher.as_ref()
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Convert a SkillEntry into a SkillSummary.
///
/// The `search_hint` falls back to `description` if not set in SKILL.md.
fn skill_entry_to_summary(e: &SkillEntry) -> SkillSummary {
    SkillSummary {
        name: e.metadata.name.clone(),
        description: e.metadata.description.clone(),
        search_hint: if e.metadata.search_hint.is_empty() {
            e.metadata.description.clone()
        } else {
            e.metadata.search_hint.clone()
        },
        tags: e.metadata.tags.clone(),
        dcc: e.metadata.dcc.clone(),
        version: e.metadata.version.clone(),
        tool_count: e.metadata.tools.len(),
        tool_names: e.metadata.tools.iter().map(|t| t.name.clone()).collect(),
        loaded: e.state == SkillState::Loaded,
    }
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillCatalog {
    #[new]
    fn py_new(registry: ActionRegistry) -> Self {
        Self::new(Arc::new(registry))
    }

    /// Discover skills from standard scan paths.
    ///
    /// Args:
    ///     extra_paths: Additional directories to scan.
    ///     dcc_name: DCC name filter (e.g. "maya", "blender").
    ///
    /// Returns the number of newly discovered skills.
    #[pyo3(name = "discover")]
    #[pyo3(signature = (extra_paths=None, dcc_name=None))]
    fn py_discover(&self, extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> usize {
        self.discover(extra_paths.as_deref(), dcc_name)
    }

    /// Load a skill by name — registers its tools.
    ///
    /// Returns a list of registered action names.
    /// Raises ValueError if the skill is not found.
    #[pyo3(name = "load_skill")]
    fn py_load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.load_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Unload a skill — removes its tools from the registry.
    ///
    /// Returns the number of actions removed.
    /// Raises ValueError if the skill is not loaded.
    #[pyo3(name = "unload_skill")]
    fn py_unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.unload_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Search for skills matching criteria.
    #[pyo3(name = "find_skills")]
    #[pyo3(signature = (query=None, tags=vec![], dcc=None))]
    fn py_find_skills(
        &self,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.find_skills(query, &tag_refs, dcc)
    }

    /// List all skills with their load status.
    #[pyo3(name = "list_skills")]
    #[pyo3(signature = (status=None))]
    fn py_list_skills(&self, status: Option<&str>) -> Vec<SkillSummary> {
        self.list_skills(status)
    }

    /// Get detailed info about a specific skill.
    ///
    /// Returns None if the skill is not found. The detail is returned as a
    /// Python dict (serialized via serde_json).
    #[pyo3(name = "get_skill_info")]
    fn py_get_skill_info(&self, py: Python<'_>, skill_name: &str) -> PyResult<Option<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        match self.get_skill_info(skill_name) {
            Some(info) => {
                let val = serde_json::to_value(&info)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(Some(json_value_to_pyobject(py, &val)?))
            }
            None => Ok(None),
        }
    }

    /// Number of skills in the catalog.
    fn __len__(&self) -> usize {
        self.len()
    }

    /// Whether the catalog is empty.
    fn __bool__(&self) -> bool {
        !self.is_empty()
    }

    /// Check if a skill is loaded.
    #[pyo3(name = "is_loaded")]
    fn py_is_loaded(&self, skill_name: &str) -> bool {
        self.is_loaded(skill_name)
    }

    /// Number of loaded skills.
    #[pyo3(name = "loaded_count")]
    fn py_loaded_count(&self) -> usize {
        self.loaded_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillCatalog(total={}, loaded={})",
            self.len(),
            self.loaded_count()
        )
    }
}

// ── Tests ──

#[cfg(test)]
mod tests;
