//! dcc-mcp-skills: SKILL.md scanning, loading, dependency resolution, hot-reload, and progressive discovery.

pub mod catalog;
mod loader;
pub mod manager;
pub mod resolver;
mod scanner;
pub mod validator;
pub mod watcher;

pub use catalog::{SkillCatalog, SkillDetail, SkillState, SkillSummary};
pub use loader::{LoadResult, parse_skill_md, scan_and_load, scan_and_load_lenient};
pub use manager::SkillsManager;
pub use resolver::{
    ResolveError, ResolvedSkills, expand_transitive_dependencies, resolve_dependencies,
    validate_dependencies,
};
pub use scanner::SkillScanner;
pub use validator::{SkillValidationIssue, SkillValidationReport, validate_skill_dir};
pub use watcher::{SkillWatcher, WatcherError};

#[cfg(feature = "python-bindings")]
pub use loader::{py_parse_skill_md, py_scan_and_load, py_scan_and_load_lenient};
#[cfg(feature = "python-bindings")]
pub use resolver::{
    py_expand_transitive_dependencies, py_resolve_dependencies, py_validate_dependencies,
};
#[cfg(feature = "python-bindings")]
pub use scanner::py_scan_skill_paths;
#[cfg(feature = "python-bindings")]
pub use validator::{PySkillValidationIssue, PySkillValidationReport, py_validate_skill};
#[cfg(feature = "python-bindings")]
pub use watcher::PySkillWatcher;
