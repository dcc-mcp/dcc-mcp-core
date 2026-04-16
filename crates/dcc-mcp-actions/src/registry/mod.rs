//! ActionRegistry — thread-safe registry for Action classes.
//!
//! Uses DashMap for lock-free concurrent reads, replacing the Python singleton pattern.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::py_json::json_value_to_pyobject;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::default_schema;

/// Metadata about a registered Action (stored in Rust).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Thread-safe Action registry.
///
/// Unlike the Python singleton, each ActionManager can own its own registry,
/// eliminating cross-DCC pollution.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolRegistry", from_py_object)
)]
#[derive(Debug, Clone)]
pub struct ActionRegistry {
    /// Main registry: action_name → ActionMeta
    actions: Arc<DashMap<String, ActionMeta>>,
    /// DCC-specific registry: dcc_name → { action_name → ActionMeta }
    dcc_actions: Arc<DashMap<String, DashMap<String, ActionMeta>>>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            actions: Arc::new(DashMap::new()),
            dcc_actions: Arc::new(DashMap::new()),
        }
    }

    /// Register an action with metadata.
    pub fn register_action(&self, meta: ActionMeta) {
        let name = meta.name.clone();
        let dcc = meta.dcc.clone();
        // Clone meta for the DCC map before moving it into `actions`.
        // This avoids a get-after-insert race where another thread could
        // overwrite the entry between insert and get.
        let meta_for_dcc = meta.clone();
        self.actions.insert(name.clone(), meta);
        self.dcc_actions
            .entry(dcc)
            .or_default()
            .insert(name, meta_for_dcc);
    }

    /// Get action metadata by name.
    #[must_use]
    pub fn get_action(&self, name: &str, dcc_name: Option<&str>) -> Option<ActionMeta> {
        if let Some(dcc) = dcc_name {
            if let Some(dcc_map) = self.dcc_actions.get(dcc) {
                return dcc_map.get(name).map(|r| r.value().clone());
            }
            return None;
        }
        self.actions.get(name).map(|r| r.value().clone())
    }

    /// List all actions for a DCC.
    #[must_use]
    pub fn list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.dcc_actions
            .get(dcc_name)
            .map(|dcc_map| dcc_map.iter().map(|r| r.key().clone()).collect())
            .unwrap_or_default()
    }

    /// List all registered DCC names.
    #[must_use]
    pub fn get_all_dccs(&self) -> Vec<String> {
        self.dcc_actions.iter().map(|r| r.key().clone()).collect()
    }

    /// Get all actions as metadata list.
    #[must_use]
    pub fn list_actions(&self, dcc_name: Option<&str>) -> Vec<ActionMeta> {
        if let Some(dcc) = dcc_name {
            return self
                .dcc_actions
                .get(dcc)
                .map(|dcc_map| dcc_map.iter().map(|r| r.value().clone()).collect())
                .unwrap_or_default();
        }
        self.actions.iter().map(|r| r.value().clone()).collect()
    }

    /// Search actions by category, tags, and/or DCC name.
    ///
    /// All provided filters are AND-ed together:
    /// - `category`: exact match (empty string = no category filter)
    /// - `tags`: action must contain **all** listed tags (empty vec = no tag filter)
    /// - `dcc_name`: scoped to a specific DCC (None = all DCCs)
    ///
    /// Returns all matching `ActionMeta` entries.
    ///
    /// # Example
    ///
    /// ```rust
    /// use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
    ///
    /// let reg = ActionRegistry::new();
    /// reg.register_action(ActionMeta {
    ///     name: "create_sphere".into(),
    ///     category: "geometry".into(),
    ///     tags: vec!["create".into(), "mesh".into()],
    ///     dcc: "maya".into(),
    ///     ..Default::default()
    /// });
    ///
    /// // Find all geometry actions with the "create" tag in maya
    /// let results = reg.search_actions(Some("geometry"), &["create"], Some("maya"));
    /// assert_eq!(results.len(), 1);
    /// ```
    #[must_use]
    pub fn search_actions(
        &self,
        category: Option<&str>,
        tags: &[&str],
        dcc_name: Option<&str>,
    ) -> Vec<ActionMeta> {
        self.list_actions(dcc_name)
            .into_iter()
            .filter(|meta| {
                // Category filter: if provided, must match exactly
                if let Some(cat) = category {
                    if !cat.is_empty() && meta.category != cat {
                        return false;
                    }
                }
                // Tags filter: action must contain ALL requested tags
                if !tags.is_empty() {
                    for tag in tags {
                        if !meta.tags.iter().any(|t| t == tag) {
                            return false;
                        }
                    }
                }
                true
            })
            .collect()
    }

    /// Count actions matching the given search criteria.
    ///
    /// Convenience wrapper around [`search_actions`](Self::search_actions).
    #[must_use]
    pub fn count_actions(
        &self,
        category: Option<&str>,
        tags: &[&str],
        dcc_name: Option<&str>,
    ) -> usize {
        self.search_actions(category, tags, dcc_name).len()
    }

    /// Get all unique categories registered in the registry.
    ///
    /// Optionally scoped to a specific DCC.
    #[must_use]
    pub fn get_categories(&self, dcc_name: Option<&str>) -> Vec<String> {
        let mut categories: Vec<String> = self
            .list_actions(dcc_name)
            .into_iter()
            .filter(|m| !m.category.is_empty())
            .map(|m| m.category)
            .collect();
        categories.sort();
        categories.dedup();
        categories
    }

    /// Get all unique tags registered in the registry.
    ///
    /// Optionally scoped to a specific DCC.
    #[must_use]
    pub fn get_tags(&self, dcc_name: Option<&str>) -> Vec<String> {
        let mut tags: Vec<String> = self
            .list_actions(dcc_name)
            .into_iter()
            .flat_map(|m| m.tags)
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Register multiple actions at once.
    ///
    /// Equivalent to calling [`register_action`](Self::register_action) for each entry,
    /// but avoids repeated lock overhead for large batches.
    pub fn register_batch(&self, metas: impl IntoIterator<Item = ActionMeta>) {
        for meta in metas {
            self.register_action(meta);
        }
    }

    /// Unregister an action by name, optionally scoped to a DCC.
    ///
    /// If `dcc_name` is `None`, removes the action from the global registry AND
    /// from every per-DCC map that contains it.
    ///
    /// If `dcc_name` is `Some`, removes only the per-DCC entry; the global entry
    /// is removed only when no per-DCC map references it any longer.
    ///
    /// Returns `true` if an entry was removed, `false` if the action was not found.
    pub fn unregister(&self, name: &str, dcc_name: Option<&str>) -> bool {
        if let Some(dcc) = dcc_name {
            // Remove from the targeted DCC map only.
            let removed_from_dcc = self
                .dcc_actions
                .get(dcc)
                .map(|dcc_map| dcc_map.remove(name).is_some())
                .unwrap_or(false);
            // Remove the global entry only if no other DCC still has this action.
            let still_referenced = self
                .dcc_actions
                .iter()
                .any(|dcc_map| dcc_map.contains_key(name));
            if !still_referenced {
                self.actions.remove(name);
            }
            removed_from_dcc
        } else {
            // Remove from global registry and ALL per-DCC maps.
            let removed = self.actions.remove(name).is_some();
            for dcc_map in self.dcc_actions.iter() {
                dcc_map.remove(name);
            }
            removed
        }
    }

    /// List all actions belonging to a specific skill.
    #[must_use]
    pub fn list_actions_by_skill(&self, skill_name: &str) -> Vec<ActionMeta> {
        self.actions
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .skill_name
                    .as_ref()
                    .is_some_and(|sn| sn == skill_name)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Unregister all actions belonging to a specific skill.
    ///
    /// Returns the number of actions removed.
    pub fn unregister_skill(&self, skill_name: &str) -> usize {
        let action_names: Vec<String> = self
            .actions
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .skill_name
                    .as_ref()
                    .is_some_and(|sn| sn == skill_name)
            })
            .map(|entry| entry.key().clone())
            .collect();
        let count = action_names.len();
        for name in action_names {
            self.unregister(&name, None);
        }
        count
    }

    /// Clear all registered actions.
    pub fn reset(&self) {
        self.actions.clear();
        self.dcc_actions.clear();
    }

    /// Get number of registered actions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns `true` if no actions are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ActionRegistry {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    /// Register multiple actions at once from a list of dicts.
    ///
    /// Each dict may contain the same keyword arguments as :meth:`register`.
    /// Fields not present in a dict fall back to their defaults.
    /// Entries without a ``"name"`` key (or with an empty name) are silently skipped.
    ///
    /// Example::
    ///
    ///   reg.register_batch([
    ///       {"name": "create_sphere", "category": "geometry", "dcc": "maya"},
    ///       {"name": "delete_object", "category": "edit",     "dcc": "maya"},
    ///   ])
    #[pyo3(name = "register_batch")]
    fn py_register_batch(&self, actions: Vec<pyo3::Bound<'_, pyo3::types::PyAny>>) {
        for item in &actions {
            let Ok(dict) = item.cast::<pyo3::types::PyDict>() else {
                continue;
            };
            let name: String = dict
                .get_item("name")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let description: String = dict
                .get_item("description")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let category: String = dict
                .get_item("category")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let tags: Vec<String> = dict
                .get_item("tags")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let dcc: String = dict
                .get_item("dcc")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_else(|| DEFAULT_DCC.to_string());
            let version: String = dict
                .get_item("version")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_else(|| DEFAULT_VERSION.to_string());
            let input_schema_str: Option<String> = dict
                .get_item("input_schema")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let output_schema_str: Option<String> = dict
                .get_item("output_schema")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let source_file: Option<String> = dict
                .get_item("source_file")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let skill_name: Option<String> = dict
                .get_item("skill_name")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());

            let input_schema =
                parse_schema_or_default(input_schema_str.as_deref(), "input_schema", &name);
            let output_schema =
                parse_schema_or_default(output_schema_str.as_deref(), "output_schema", &name);

            self.register_action(ActionMeta {
                name,
                description,
                category,
                tags,
                dcc,
                version,
                input_schema,
                output_schema,
                source_file,
                skill_name,
            });
        }
    }

    /// Unregister an action by name.
    ///
    /// If ``dcc_name`` is ``None`` (default), the action is removed from the global
    /// registry and every per-DCC map that contains it.
    ///
    /// If ``dcc_name`` is provided, only the per-DCC entry is removed; the global
    /// entry is removed only when no other DCC still references the action.
    ///
    /// Returns ``True`` if the action was found and removed, ``False`` otherwise.
    ///
    /// Example::
    ///
    ///   reg.register(name="create_sphere", dcc="maya")
    ///   assert reg.unregister("create_sphere") is True
    ///   assert reg.unregister("create_sphere") is False  # already gone
    #[pyo3(name = "unregister")]
    #[pyo3(signature = (name, dcc_name=None))]
    fn py_unregister(&self, name: &str, dcc_name: Option<&str>) -> bool {
        self.unregister(name, dcc_name)
    }

    /// Register an action. Called from Python ActionManager.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (name, description="".to_string(), category="".to_string(), tags=vec![], dcc=DEFAULT_DCC.to_string(), version=DEFAULT_VERSION.to_string(), input_schema=None, output_schema=None, source_file=None, skill_name=None))]
    fn register(
        &self,
        name: String,
        description: String,
        category: String,
        tags: Vec<String>,
        dcc: String,
        version: String,
        input_schema: Option<String>,
        output_schema: Option<String>,
        source_file: Option<String>,
        skill_name: Option<String>,
    ) {
        let input_schema = parse_schema_or_default(input_schema.as_deref(), "input_schema", &name);
        let output_schema =
            parse_schema_or_default(output_schema.as_deref(), "output_schema", &name);

        self.register_action(ActionMeta {
            name,
            description,
            category,
            tags,
            dcc,
            version,
            input_schema,
            output_schema,
            source_file,
            skill_name,
        });
    }

    /// Get action metadata as dict.
    #[pyo3(name = "get_action")]
    #[pyo3(signature = (name, dcc_name=None))]
    fn py_get_action(
        &self,
        py: Python,
        name: &str,
        dcc_name: Option<&str>,
    ) -> PyResult<Option<Py<PyAny>>> {
        self.get_action(name, dcc_name)
            .map(|meta| action_meta_to_py(py, &meta))
            .transpose()
    }

    /// List all action names for a DCC.
    #[pyo3(name = "list_actions_for_dcc")]
    fn py_list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.list_actions_for_dcc(dcc_name)
    }

    /// List all actions with metadata.
    #[pyo3(name = "list_actions")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_list_actions(&self, py: Python, dcc_name: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
        self.list_actions(dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    /// Get all registered DCC names.
    #[pyo3(name = "get_all_dccs")]
    fn py_get_all_dccs(&self) -> Vec<String> {
        self.get_all_dccs()
    }

    /// Search actions by category, tags, and/or DCC name.
    ///
    /// All provided filters are AND-ed together:
    ///
    /// - ``category``: exact category match (``None`` or empty string = no filter)
    /// - ``tags``: action must contain **all** listed tags (empty list = no filter)
    /// - ``dcc_name``: scoped to a specific DCC (``None`` = all DCCs)
    ///
    /// Returns a list of action metadata dicts.
    ///
    /// Example::
    ///
    ///   reg.register(name="create_sphere", category="geometry",
    ///                tags=["create", "mesh"], dcc="maya")
    ///   results = reg.search_actions(category="geometry", tags=["create"])
    ///   # [{"name": "create_sphere", ...}]
    #[pyo3(name = "search_actions")]
    #[pyo3(signature = (category=None, tags=vec![], dcc_name=None))]
    fn py_search_actions(
        &self,
        py: Python,
        category: Option<&str>,
        tags: Vec<String>,
        dcc_name: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.search_actions(category, &tag_refs, dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    /// Get all unique categories in the registry.
    ///
    /// Optionally scoped to a specific DCC.
    #[pyo3(name = "get_categories")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_get_categories(&self, dcc_name: Option<&str>) -> Vec<String> {
        self.get_categories(dcc_name)
    }

    /// Get all unique tags in the registry.
    ///
    /// Optionally scoped to a specific DCC.
    #[pyo3(name = "get_tags")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_get_tags(&self, dcc_name: Option<&str>) -> Vec<String> {
        self.get_tags(dcc_name)
    }

    /// Count actions matching the given search criteria.
    ///
    /// Convenience wrapper around :meth:`search_actions`.
    ///
    /// Example::
    ///
    ///   reg.register(name="create_sphere", category="geometry", dcc="maya")
    ///   assert reg.count_actions(category="geometry") == 1
    #[pyo3(name = "count_actions")]
    #[pyo3(signature = (category=None, tags=vec![], dcc_name=None))]
    fn py_count_actions(
        &self,
        category: Option<&str>,
        tags: Vec<String>,
        dcc_name: Option<&str>,
    ) -> usize {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.count_actions(category, &tag_refs, dcc_name)
    }

    /// Reset the registry.
    #[pyo3(name = "reset")]
    fn py_reset(&self) {
        self.reset()
    }

    fn __len__(&self) -> usize {
        self.len()
    }

    fn __contains__(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    fn __repr__(&self) -> String {
        format!("ToolRegistry(actions={})", self.len())
    }
}

/// Parse a JSON schema string, falling back to [`default_schema`] on `None` or invalid JSON.
#[cfg(feature = "python-bindings")]
fn parse_schema_or_default(
    json: Option<&str>,
    field_name: &str,
    action_name: &str,
) -> serde_json::Value {
    match json {
        Some(s) => serde_json::from_str(s).unwrap_or_else(|e| {
            tracing::warn!("Invalid {field_name} JSON for '{action_name}': {e} — using default");
            default_schema().clone()
        }),
        None => default_schema().clone(),
    }
}

/// Convert [`ActionMeta`] to a Python dict via serde serialization.
///
/// This leverages the existing `#[derive(Serialize)]` on `ActionMeta` to avoid
/// manually enumerating every field — new fields are automatically included.
#[cfg(feature = "python-bindings")]
fn action_meta_to_py(py: Python, meta: &ActionMeta) -> PyResult<Py<PyAny>> {
    let json_val = serde_json::to_value(meta)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &json_val)
}

#[cfg(test)]
mod tests;
