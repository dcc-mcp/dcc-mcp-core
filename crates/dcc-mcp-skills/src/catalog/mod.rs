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
pub mod scoring;
pub mod types;

pub use types::{SkillDetail, SkillEntry, SkillState, SkillSummary};

use execute::{ScriptExecutorFn, execute_script, resolve_tool_script};

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::{DashMap, DashSet};
use dcc_mcp_actions::{
    ActionDispatcher,
    registry::{ActionMeta, ActionRegistry},
};
use dcc_mcp_models::{SkillGroup, SkillMetadata, SkillScope};
use std::sync::Arc;

use crate::loader;

mod catalog;
mod groups;
mod helpers;
#[cfg(feature = "python-bindings")]
mod python;

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

#[cfg(test)]
mod tests;
