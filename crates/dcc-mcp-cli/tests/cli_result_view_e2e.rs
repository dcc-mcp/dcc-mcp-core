mod support;

use serde_json::Value;
use support::*;

#[test]
fn canonical_raw_and_selected_views_preserve_process_failure_and_evidence() {
    let fixture = spawn_gateway_fixture();
    let root = tempfile::TempDir::new().unwrap();
    for failed in [false, true] {
        let mut views = Vec::new();
        for flags in [
            vec![],
            vec!["--raw-result"],
            vec!["--select", "/result/structuredContent/context"],
        ] {
            let output = cli_command()
                .args([
                    "--base-url",
                    &fixture.base_url,
                    "--output",
                    "json",
                    "--no-auto-gateway",
                    "--quiet",
                    "call",
                    "houdini.abc12345.result_view",
                    "--json",
                    if failed { "{\"fail\":true}" } else { "{}" },
                ])
                .args(flags)
                .env("DCC_MCP_REGISTRY_DIR", root.path())
                .env(
                    "DCC_MCP_GATEWAY_PROFILES_FILE",
                    root.path().join("profiles.json"),
                )
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(if failed { 1 } else { 0 }));
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["success"], !failed);
            assert!(value.get("cli_update").is_none());
            assert!(value.get("marketplace_updates").is_none());
            if failed {
                let error: Value = serde_json::from_slice(&output.stderr).unwrap();
                assert_eq!(error["error"]["message"], "result view fixture");
                assert_eq!(error["error"]["details"]["reason"], "fixture_reason");
            } else {
                assert!(output.stderr.is_empty());
            }
            views.push(value);
        }
        let payload = &views[0]["result"]["structuredContent"];
        assert_eq!(views[0]["result"]["content"], serde_json::json!([]));
        assert_eq!(views[1]["result"]["structuredContent"], *payload);
        let mirrored: Value =
            serde_json::from_str(views[1]["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(mirrored, *payload);
        assert_eq!(
            views[2]["selected"]["/result/structuredContent/context"],
            payload["context"]
        );
        assert_eq!(payload["context"]["flotation_status"], "not_evaluated");
        assert_eq!(payload["context"]["flotation_verified"], false);
    }
}
