use super::*;

#[test]
fn call_reads_arguments_from_json_file() {
    use std::io::Write;

    let mut file = tempfile::NamedTempFile::new().expect("create temp JSON file");
    write!(file, r#"{{"source":"{}"}}"#, "x".repeat(40_000)).expect("write JSON file");
    let value = read_call_arguments("{}", Some(file.path())).expect("read call arguments");

    assert_eq!(value["source"].as_str().map(str::len), Some(40_000));
}

#[test]
fn call_json_file_flag_parses_without_inline_json() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "call",
        "godot_project__write_script",
        "--json-file",
        "payload.json",
    ])
    .expect("parse --json-file");
    let Command::Call { json_file, .. } = args.command else {
        panic!("expected call command");
    };
    assert_eq!(json_file, Some(PathBuf::from("payload.json")));
}

#[test]
fn gateway_endpoint_for_command_ensures_gateway_for_agent_control_commands() {
    let local = GatewayTarget::Local;
    let remote = GatewayTarget::Remote {
        name: "pcA".to_string(),
        endpoint: Endpoint::new(DEFAULT_BASE_URL),
    };
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Smoke {
                url: None,
                query: "sphere".to_string(),
                limit: 5,
                timeout_secs: 5,
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Smoke {
                url: Some("http://127.0.0.1:8765/mcp".to_string()),
                query: "sphere".to_string(),
                limit: 5,
                timeout_secs: 5,
            },
            &local,
        )
        .is_none()
    );
    assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::Health, &local).is_some());
    assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::List, &local).is_some());
    assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::List, &remote).is_some());
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Search {
                query: Some("sphere".to_string()),
                dcc_type: None,
                instance_id: None,
                limit: None,
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Describe {
                tool_slug: "maya.abc12345.create_sphere".to_string(),
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::LoadSkill {
                skill_name: Some("maya-modeling".to_string()),
                dcc_type: Some("maya".to_string()),
                dcc: None,
                instance_id: Some("abc12345".to_string()),
                activate_groups: None,
                request_json: None,
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Call {
                tool_slug: "maya.abc12345.create_sphere".to_string(),
                dcc_type: None,
                instance_id: None,
                arguments_json: "{}".to_string(),
                json_file: None,
                meta_json: None,
                timeout_secs: 30,
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::WaitReady {
                dcc_type: Some("maya".to_string()),
                instance_id: Some("abc12345".to_string()),
                require: vec!["process".to_string(), "dispatcher".to_string()],
                timeout_secs: 30,
                interval_secs: 1,
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::ReloadSkills {
                dcc_type: Some("maya".to_string()),
                instance_id: Some("abc12345".to_string()),
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::StopInstance {
                dcc_type: "maya".to_string(),
                instance_id: "abc12345".to_string(),
                expected_owner: Some("release-smoke-test".to_string()),
                expected_session: Some("test".to_string()),
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Search {
                query: Some("sphere".to_string()),
                dcc_type: None,
                instance_id: None,
                limit: None,
            },
            &remote,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::ReloadSkills {
                dcc_type: Some("maya".to_string()),
                instance_id: Some("abc12345".to_string()),
            },
            &remote,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Update {
                action: UpdateAction::Check {
                    binary: Some("dcc-mcp-server".to_string()),
                    current_version: Some("0.0.0".to_string()),
                },
            },
            &local,
        )
        .is_some()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Doctor {
                registry_dir: None,
                gateway_host: "127.0.0.1".to_string(),
                gateway_port: 9765,
            },
            &local,
        )
        .is_none()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Marketplace {
                action: MarketplaceAction::List,
            },
            &local,
        )
        .is_none()
    );
    assert!(
        gateway_endpoint_for_command(
            DEFAULT_BASE_URL,
            &Command::Gateway {
                action: Some(GatewayAction::Status(GatewayStatusArgs {
                    host: "127.0.0.1".to_string(),
                    port: 9765,
                    registry_dir: None,
                })),
                daemon: default_gateway_daemon_args(),
            },
            &local,
        )
        .is_none()
    );
}

#[test]
fn call_parses_configurable_request_timeout() {
    let args = Args::parse_from([
        "dcc-mcp-cli",
        "call",
        "blender.abc12345.render",
        "--timeout-secs",
        "120",
    ]);

    let Command::Call { timeout_secs, .. } = args.command else {
        panic!("expected call command");
    };
    assert_eq!(timeout_secs, 120);
}

#[test]
fn gateway_daemon_start_defaults_to_persistent_daemon() {
    let args = Args::parse_from(["dcc-mcp-cli", "gateway", "daemon", "start"]);

    let Command::Gateway {
        action:
            Some(GatewayAction::Daemon {
                action: GatewayDaemonAction::Start(start),
            }),
        ..
    } = args.command
    else {
        panic!("expected gateway daemon start");
    };

    assert_eq!(start.gateway_idle_timeout_secs, 0);
}

#[test]
fn gateway_daemon_restart_defaults_to_persistent_daemon() {
    let args = Args::parse_from(["dcc-mcp-cli", "gateway", "daemon", "restart"]);

    let Command::Gateway {
        action:
            Some(GatewayAction::Daemon {
                action: GatewayDaemonAction::Restart(restart),
            }),
        ..
    } = args.command
    else {
        panic!("expected gateway daemon restart");
    };

    assert_eq!(restart.start.gateway_idle_timeout_secs, 0);
    assert_eq!(restart.stop_timeout_secs, 10);
}

fn default_gateway_daemon_args() -> dcc_mcp_sidecar::gateway_daemon::GatewayArgs {
    dcc_mcp_sidecar::gateway_daemon::GatewayArgs {
        host: "127.0.0.1".to_string(),
        port: 9765,
        name: None,
        remote_host: "0.0.0.0".to_string(),
        remote_port: 59765,
        registry_dir: None,
        no_admin: false,
        admin_path: "/admin".to_string(),
        stale_timeout_secs: 30,
        relay_sources: Vec::new(),
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
        semantic_search_enabled: false,
        daemon: false,
        pidfile: None,
        restart: false,
    }
}
