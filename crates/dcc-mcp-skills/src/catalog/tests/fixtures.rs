//! Shared test helpers for catalog tests.
//!
//! Contains factory helpers for `SkillCatalog`, `SkillMetadata`, and
//! `SkillEntry` that are reused across multiple test files.

use super::*;
use dcc_mcp_models::ToolDeclaration;

/// Build a minimal, empty catalog backed by a fresh `ActionRegistry`.
pub fn make_test_catalog() -> SkillCatalog {
    let registry = Arc::new(ActionRegistry::new());
    SkillCatalog::new(registry)
}

/// Build a `SkillMetadata` with the given name, DCC, and tool names.
///
/// Version is set to `"1.0.0"` and tags to `["test"]` for easy
/// assertions; all other fields use `Default`.
pub fn make_test_skill(name: &str, dcc: &str, tool_names: &[&str]) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Test skill: {name}"),
        tools: tool_names
            .iter()
            .map(|t| ToolDeclaration {
                name: t.to_string(),
                ..Default::default()
            })
            .collect(),
        dcc: dcc.to_string(),
        tags: vec!["test".to_string()],
        version: "1.0.0".to_string(),
        ..Default::default()
    }
}

/// Like [`make_test_skill`] but also sets `search_hint`.
pub fn make_test_skill_with_hint(
    name: &str,
    dcc: &str,
    hint: &str,
    tool_names: &[&str],
) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Test skill: {name}"),
        search_hint: hint.to_string(),
        tools: tool_names
            .iter()
            .map(|t| ToolDeclaration {
                name: t.to_string(),
                ..Default::default()
            })
            .collect(),
        dcc: dcc.to_string(),
        tags: vec!["test".to_string()],
        version: "1.0.0".to_string(),
        ..Default::default()
    }
}

/// Build a catalog that also owns an `ActionDispatcher`.
pub fn make_catalog_with_dispatcher() -> (SkillCatalog, Arc<ActionDispatcher>) {
    let registry = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    let catalog = SkillCatalog::new_with_dispatcher(registry, dispatcher.clone());
    (catalog, dispatcher)
}

/// Insert a skill at an explicit [`SkillScope`].
///
/// `add_skill` always tags skills as `Repo`; to exercise scope filtering
/// we reach past that constructor and inject the entry directly.
pub fn add_skill_with_scope(catalog: &SkillCatalog, meta: SkillMetadata, scope: SkillScope) {
    catalog.entries.insert(
        meta.name.clone(),
        SkillEntry {
            metadata: meta,
            state: SkillState::Discovered,
            registered_tools: Vec::new(),
            scope,
        },
    );
}
