mod support;

use serde_json::Value;
use tempfile::TempDir;

use support::run_json_with_env;

const DECISION_SCHEMA: &str =
    include_str!("../../../contracts/dcc-discovery-decision-v1.schema.json");
const GAME_ENGINE_FIXTURES: &str =
    include_str!("../../../tests/fixtures/discovery-decision/game-engines.json");

#[test]
fn game_engine_decisions_match_v1_schema_and_preserve_unknowns() {
    let schema: Value = serde_json::from_str(DECISION_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let fixtures: Value = serde_json::from_str(GAME_ENGINE_FIXTURES).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();

    assert_eq!(cases.len(), 4);
    for case in cases {
        let registry = TempDir::new().unwrap();
        let registry_s = registry.path().to_string_lossy().to_string();
        let dcc_type = case["dcc_type"].as_str().unwrap();
        let decision = run_json_with_env(
            &["--output", "json", "dcc-types", "--dcc-type", dcc_type],
            &[("DCC_MCP_REGISTRY_DIR", registry_s.as_str())],
        );
        validator
            .validate(&decision)
            .unwrap_or_else(|error| panic!("{}: {error}", case["id"]));
        assert_eq!(decision["live_instances"], 0);
        assert_eq!(decision["public_adapter"], "present");
        assert_eq!(decision["released_catalog"], "present");
        assert_eq!(decision["package_installation"], "unknown");
        assert_eq!(decision["project_bootstrap"], "unknown");
        assert_eq!(decision["registry_registration"], "absent");
        assert_eq!(decision["exact_instance_call"], "not_run");
        assert_eq!(decision["real_host_effect"], "not_verified");
        assert_eq!(
            decision["next_action"]["instructions_url"],
            case["expected_instructions_url"]
        );
        assert!(
            decision["uncertainties"]
                .as_array()
                .unwrap()
                .contains(&Value::String("real_host".to_string()))
        );
        assert!(
            !decision
                .to_string()
                .to_ascii_lowercase()
                .contains("unsupported")
        );
    }

    let unreal = cases
        .iter()
        .find(|case| case["id"] == "unreal-5.6")
        .unwrap();
    assert_eq!(unreal["context"]["legacy_remote_execution_required"], false);
    assert_eq!(
        unreal["context"]["selected_bootstrap_path"],
        "engine_bundled_python"
    );
    assert_eq!(unreal["context"]["optional_native_bridge_required"], false);

    let tuanjie = cases
        .iter()
        .find(|case| case["id"] == "unity-tuanjie")
        .unwrap();
    assert_eq!(tuanjie["context"]["reported_version"], "2022.3.48t1");

    let godot = cases
        .iter()
        .find(|case| case["id"] == "godot-project")
        .unwrap();
    assert_eq!(godot["context"]["machine_wide_host_detected"], false);
}
