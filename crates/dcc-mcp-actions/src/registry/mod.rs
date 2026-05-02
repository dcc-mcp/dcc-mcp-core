//! ActionRegistry — thread-safe registry for DCC tools.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

#[cfg(feature = "python-bindings")]
use dcc_mcp_pybridge::py_json::json_value_to_pyobject;

use dashmap::DashMap;
use dcc_mcp_models::registry::{Registry, SearchQuery};
use std::sync::Arc;

#[cfg(feature = "python-bindings")]
use dcc_mcp_naming::{DEFAULT_DCC, DEFAULT_VERSION};

/// Default JSON schema for action input/output when none is provided.
///
/// Backed by a [`std::sync::LazyLock`] so the value is allocated at most once
/// per process. Callers that need ownership should `.clone()` the returned
/// reference.
#[cfg(feature = "python-bindings")]
fn default_schema() -> &'static serde_json::Value {
    use std::sync::LazyLock;
    static DEFAULT_SCHEMA: LazyLock<serde_json::Value> =
        LazyLock::new(|| serde_json::json!({"type": "object", "properties": {}}));
    &DEFAULT_SCHEMA
}

mod meta;
#[cfg(feature = "python-bindings")]
mod python;

pub use meta::ActionMeta;

/// Thread-safe Action registry.
///
/// Each registry instance is independent, eliminating cross-DCC pollution.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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
                if let Some(cat) = category
                    && !cat.is_empty()
                    && meta.category != cat
                {
                    return false;
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

    // ── Group / enabled helpers (progressive tool exposure) ──────────────

    /// Set ``enabled`` flag for every action with the given ``group`` value.
    ///
    /// Returns the number of actions whose state changed.
    pub fn set_group_enabled(&self, group: &str, enabled: bool) -> usize {
        let mut changed = 0;
        for mut entry in self.actions.iter_mut() {
            if entry.value().group == group && entry.value().enabled != enabled {
                entry.value_mut().enabled = enabled;
                changed += 1;
            }
        }
        for dcc_map in self.dcc_actions.iter() {
            for mut entry in dcc_map.value().iter_mut() {
                if entry.value().group == group && entry.value().enabled != enabled {
                    entry.value_mut().enabled = enabled;
                }
            }
        }
        changed
    }

    /// Enable or disable a single action by name.
    ///
    /// Returns ``true`` if the action existed.
    pub fn set_action_enabled(&self, name: &str, enabled: bool) -> bool {
        let Some(mut entry) = self.actions.get_mut(name) else {
            return false;
        };
        entry.value_mut().enabled = enabled;
        for dcc_map in self.dcc_actions.iter() {
            if let Some(mut e) = dcc_map.value().get_mut(name) {
                e.value_mut().enabled = enabled;
            }
        }
        true
    }

    /// List actions belonging to a specific group (all DCCs).
    pub fn list_actions_in_group(&self, group: &str) -> Vec<ActionMeta> {
        self.actions
            .iter()
            .filter(|e| e.value().group == group)
            .map(|e| e.value().clone())
            .collect()
    }

    /// List currently enabled actions.
    pub fn list_actions_enabled(&self, dcc_name: Option<&str>) -> Vec<ActionMeta> {
        self.list_actions(dcc_name)
            .into_iter()
            .filter(|m| m.enabled)
            .collect()
    }

    /// Enumerate distinct group names present in the registry.
    pub fn list_groups(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for entry in self.actions.iter() {
            let g = &entry.value().group;
            if !g.is_empty() && !seen.contains(g) {
                seen.push(g.clone());
            }
        }
        seen
    }
}

// ── impl Registry<ActionMeta> ────────────────────────────────────────────────

/// Satisfy the shared [`Registry<ActionMeta>`] contract.
///
/// Delegates to the existing `register_action` / `get_action` / `list_actions`
/// / `unregister` methods so all per-DCC indexing is preserved byte-for-byte.
impl Registry<ActionMeta> for ActionRegistry {
    fn register(&self, entry: ActionMeta) {
        self.register_action(entry);
    }

    fn get(&self, key: &str) -> Option<ActionMeta> {
        self.get_action(key, None)
    }

    fn list(&self) -> Vec<ActionMeta> {
        self.list_actions(None)
    }

    fn remove(&self, key: &str) -> bool {
        self.unregister(key, None)
    }

    fn count(&self) -> usize {
        self.len()
    }

    fn search(&self, query: &SearchQuery) -> Vec<ActionMeta> {
        use dcc_mcp_models::RegistryEntry as _;
        let q = query.query.to_ascii_lowercase();
        let mut results: Vec<ActionMeta> = self
            .actions
            .iter()
            .filter(|e| {
                e.value()
                    .search_tags()
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&q))
            })
            .map(|e| e.value().clone())
            .collect();
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        results
    }
}

#[cfg(test)]
mod tests;
