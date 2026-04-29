use super::*;

pub(super) mod fixtures;
mod test_catalog_crud;
mod test_dispatcher;
mod test_execute_script;
mod test_execute_script_env;
mod test_execute_script_real;
mod test_resolve_tool_script;
mod test_search_hint;
mod test_search_skills;

// ── Registry<SkillEntry> contract test ───────────────────────────────────────

#[test]
fn skill_catalog_satisfies_registry_contract() {
    use dcc_mcp_models::registry::testing::assert_registry_contract;
    use fixtures::{make_test_catalog, make_test_skill};

    let sample_entry = SkillEntry {
        metadata: make_test_skill("contract_skill", "maya", &["tool_a"]),
        state: SkillState::Discovered,
        registered_tools: Vec::new(),
        scope: SkillScope::Repo,
    };
    assert_registry_contract(make_test_catalog, sample_entry);
}
