//! Registry resolution must follow the command's local-state requirements.

mod support;

use serde_json::json;
use support::*;
use tempfile::TempDir;

#[test]
fn remote_control_does_not_require_user_home() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    let profiles = config.path().join("gateway-profiles.json");
    std::fs::write(
        &profiles,
        json!({"profiles": {"remote": {"base_url": fixture.base_url}}}).to_string(),
    )
    .unwrap();
    let envs = [
        (
            if cfg!(windows) { "USERPROFILE" } else { "HOME" },
            "relative-home",
        ),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles.to_str().unwrap()),
    ];
    for target in [
        ["--base-url", fixture.base_url.as_str()],
        ["--gateway", "remote"],
    ] {
        for (args, field, expected) in [
            (vec!["list"], "total", json!(1)),
            (
                vec!["call", "maya.abc12345.create_sphere", "--json", "{}"],
                "control_route",
                json!("gateway"),
            ),
            (
                vec![
                    "stop-instance",
                    "--dcc-type",
                    "maya",
                    "--instance-id",
                    "abc12345",
                ],
                "stopping",
                json!(true),
            ),
        ] {
            let mut command = vec!["--no-update-maintenance", "--no-auto-gateway"];
            command.extend(target);
            command.extend(args);
            let result = run_json_with_env_removed(
                &command,
                &envs,
                &[
                    "DCC_MCP_REGISTRY_DIR",
                    "DCC_MCP_GATEWAY_PROFILE",
                    "DCC_MCP_BASE_URL",
                ],
            );
            assert_eq!(result[field], expected);
        }
    }
}

#[test]
fn local_inventory_and_operation_stop_still_require_registry() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    for args in [
        vec!["list"],
        vec![
            "--base-url",
            &fixture.base_url,
            "stop-instance",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--operation-id",
            "owned-operation",
        ],
    ] {
        let output = cli_command()
            .args(["--no-update-maintenance", "--no-auto-gateway"])
            .args(args)
            .env_remove("DCC_MCP_REGISTRY_DIR")
            .env_remove("DCC_MCP_GATEWAY_PROFILE")
            .env_remove("DCC_MCP_BASE_URL")
            .env(
                if cfg!(windows) { "USERPROFILE" } else { "HOME" },
                "relative-home",
            )
            .env(
                "DCC_MCP_GATEWAY_PROFILES_FILE",
                config.path().join("profiles.json"),
            )
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("set DCC_MCP_REGISTRY_DIR explicitly")
        );
    }
}
