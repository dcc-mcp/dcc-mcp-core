//! Round-trip the production restart argv through a fresh nested clap parser.
//! Conflicting environment values live only in a subprocess, so these tests
//! never mutate the parallel test runner's environment.

use super::{GatewayArgs, build_gateway_config, restart_child_args};
use clap::{CommandFactory as _, FromArgMatches as _};
use std::ffi::OsString;
use std::process::Command;

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    command: Subcommand,
}

#[derive(clap::Subcommand)]
enum Subcommand {
    Gateway(GatewayArgs),
}

fn parse(argv: Vec<OsString>) -> GatewayArgs {
    let matches = Cli::command().try_get_matches_from(argv).unwrap();
    let Subcommand::Gateway(mut args) = Cli::from_arg_matches(&matches).unwrap().command;
    args.note_value_sources(matches.subcommand_matches("gateway").unwrap());
    args
}

const CASE_ENV: &str = "DCC_MCP_TEST_RESTART_ARGV_CASE";

#[test]
fn restart_arguments_round_trip() {
    let Ok(case) = std::env::var(CASE_ENV) else {
        for case in ["explicit_false", "explicit_true", "env_only", "defaults"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args([
                "--exact",
                "gateway_daemon::restart_tests::restart_arguments_round_trip",
                "--nocapture",
            ]);
            // Remove inherited gateway settings without touching this process.
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("DCC_MCP_") {
                    command.env_remove(key);
                }
            }
            command.env(CASE_ENV, case);
            if case != "defaults" {
                let env_value = if case == "explicit_true" { "0" } else { "1" };
                for key in [
                    "DCC_MCP_GATEWAY_PERSIST",
                    "DCC_MCP_NO_ADMIN",
                    "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
                    "DCC_MCP_DISCOVER_MDNS",
                ] {
                    command.env(key, env_value);
                }
                command.env("DCC_MCP_ADMIN_PATH", "/inherited-admin");
                command.env("DCC_MCP_STALE_TIMEOUT", "90");
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
                    .contains(&format!("restart argv case passed: {case}")),
                "the subprocess must run the selected case, not silently match zero tests",
            );
        }
        return;
    };

    let mut argv: Vec<OsString> = ["dcc-mcp-server", "gateway"].map(Into::into).to_vec();
    if case.starts_with("explicit_") {
        let value = if case == "explicit_true" {
            "true"
        } else {
            "false"
        };
        for flag in ["gateway-persist", "no-admin", "semantic-search-enabled"] {
            argv.push(format!("--{flag}={value}").into());
        }
        #[cfg(feature = "mdns")]
        argv.push(format!("--discover-mdns={value}").into());
        argv.extend(
            [
                "--host",
                "127.0.0.2",
                "--port",
                "19765",
                "--name",
                "restart gateway",
                "--remote-host",
                "127.0.0.3",
                "--remote-port",
                "0",
                "--registry-dir",
                "registry with spaces",
                "--admin-path",
                "/admin",
                "--stale-timeout-secs",
                "30",
                "--gateway-idle-timeout-secs",
                "30",
                "--daemon",
                "--pidfile",
                "gateway with spaces.pid",
                "--restart",
                "--relay-source",
                "http://127.0.0.1:19872=http://127.0.0.1:19873",
                "--relay-source",
                "http://127.0.0.1:19874=http://127.0.0.1:19875",
            ]
            .map(OsString::from),
        );
    }
    let parent = parse(argv);
    let child_argv = restart_child_args(&parent);
    assert!(!child_argv.iter().any(|arg| arg == "--restart"));
    let child = parse(
        std::iter::once(OsString::from("dcc-mcp-server"))
            .chain(child_argv)
            .collect(),
    );
    let before = build_gateway_config(&parent, "restart-test");
    let after = build_gateway_config(&child, "restart-test");
    let expected = case == "explicit_true" || case == "env_only";
    assert_eq!(after.gateway_persist, expected);
    assert_eq!(after.admin_enabled, !expected);
    assert_eq!(after.semantic_search_enabled, expected);
    assert_eq!(after.gateway_persist, before.gateway_persist);
    assert_eq!(after.admin_enabled, before.admin_enabled);
    assert_eq!(
        after.semantic_search_enabled,
        before.semantic_search_enabled
    );
    assert_eq!(after.host, before.host);
    assert_eq!(after.gateway_port, before.gateway_port);
    assert_eq!(after.remote_host, before.remote_host);
    assert_eq!(after.remote_gateway_port, before.remote_gateway_port);
    assert_eq!(after.registry_dir, before.registry_dir);
    assert_eq!(after.admin_path, before.admin_path);
    assert_eq!(after.stale_timeout_secs, before.stale_timeout_secs);
    assert_eq!(
        after.gateway_idle_timeout_secs,
        before.gateway_idle_timeout_secs
    );
    assert_eq!(
        child.gateway_idle_timeout_secs,
        parent.gateway_idle_timeout_secs
    );
    assert_eq!(child.name, parent.name);
    assert_eq!(child.pidfile, parent.pidfile);
    assert_eq!(child.relay_sources.len(), parent.relay_sources.len());
    for (child, parent) in child.relay_sources.iter().zip(&parent.relay_sources) {
        assert_eq!(child.0.admin_url, parent.0.admin_url);
        assert_eq!(child.0.public_base_url, parent.0.public_base_url);
    }
    assert!(child.daemon);
    assert!(!child.restart);
    #[cfg(feature = "mdns")]
    {
        assert_eq!(after.discover_mdns, expected);
        assert_eq!(after.discover_mdns, before.discover_mdns);
    }
    println!("restart argv case passed: {case}");
}
