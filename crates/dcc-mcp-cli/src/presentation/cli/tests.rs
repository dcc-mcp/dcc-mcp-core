use super::*;

#[test]
fn require_gateway_is_a_global_fail_closed_control_flag() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "call",
        "maya.abc12345.inspect",
        "--require-gateway",
        "--agent-session-id",
        "task-42",
    ])
    .expect("parse --require-gateway after the subcommand");

    assert!(args.require_gateway);
    assert_eq!(args.agent_session_id.as_deref(), Some("task-42"));
}

#[test]
fn dcc_types_contract_accepts_a_custom_catalog() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "dcc-types",
        "--catalog",
        "studio-catalog.yml",
    ])
    .expect("parse dcc-types command");

    let Command::DccTypes { catalog } = args.command else {
        panic!("expected dcc-types command");
    };
    assert_eq!(catalog, Some(PathBuf::from("studio-catalog.yml")));
}

#[test]
fn search_contract_accepts_unquoted_positional_query_words() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "search",
        "create",
        "sphere",
        "--dcc-type",
        "maya",
    ])
    .expect("parse positional search query");

    let Command::Search {
        query,
        query_terms,
        dcc_type,
        ..
    } = args.command
    else {
        panic!("expected search command");
    };
    assert!(query.is_none());
    assert_eq!(
        resolve_query(query, query_terms).as_deref(),
        Some("create sphere")
    );
    assert_eq!(dcc_type.as_deref(), Some("maya"));
}

#[test]
fn marketplace_search_contract_accepts_positional_query_and_dcc_type_alias() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "marketplace",
        "search",
        "maya",
        "rigging",
        "--dcc-type",
        "maya",
    ])
    .expect("parse positional marketplace query");

    let Command::Marketplace {
        action:
            MarketplaceAction::Search {
                query,
                query_terms,
                dcc,
                ..
            },
    } = args.command
    else {
        panic!("expected marketplace search command");
    };
    assert_eq!(
        resolve_query(query, query_terms).as_deref(),
        Some("maya rigging")
    );
    assert_eq!(dcc.as_deref(), Some("maya"));
}

#[test]
fn stats_contract_parses_composable_runtime_filters() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "stats",
        "--range",
        "7d",
        "--dcc-type",
        "houdini",
        "--skill",
        "houdini-render",
        "--tool",
        "render_rop",
        "--status",
        "failure",
        "--instance-id",
        "instance-a",
        "--session-id",
        "solar-session",
    ])
    .expect("parse stats filters");

    let Command::Stats {
        range,
        dcc_type,
        skill,
        tool,
        status,
        instance_id,
        session_id,
    } = args.command
    else {
        panic!("expected stats command");
    };
    assert_eq!(range, "7d");
    assert_eq!(dcc_type.as_deref(), Some("houdini"));
    assert_eq!(skill.as_deref(), Some("houdini-render"));
    assert_eq!(tool.as_deref(), Some("render_rop"));
    assert_eq!(status.as_deref(), Some("failure"));
    assert_eq!(instance_id.as_deref(), Some("instance-a"));
    assert_eq!(session_id.as_deref(), Some("solar-session"));
}

#[test]
fn call_batch_contract_parses_steps_and_timeout() {
    let steps = r#"[{"tool_slug":"maya.abc.capture","arguments":{}}]"#;
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "call",
        "--batch",
        "--steps",
        steps,
        "--timeout-secs",
        "45",
    ])
    .unwrap();

    let Command::Call {
        tool_slug,
        batch,
        steps: parsed_steps,
        timeout_secs,
        ..
    } = args.command
    else {
        panic!("expected call command");
    };
    assert!(tool_slug.is_none());
    assert!(batch);
    assert_eq!(parsed_steps.as_deref(), Some(steps));
    assert_eq!(timeout_secs, 45);
}

#[test]
fn call_batch_compatibility_alias_still_parses() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "call-batch",
        "--json",
        r#"{"calls":[{"tool_slug":"maya.abc.capture","arguments":{}}]}"#,
    ])
    .unwrap();
    assert!(matches!(args.command, Command::CallBatch { .. }));
}

#[test]
fn ui_control_contract_parses_a_stable_snapshot_command() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "ui-control",
        "snapshot",
        "--dcc-type",
        "unreal",
        "--instance-id",
        "abc12345",
        "--json",
        r#"{"session_id":"menu","process_id":42}"#,
        "--timeout-secs",
        "12",
    ])
    .expect("parse ui-control snapshot");

    let Command::UiControl {
        action: UiControlAction::Snapshot(snapshot),
    } = args.command
    else {
        panic!("expected ui-control snapshot command");
    };
    assert_eq!(snapshot.dcc_type.as_deref(), Some("unreal"));
    assert_eq!(snapshot.instance_id.as_deref(), Some("abc12345"));
    assert_eq!(snapshot.timeout_secs, 12);
    assert!(!snapshot.full_output);
    assert_eq!(
        read_call_arguments(&snapshot.arguments_json, snapshot.json_file.as_deref()).unwrap(),
        serde_json::json!({"session_id": "menu", "process_id": 42})
    );
}

#[test]
fn ui_control_contract_parses_a_stable_system_operation_command() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "ui-control",
        "system-operation",
        "--instance-id",
        "abc12345",
        "--json",
        r#"{"operation_id":"link-vendor-plugin"}"#,
        "--full-output",
    ])
    .expect("parse ui-control system-operation");

    let Command::UiControl {
        action: UiControlAction::SystemOperation(operation),
    } = args.command
    else {
        panic!("expected ui-control system-operation command");
    };
    assert_eq!(operation.instance_id.as_deref(), Some("abc12345"));
    assert!(operation.full_output);
    assert_eq!(
        read_call_arguments(&operation.arguments_json, operation.json_file.as_deref()).unwrap(),
        serde_json::json!({"operation_id": "link-vendor-plugin"})
    );
}

#[test]
fn ui_control_operations_map_to_canonical_ui_control_tools() {
    let args = UiControlArgs {
        dcc_type: None,
        instance_id: None,
        arguments_json: "{}".to_string(),
        json_file: None,
        meta_json: None,
        timeout_secs: 30,
        full_output: false,
    };
    for (action, expected) in [
        (
            UiControlAction::Snapshot(args.clone()),
            "ui_control__snapshot",
        ),
        (UiControlAction::Find(args.clone()), "ui_control__find"),
        (UiControlAction::Act(args.clone()), "ui_control__act"),
        (
            UiControlAction::SystemOperation(args.clone()),
            "ui_control__system_operation",
        ),
        (UiControlAction::Wait(args.clone()), "ui_control__wait_for"),
        (
            UiControlAction::Stop(args.clone()),
            "ui_control__stop_computer_use",
        ),
    ] {
        assert_eq!(action.into_call().0, expected);
    }
}

#[test]
fn ui_control_full_output_flag_preserves_the_diagnostic_escape_hatch() {
    let args = Args::try_parse_from([
        "dcc-mcp-cli",
        "ui-control",
        "snapshot",
        "--instance-id",
        "abc12345",
        "--full-output",
    ])
    .expect("parse --full-output");

    let Command::UiControl {
        action: UiControlAction::Snapshot(snapshot),
    } = args.command
    else {
        panic!("expected ui-control snapshot command");
    };
    assert!(snapshot.full_output);
}

#[test]
fn ui_control_compact_output_keeps_agent_fields_and_drops_bulk_trees() {
    let root = tempfile::tempdir().expect("create artifact directory");
    let artifact = root.path().join("computer-use-frame.png");
    let payload = serde_json::json!({
        "success": true,
        "message": "Captured isolated Windows UI Control snapshot.",
        "prompt": "Act once, then snapshot again.",
        "error": null,
        "context": {
            "session_id": "fab",
            "snapshot_id": "snapshot-7",
            "snapshot": {
                "session_id": "fab",
                "focus_id": "search",
                "truncated": false,
                "node_count": 501,
                "root": {"id": "desktop", "children": [{"text": "bulk"}]},
                "metadata": {
                    "snapshot_id": "snapshot-7",
                    "backend": "windows-ui-control-host",
                    "computer_use": {"raw": "bulk"}
                }
            },
            "observation": {
                "observation_id": "observation-7",
                "process_id": 42,
                "width": 1280,
                "height": 720
            },
            "policy": {"allow_raw_coordinates": true},
            "audit": {"coordinates": [1, 2]},
            "__rich__": {
                "kind": "image",
                "data": "<materialized:image>",
                "mime": "image/png",
                "artifact_path": artifact
            }
        }
    });
    let value = serde_json::json!({
        "success": true,
        "tool_slug": "unreal.ui_control__snapshot",
        "dcc_type": "unreal",
        "instance_id": "abc12345",
        "arguments": {"large": "request echo"},
        "result": {"structuredContent": payload},
        "source": "local_mcp"
    });

    let compact = compact_ui_control_result("ui_control__snapshot", &value);

    assert_eq!(compact["success"], true);
    assert_eq!(compact["tool"], "ui_control__snapshot");
    assert_eq!(compact["snapshot_id"], "snapshot-7");
    assert_eq!(compact["snapshot"]["node_count"], 501);
    assert_eq!(compact["observation"]["observation_id"], "observation-7");
    assert_eq!(
        compact["__rich__"]["artifact_path"],
        artifact.display().to_string()
    );
    let encoded = serde_json::to_string(&compact).unwrap();
    assert!(!encoded.contains("request echo"));
    assert!(!encoded.contains("children"));
    assert!(!encoded.contains("allow_raw_coordinates"));
    assert!(!encoded.contains("coordinates"));
}

#[test]
fn call_materializes_rest_rich_image_without_printing_base64() {
    let root = tempfile::tempdir().expect("create artifact directory");
    let bytes = b"\x89PNG\r\n\x1a\ncomputer-use";
    let encoded = BASE64_STANDARD.encode(bytes);
    let mut value = serde_json::json!({
        "success": true,
        "output": {
            "context": {
                "__rich__": {
                    "kind": "image",
                    "data": encoded,
                    "mime": "image/png"
                }
            }
        }
    });

    materialize_call_images(&mut value, root.path());

    let rich = value.pointer("/output/context/__rich__").unwrap();
    let artifact = PathBuf::from(rich["artifact_path"].as_str().unwrap());
    assert!(artifact.is_absolute());
    assert!(artifact.starts_with(root.path()));
    assert_eq!(
        artifact.extension().and_then(|value| value.to_str()),
        Some("png")
    );
    assert_eq!(std::fs::read(artifact).unwrap(), bytes);
    assert_eq!(rich["data"], MATERIALIZED_IMAGE_PLACEHOLDER);
    assert!(!serde_json::to_string(&value).unwrap().contains(&encoded));
}

#[test]
fn call_materializes_native_mcp_image_and_reports_invalid_data_safely() {
    let root = tempfile::tempdir().expect("create artifact directory");
    let bytes = b"native image";
    let encoded = BASE64_STANDARD.encode(bytes);
    let mut value = serde_json::json!({
        "result": {
            "content": [
                {"type": "image", "data": encoded, "mimeType": "image/webp"},
                {"type": "image", "data": "%%%not-base64%%%", "mimeType": "image/png"}
            ]
        }
    });

    materialize_call_images(&mut value, root.path());

    let first = &value["result"]["content"][0];
    let artifact = PathBuf::from(first["artifact_path"].as_str().unwrap());
    assert_eq!(std::fs::read(artifact).unwrap(), bytes);
    assert_eq!(first["data"], MATERIALIZED_IMAGE_PLACEHOLDER);
    let invalid = &value["result"]["content"][1];
    assert_eq!(invalid["data"], MATERIALIZED_IMAGE_PLACEHOLDER);
    assert_eq!(
        invalid["materialization_error"],
        "invalid base64 image data"
    );
    assert!(
        !serde_json::to_string(&value)
            .unwrap()
            .contains("%%%not-base64%%%")
    );
}

#[test]
fn call_redacts_malformed_data_with_preexisting_artifact_path() {
    let root = tempfile::tempdir().expect("create artifact directory");
    let encoded = "%%%private-invalid-base64%%%";
    let existing = root.path().join("existing.png");
    let mut value = serde_json::json!({
        "output": {
            "context": {
                "__rich__": {
                    "kind": "image",
                    "data": encoded,
                    "mime": "image/png",
                    "artifact_path": existing
                }
            }
        }
    });

    materialize_call_images(&mut value, root.path());

    let rich = value.pointer("/output/context/__rich__").unwrap();
    assert_eq!(rich["data"], MATERIALIZED_IMAGE_PLACEHOLDER);
    assert_eq!(rich["artifact_path"], existing.display().to_string());
    assert_eq!(rich["materialization_error"], "invalid base64 image data");
    assert!(!serde_json::to_string(&value).unwrap().contains(encoded));
}

#[test]
fn image_artifact_pruning_removes_expired_owned_files_only() {
    let root = tempfile::tempdir().expect("create artifact directory");
    let expired = root.path().join("computer-use-expired.png");
    let protected = root.path().join("computer-use-current.png");
    let unrelated = root.path().join("artist-reference.png");
    std::fs::write(&expired, b"expired").expect("write expired artifact");
    std::fs::write(&protected, b"current").expect("write protected artifact");
    std::fs::write(&unrelated, b"reference").expect("write unrelated image");

    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(48 * 60 * 60);
    prune_image_artifacts(
        root.path(),
        future,
        std::time::Duration::from_secs(24 * 60 * 60),
        u64::MAX,
        Some(&protected),
    );

    assert!(!expired.exists());
    assert!(protected.exists());
    assert!(unrelated.exists());
}

#[test]
fn image_artifact_pruning_bounds_total_owned_size() {
    let root = tempfile::tempdir().expect("create artifact directory");
    for index in 0..3 {
        std::fs::write(
            root.path().join(format!("computer-use-{index}.png")),
            b"1234",
        )
        .expect("write image artifact");
    }

    prune_image_artifacts(
        root.path(),
        std::time::SystemTime::now() + std::time::Duration::from_secs(2 * 60),
        std::time::Duration::ZERO,
        5,
        None,
    );

    let remaining_size: u64 = std::fs::read_dir(root.path())
        .expect("read artifact directory")
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum();
    assert!(remaining_size <= 5, "remaining size was {remaining_size}");
}

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
                query_terms: Vec::new(),
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
                tool_slug: Some("maya.abc12345.create_sphere".to_string()),
                batch: false,
                steps: None,
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
                query_terms: Vec::new(),
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
            &Command::DccTypes { catalog: None },
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
