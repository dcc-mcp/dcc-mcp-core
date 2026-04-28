use super::*;

mod batch_unregister;
mod concurrency;
pub(super) mod fixtures;
mod happy_path;
mod search;
mod serialization;

// ── Registry<ActionMeta> contract test ───────────────────────────────────────

#[test]
fn action_registry_satisfies_registry_contract() {
    use dcc_mcp_models::registry::testing::assert_registry_contract;
    let sample = fixtures::make_action("contract_test_action", "maya");
    assert_registry_contract(ActionRegistry::new, sample);
}
