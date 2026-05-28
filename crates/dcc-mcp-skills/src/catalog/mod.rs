//! SkillCatalog — progressive skill discovery, loading, and unloading.
//!
//! Manages a catalog of discovered skills and their load state. Skills are
//! discovered via `SkillScanner`/`SkillWatcher`, and their tools are registered
//! into `ToolRegistry` on demand via `load_skill` / `unload_skill`.
//!
//! # Architecture
//!
//! ```text
//! [SKILL.md files] --scan--> SkillEntry(Discovered) --load_skill--> SkillEntry(Loaded)
//!                                    │                                    │
//!                              search_skills()                     tools in ToolRegistry
//!                              list_skills()                     + tools/list_changed notification
//!                              get_skill_info()
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use dcc_mcp_skills::catalog::SkillCatalog;
//! use dcc_mcp_actions::ToolRegistry;
//! use std::sync::Arc;
//!
//! let registry = Arc::new(ToolRegistry::new());
//! let catalog = SkillCatalog::new(registry);
//!
//! // Discover skills from standard paths
//! catalog.discover(None, Some("maya"));
//!
//! // Search for skills
//! let results = catalog.search_skills(Some("modeling"), &[], Some("maya"), None, None);
//!
//! // Load a skill — registers its tools in ToolRegistry
//! let loaded = catalog.load_skill("modeling-bevel");
//! ```

pub mod execute;
pub mod persistence;
pub mod schema_gen;
pub mod scoring;
pub mod types;

pub use persistence::{
    DriftRecord, FailedRecord, LoadReplayPolicy, LoadedSkillRecord, PersistedCatalogState,
    ReplayReport,
};
pub use types::{SkillDetail, SkillEntry, SkillState, SkillSummary};

use execute::{ScriptExecutorFn, execute_script, resolve_tool_script};

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

use dashmap::{DashMap, DashSet};
use dcc_mcp_actions::{
    EventBus, EventVeto, ToolDispatcher,
    registry::{ToolMeta, ToolRegistry},
};
use dcc_mcp_models::registry::{Registry, SearchQuery};
use dcc_mcp_models::{RegistryEntry as _, SkillGroup, SkillMetadata, SkillScope};
use parking_lot::RwLock;
use std::sync::Arc;

use crate::loader;

#[allow(clippy::module_inception)]
mod catalog;
mod groups;
pub(crate) mod helpers;
pub mod list_projection;

// PyO3 bindings live in `crate::python::catalog`.

#[cfg(test)]
pub(crate) use helpers::parse_scope_str;

pub type SkillLoadTransformFn =
    dyn Fn(SkillMetadata) -> Result<SkillMetadata, String> + Send + Sync;
pub type AfterSkillLoadFn = dyn Fn(&SkillMetadata, &[String]) -> Result<(), String> + Send + Sync;
/// Observer invoked after a successful [`SkillCatalog::unload_skill`] —
/// receives the unloaded skill's name and the tool names that were
/// unregistered. Used by the persistence layer (#1405) to evict the
/// corresponding row from the on-disk store.
pub type AfterSkillUnloadFn = dyn Fn(&str, &[String]) -> Result<(), String> + Send + Sync;
/// Observer invoked after a successful
/// [`SkillCatalog::activate_group`] / [`SkillCatalog::deactivate_group`].
/// `activated` is `true` for activate, `false` for deactivate. Used by
/// the persistence layer (#1405) to mirror catalog-wide group state.
pub type AfterGroupChangeFn = dyn Fn(&str, bool) -> Result<(), String> + Send + Sync;

// ── SkillCatalog ──

/// Manages discovered skills and their progressive loading.
///
/// Thread-safe: all state is stored in `DashMap` / `DashSet`.
///
/// When a dispatcher is attached (via [`SkillCatalog::with_dispatcher`]),
/// loading a skill also registers a handler for each action — enabling the
/// Skills-First workflow where agents never need to register handlers manually.
///
/// # Execution modes
///
/// - **In-process** (preferred inside a DCC): register a
///   [`ScriptExecutorFn`] via [`with_in_process_executor`](Self::with_in_process_executor).
///   Scripts are executed directly in the host DCC's Python interpreter so
///   DCC APIs (`maya.cmds`, `bpy`, `hou`, …) are available without spawning
///   any external process.
/// - **Subprocess** (default): each skill script is executed as a child
///   process. Suitable for standalone / non-DCC environments.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(feature = "python-bindings", pyo3::pyclass(name = "SkillCatalog"))]
pub struct SkillCatalog {
    /// All discovered skill entries, keyed by skill name.
    pub(super) entries: DashMap<String, SkillEntry>,
    /// Set of skill names currently loaded.
    pub(super) loaded: DashSet<String>,
    /// Reference to ToolRegistry for registering/unregistering tools.
    pub(super) registry: Arc<ToolRegistry>,
    /// Optional dispatcher for auto-registering script handlers on load.
    pub(super) dispatcher: Option<Arc<ToolDispatcher>>,
    /// In-process lifecycle event bus for skill load/unload events.
    pub(super) event_bus: EventBus,
    /// Optional in-process script executor.
    ///
    /// When set, skill scripts are run inside the current Python interpreter
    /// instead of being dispatched to a subprocess.  DCC adapters should
    /// register one of these via [`with_in_process_executor`](Self::with_in_process_executor)
    /// so that `maya.cmds`, `bpy`, `hou`, etc. are available to the scripts.
    pub(super) script_executor: RwLock<Option<Arc<ScriptExecutorFn>>>,
    /// Optional adapter policy hook applied before a skill registers tools.
    pub(super) load_transform: RwLock<Option<Arc<SkillLoadTransformFn>>>,
    /// Optional observer called after tools are registered and the catalog state is loaded.
    pub(super) after_load_hook: RwLock<Option<Arc<AfterSkillLoadFn>>>,
    /// Optional observer called after a skill is unloaded (#1405).
    pub(super) after_unload_hook: RwLock<Option<Arc<AfterSkillUnloadFn>>>,
    /// Optional observer called after a tool group is activated or
    /// deactivated (#1405). Lets the persistence layer mirror the
    /// catalog-wide [`SkillCatalog::active_groups`] set on disk.
    pub(super) after_group_change_hook: RwLock<Option<Arc<AfterGroupChangeFn>>>,
    /// Tool groups currently active (`"<skill>:<group>"` keys).
    pub(super) active_groups: DashSet<String>,
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

/// Return whether the group named ``group_name`` should be active at load-time.
///
/// Empty ``group_name`` (the "always-on" default group) is always active.
/// Otherwise, the group must be declared in ``groups`` with
/// ``default_active = true``.
pub(crate) fn group_default_active(groups: &[SkillGroup], group_name: &str) -> bool {
    if group_name.is_empty() {
        return true;
    }
    groups
        .iter()
        .find(|g| g.name == group_name)
        .map(|g| g.default_active)
        .unwrap_or(false)
}

// ── impl Registry<SkillEntry> ────────────────────────────────────────────────

/// Satisfy the shared [`Registry<SkillEntry>`] contract.
///
/// Delegates to the internal `DashMap<String, SkillEntry>` directly so that
/// file-hash tracking, `loaded` bookkeeping, and per-DCC indexing are
/// unaffected.  Callers that need those richer features should use the
/// dedicated methods (`add_skill`, `load_skill`, `remove_skill`, …) instead.
impl Registry<SkillEntry> for SkillCatalog {
    fn register(&self, entry: SkillEntry) {
        self.entries.insert(entry.key(), entry);
        self.refresh_dependency_states();
    }

    fn get(&self, key: &str) -> Option<SkillEntry> {
        self.entries.get(key).map(|e| e.value().clone())
    }

    fn list(&self) -> Vec<SkillEntry> {
        self.entries.iter().map(|e| e.value().clone()).collect()
    }

    fn remove(&self, key: &str) -> bool {
        let removed = self.entries.remove(key).is_some();
        if removed {
            self.refresh_dependency_states();
        }
        removed
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    fn search(&self, query: &SearchQuery) -> Vec<SkillEntry> {
        let q = query.query.to_ascii_lowercase();
        let mut results: Vec<SkillEntry> = self
            .entries
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
