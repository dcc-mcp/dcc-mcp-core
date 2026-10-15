//! Exercise the parser used by the real CLI entry point in isolated processes.

use super::cli_args::{GatewayAction, GatewayDaemonAction};
use super::{Args, Command};
use dcc_mcp_sidecar::gateway_daemon::build_gateway_config;

const CASE_ENV: &str = "DCC_MCP_TEST_CLI_GATEWAY_PRECEDENCE_CASE";

#[test]
fn gateway_parser_preserves_boolean_precedence() {
    let Ok(case) = std::env::var(CASE_ENV) else {
        for case in ["explicit_false", "explicit_true", "env_only", "defaults"] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args([
                "--exact",
                "presentation::cli::gateway_precedence_tests::gateway_parser_preserves_boolean_precedence",
                "--nocapture",
            ]);
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("DCC_MCP_") {
                    command.env_remove(key);
                }
            }
            command.env(CASE_ENV, case);
            if case != "defaults" {
                let value = if case == "explicit_true" { "0" } else { "1" };
                for key in [
                    "DCC_MCP_GATEWAY_PERSIST",
                    "DCC_MCP_NO_ADMIN",
                    "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
                ] {
                    command.env(key, value);
                }
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{case}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .contains(&format!("CLI gateway case passed: {case}")),
                "the subprocess must run the selected case",
            );
        }
        return;
    };

    // Both normal startup and restart dispatch use the same annotated parser.
    for restart in [false, true] {
        let mut argv = vec!["dcc-mcp-cli".to_string(), "gateway".to_string()];
        if restart {
            argv.push("--restart".to_string());
        }
        if case.starts_with("explicit_") {
            let value = if case == "explicit_true" {
                "true"
            } else {
                "false"
            };
            for flag in ["gateway-persist", "no-admin", "semantic-search-enabled"] {
                argv.push(format!("--{flag}={value}"));
            }
        }
        let args = Args::try_parse_annotated_from(argv).unwrap();
        let Command::Gateway {
            action: None,
            daemon,
        } = args.command
        else {
            panic!("expected direct gateway startup");
        };
        let expected = matches!(case.as_str(), "explicit_true" | "env_only");
        let config = build_gateway_config(&daemon, "CLI precedence test");
        assert_eq!(daemon.restart, restart);
        assert_eq!(config.gateway_persist, expected);
        assert_eq!(config.admin_enabled, !expected);
        assert_eq!(config.semantic_search_enabled, expected);
    }

    // The same `gateway` parser also owns lifecycle subcommands. Annotating
    // its flattened daemon flags must not consume or misroute those commands.
    let args =
        Args::try_parse_annotated_from(["dcc-mcp-cli", "gateway", "daemon", "start"]).unwrap();
    assert!(matches!(
        args.command,
        Command::Gateway {
            action: Some(GatewayAction::Daemon {
                action: GatewayDaemonAction::Start(_)
            }),
            ..
        }
    ));
    let args =
        Args::try_parse_annotated_from(["dcc-mcp-cli", "gateway", "daemon", "restart"]).unwrap();
    assert!(matches!(
        args.command,
        Command::Gateway {
            action: Some(GatewayAction::Daemon {
                action: GatewayDaemonAction::Restart(_)
            }),
            ..
        }
    ));
    println!("CLI gateway case passed: {case}");
}
