use super::*;

#[test]
fn fixed_local_component_command_disables_all_automatic_maintenance() {
    let args = Args::try_parse_annotated_from([
        "dcc-mcp-cli",
        "components",
        "install-verified",
        "dcc-cua",
        "--target-core-cli",
        "installed/dcc-mcp-cli",
        "--expected-core-cli-sha256",
        "core-hash",
        "--manifest",
        "new.json",
        "--manifest-sha256",
        "new-manifest-hash",
        "--previous-manifest",
        "old.json",
        "--previous-manifest-sha256",
        "old-manifest-hash",
        "--source-root",
        "qualified",
        "--version",
        "1.11.1",
        "--expected-current-sha256",
        "old-hash",
        "--expected-new-sha256",
        "new-hash",
        "--yes",
    ])
    .unwrap();
    assert!(args.no_update_maintenance);
    assert!(args.no_auto_gateway);
    assert!(matches!(
        args.command,
        Command::Components {
            action: super::super::components_cmd::ComponentsAction::InstallVerified(_)
        }
    ));
}
