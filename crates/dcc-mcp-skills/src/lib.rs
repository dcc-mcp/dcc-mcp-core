//! dcc-mcp-skills: SKILL.md scanning, loading, dependency resolution, hot-reload, and progressive discovery.

pub mod catalog;
pub mod constants;
pub mod feedback;
pub mod gui_executable;
mod loader;
pub mod manager;
pub mod paths;
pub mod resolver;
mod scanner;
pub mod validator;
pub mod versioning;
pub mod watcher;

#[cfg(feature = "python-bindings")]
pub(crate) mod python;

pub use catalog::{SkillCatalog, SkillDetail, SkillState, SkillSummary};
pub use feedback::{SkillFeedback, get_skill_feedback, record_skill_feedback};
pub use gui_executable::{GuiExecutableHint, correct_python_executable, is_gui_executable};
pub use loader::{
    LoadResult, parse_skill_md, scan_and_load, scan_and_load_lenient, scan_and_load_strict,
    scan_and_load_team, scan_and_load_team_lenient, scan_and_load_user, scan_and_load_user_lenient,
};
pub use manager::SkillsManager;
pub use paths::{
    copy_skill_to_team_dir, copy_skill_to_user_dir, get_app_skill_paths_from_env,
    get_app_team_skill_paths_from_env, get_app_user_skill_paths_from_env, get_skill_paths_from_env,
    get_skills_dir, get_team_skill_paths_from_env, get_team_skills_dir,
    get_user_skill_paths_from_env, get_user_skills_dir,
};
pub use resolver::{
    ResolveError, ResolvedSkills, expand_transitive_dependencies, resolve_dependencies,
    validate_dependencies,
};
pub use scanner::SkillScanner;
pub use validator::{SkillValidationIssue, SkillValidationReport, validate_skill_dir};
pub use versioning::{SkillVersionEntry, SkillVersionManifest, get_skill_version_manifest};
pub use watcher::{SkillWatcher, WatcherError};

#[cfg(feature = "python-bindings")]
pub use feedback::{py_get_skill_feedback, py_record_skill_feedback};
#[cfg(feature = "python-bindings")]
pub use gui_executable::{PyGuiExecutableHint, py_correct_python_executable, py_is_gui_executable};
#[cfg(feature = "python-bindings")]
pub use loader::{
    py_parse_skill_md, py_scan_and_load, py_scan_and_load_lenient, py_scan_and_load_strict,
    py_scan_and_load_team, py_scan_and_load_team_lenient, py_scan_and_load_user,
    py_scan_and_load_user_lenient,
};
#[cfg(feature = "python-bindings")]
pub use paths::{
    py_copy_skill_to_team_dir, py_copy_skill_to_user_dir, py_get_app_skill_paths_from_env,
    py_get_app_team_skill_paths_from_env, py_get_app_user_skill_paths_from_env,
    py_get_skill_paths_from_env, py_get_skills_dir, py_get_team_skill_paths_from_env,
    py_get_team_skills_dir, py_get_user_skill_paths_from_env, py_get_user_skills_dir,
};
#[cfg(feature = "python-bindings")]
pub use resolver::{
    py_expand_transitive_dependencies, py_resolve_dependencies, py_validate_dependencies,
};
#[cfg(feature = "python-bindings")]
pub use scanner::py_scan_skill_paths;
#[cfg(feature = "python-bindings")]
pub use validator::{PySkillValidationIssue, PySkillValidationReport, py_validate_skill};
#[cfg(feature = "python-bindings")]
pub use versioning::py_get_skill_version_manifest;
#[cfg(feature = "python-bindings")]
pub use watcher::PySkillWatcher;
