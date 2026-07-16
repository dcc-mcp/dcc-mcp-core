mod support;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use support::*;

#[test]
fn list_search_describe_and_call_gateway_rest_surface() {
    let fixture = spawn_gateway_fixture();

    let list = run_json(&["--base-url", &fixture.base_url, "list"]);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["gateway"]["current"]["name"], "Maya-main-15084");
    assert_eq!(
        list["gateway"]["candidates"][0]["name"],
        "Maya-layout-120920"
    );

    let search = run_json(&[
        "--base-url",
        &fixture.base_url,
        "search",
        "--query",
        "sphere",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
    ]);
    assert_eq!(search["hits"][0]["slug"], "maya.abc12345.create_sphere");
    assert_eq!(search["hits"][0]["instance_id"], "abc12345");

    let describe = run_json(&[
        "--base-url",
        &fixture.base_url,
        "describe",
        "maya.abc12345.create_sphere",
    ]);
    assert_eq!(
        describe["record"]["tool_slug"],
        "maya.abc12345.create_sphere"
    );

    let loaded = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "workflow",
        "--dcc-type",
        "3dsmax",
        "--instance-id",
        "80321760",
    ]);
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["skill_name"], "workflow");
    assert_eq!(loaded["dcc_type"], "3dsmax");
    assert_eq!(loaded["instance_id"], "80321760");
    assert_eq!(loaded["registered_tools"][0], "workflow__run");

    let loaded_from_json = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "--json",
        r#"{"skill_name":"workflow","dcc_type":"3dsmax","instance_id":"80321760","activate_groups":false}"#,
    ]);
    assert_eq!(loaded_from_json["loaded"], true);
    assert_eq!(loaded_from_json["activate_groups"], false);
    assert_eq!(loaded_from_json["registered_tools"][0], "workflow__run");

    let call = run_json(&[
        "--base-url",
        &fixture.base_url,
        "call",
        "maya.abc12345.create_sphere",
        "--json",
        r#"{"radius":2}"#,
    ]);
    assert_eq!(call["success"], true);
    assert_eq!(call["arguments"]["radius"], 2);

    let mut payload = NamedTempFile::new().expect("create call payload");
    serde_json::to_writer(
        payload.as_file_mut(),
        &serde_json::json!({"source": "x".repeat(40_000)}),
    )
    .expect("write call payload");
    let file_call = run_json(&[
        "--base-url",
        &fixture.base_url,
        "call",
        "maya.abc12345.create_sphere",
        "--json-file",
        payload.path().to_str().expect("UTF-8 temp path"),
    ]);
    assert_eq!(file_call["success"], true);
    assert_eq!(
        file_call["arguments"]["source"].as_str().map(str::len),
        Some(40_000)
    );

    let direct_call = run_json(&[
        "--base-url",
        &fixture.base_url,
        "call",
        "maya_scene__get_session_info",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--json",
        r#"{}"#,
    ]);
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["dcc_type"], "maya");
    assert_eq!(direct_call["instance_id"], "abc12345");
    assert_eq!(direct_call["backend_tool"], "maya_scene__get_session_info");

    let ready = run_json(&[
        "--base-url",
        &fixture.base_url,
        "wait-ready",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--require",
        "skill_catalog,host_execution_bridge",
        "--timeout-secs",
        "1",
    ]);
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["readiness_source"], "gateway_readyz");
    assert_eq!(ready["gateway_readyz_error"], Value::Null);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json(&[
        "--base-url",
        &fixture.base_url,
        "stop-instance",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--expected-owner",
        "release-smoke-test",
        "--expected-session",
        "test",
    ]);
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["stopping"], true);
    assert_eq!(stop["expected_owner"], "release-smoke-test");
}

#[test]
fn call_and_call_batch_exit_one_on_tool_failure_after_http_success() {
    let fixture = spawn_gateway_fixture();
    let single = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "call",
            "maya.abc12345.domain_failure",
            "--json",
            "{}",
        ])
        .output()
        .expect("run single tool failure");
    assert_eq!(single.status.code(), Some(1));
    let single_body: Value = serde_json::from_slice(&single.stdout).expect("single JSON output");
    assert_eq!(single_body["output"]["success"], false);
    assert_eq!(single_body["output"]["message"], "tool domain failure");

    let steps = r#"[{"tool_slug":"maya.abc12345.domain_failure","arguments":{}}]"#;
    let batch = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "call",
            "--batch",
            "--steps",
            steps,
        ])
        .output()
        .expect("run batch tool failure");
    assert_eq!(batch.status.code(), Some(1));
    let batch_body: Value = serde_json::from_slice(&batch.stdout).expect("batch JSON output");
    assert_eq!(batch_body["success"], false);
    assert_eq!(batch_body["results"][0]["ok"], false);
    assert_eq!(batch_body["results"][0]["error"]["kind"], "tool-error");
}

#[test]
fn local_list_reads_file_registry_after_gateway_ensure() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let port = local_mcp_port(&fixture);
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", port);
    entry.display_name = Some("Maya-Rig".to_string());
    entry
        .metadata
        .insert("owner".to_string(), "release-smoke-test".to_string());
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let list = run_json_with_env(&["list"], &envs);

    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["instances"][0]["display_name"], "Maya-Rig");
    assert_eq!(list["instances"][0]["mcp_url"], fixture.mcp_url());
    assert_eq!(list["gateway"]["current"]["role"], "local");
}

#[test]
fn local_list_uses_core_default_registry_without_env_override() {
    let fixture = spawn_local_mcp_fixture();
    let temp = TempDir::new().unwrap();
    let default_registry = temp.path().join("dcc-mcp-registry");
    let file_registry = FileRegistry::new(&default_registry).unwrap();
    let port = local_mcp_port(&fixture);
    let mut entry = ServiceEntry::new("photoshop", "127.0.0.1", port);
    entry.display_name = Some("Photoshop-Default-Registry".to_string());
    file_registry.register(entry).unwrap();

    let temp_s = temp.path().to_string_lossy().to_string();
    let default_registry_s = default_registry.to_string_lossy().to_string();
    let profiles = temp.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("TMP", temp_s.as_str()),
        ("TEMP", temp_s.as_str()),
        ("TMPDIR", temp_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let list = run_json_with_env_removed(
        &["list"],
        &envs,
        &[
            "DCC_MCP_REGISTRY_DIR",
            "DCC_MCP_GATEWAY_PROFILE",
            "DCC_MCP_BASE_URL",
        ],
    );

    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["registry_dir"], default_registry_s);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "photoshop");
    assert_eq!(
        list["instances"][0]["display_name"],
        "Photoshop-Default-Registry"
    );
}

#[test]
fn local_profile_controls_registered_instance_through_direct_mcp() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry.display_name = Some("Maya-Local".to_string());
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    entry
        .metadata
        .insert("owner".to_string(), "release-smoke-test".to_string());
    entry
        .metadata
        .insert("session".to_string(), "test".to_string());
    entry
        .metadata
        .insert("safe_stop_url".to_string(), fixture.safe_stop_url());
    let instance_id = entry.instance_id.to_string();
    let instance_short = entry.instance_id.simple().to_string()[..8].to_string();
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let search = run_json_with_env(&["search", "--query", "scene", "--dcc-type", "maya"], &envs);
    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["total"], 2);
    assert_eq!(
        search["hits"][0]["backend_tool"],
        "maya_scene__get_session_info"
    );
    assert_eq!(search["hits"][0]["instance_id"], instance_id);
    let slug = search["hits"][0]["slug"].as_str().unwrap();
    assert!(slug.starts_with(&format!("maya.{instance_short}.")));

    let describe = run_json_with_env(&["describe", slug], &envs);
    assert_eq!(describe["source"], "local_mcp");
    assert_eq!(describe["record"]["tool_slug"], slug);
    assert_eq!(describe["tool"]["name"], "maya_scene__get_session_info");

    let loaded = run_json_with_env(
        &[
            "load-skill",
            "workflow",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
        ],
        &envs,
    );
    assert_eq!(loaded["source"], "local_mcp");
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["registered_tools"][0], "workflow__run");

    let call = run_json_with_env(&["call", slug, "--json", r#"{"detail":true}"#], &envs);
    assert_eq!(call["source"], "local_mcp");
    assert_eq!(call["success"], true);
    assert_eq!(call["tool_slug"], slug);
    assert_eq!(call["result"]["isError"], false);

    let direct_call = run_json_with_env(
        &[
            "call",
            "workflow__run",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--json",
            r#"{"name":"demo"}"#,
        ],
        &envs,
    );
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["backend_tool"], "workflow__run");

    let reload = run_json_with_env(
        &[
            "reload-skills",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
        ],
        &envs,
    );
    assert_eq!(reload["source"], "local_mcp");
    assert_eq!(reload["reloaded"], true);
    assert_eq!(reload["count"], 1);
    assert_eq!(
        reload["results"][0]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(reload["results"][0]["reloaded"], true);

    let ready = run_json_with_env(
        &[
            "wait-ready",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--require",
            "dispatcher,host_execution_bridge",
            "--timeout-secs",
            "1",
        ],
        &envs,
    );
    assert_eq!(ready["source"], "local_mcp");
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json_with_env(
        &[
            "stop-instance",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--expected-owner",
            "release-smoke-test",
            "--expected-session",
            "test",
        ],
        &envs,
    );
    assert_eq!(stop["source"], "local_mcp");
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["response"]["accepted"], true);
}

#[test]
fn local_call_requires_owner_metadata_for_leased_instance() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    let instance_id = entry.instance_id;
    let instance_short = entry.instance_id.to_string()[..8].to_string();
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    entry.acquire_lease(
        "workflow-a",
        Some("job-a".to_string()),
        Some(std::time::SystemTime::now() + std::time::Duration::from_secs(60)),
    );
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles_s = registry
        .path()
        .join("gateway-profiles.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let slug = format!("maya.{instance_short}.maya_scene__get_session_info");

    let stderr = run_failure_with_env(&["call", &slug, "--json", "{}"], &envs);
    assert!(stderr.contains("instance-leased"), "stderr: {stderr}");

    let stderr = run_failure_with_env(
        &[
            "call",
            &slug,
            "--json",
            "{}",
            "--meta-json",
            r#"{"lease_owner":"workflow-b"}"#,
        ],
        &envs,
    );
    assert!(stderr.contains("lease-owner-mismatch"), "stderr: {stderr}");

    let call = run_json_with_env(
        &[
            "call",
            &slug,
            "--json",
            "{}",
            "--meta-json",
            r#"{"lease_owner":"workflow-a"}"#,
        ],
        &envs,
    );
    assert_eq!(call["success"], true);

    let key = dcc_mcp_transport::discovery::types::ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id,
    };
    let mut expired = file_registry.get(&key).expect("leased registry row");
    expired.acquire_lease(
        "expired-workflow",
        None,
        Some(std::time::SystemTime::now() - std::time::Duration::from_secs(1)),
    );
    file_registry.register(expired).unwrap();

    let call = run_json_with_env(&["call", &slug, "--json", "{}"], &envs);
    assert_eq!(call["success"], true);
}

#[test]
fn local_search_without_query_lists_tools_for_dcc_filter() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let search = run_json_with_env(&["search", "--dcc-type", "maya"], &envs);
    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["query"], Value::Null);
    let hit_names: Vec<&str> = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["backend_tool"].as_str())
        .collect();
    assert!(
        hit_names.contains(&"maya_scene__get_session_info"),
        "empty-query local search should list loaded tools: {search}"
    );
    assert!(
        hit_names.contains(&"workflow__run"),
        "empty-query local search should include all loaded tools: {search}"
    );
}

#[test]
fn local_describe_uses_rest_for_loaded_tool_missing_from_tools_list() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    let instance_short = entry.instance_id.to_string()[..8].to_string();
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles_s = registry
        .path()
        .join("gateway-profiles.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let slug = format!("maya.{instance_short}.dynamic__run");
    let describe = run_json_with_env(&["describe", &slug], &envs);
    assert_eq!(describe["source"], "local_mcp");
    assert_eq!(describe["tool"]["name"], "dynamic__run");
    assert_eq!(describe["tool"]["inputSchema"]["required"][0], "name");
}

#[test]
fn local_search_routes_ready_sidecar_and_skips_unavailable_rows() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();

    let mut diagnostic = ServiceEntry::new("maya", "127.0.0.1", 9);
    diagnostic.display_name = Some("Maya-Diagnostic".to_string());
    diagnostic.status = ServiceStatus::Booting;
    diagnostic
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    diagnostic
        .metadata
        .insert("failure_stage".to_string(), "gateway-health".to_string());
    diagnostic.metadata.insert(
        "failure_reason".to_string(),
        "gateway health OK before sidecar dispatch".to_string(),
    );
    diagnostic
        .metadata
        .insert("mcp_url".to_string(), "http://127.0.0.1:9/mcp".to_string());
    file_registry.register(diagnostic).unwrap();

    let mut unavailable_sidecar = ServiceEntry::new("maya", "127.0.0.1", 9);
    unavailable_sidecar.display_name = Some("Maya-Sidecar-Unavailable".to_string());
    unavailable_sidecar.status = ServiceStatus::Available;
    unavailable_sidecar
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    unavailable_sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    unavailable_sidecar
        .metadata
        .insert("failure_stage".to_string(), "host-rpc-connect".to_string());
    unavailable_sidecar.metadata.insert(
        "failure_reason".to_string(),
        "connection refused".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "host_rpc_uri".to_string(),
        "commandport://127.0.0.1:6000".to_string(),
    );
    unavailable_sidecar
        .metadata
        .insert("host_rpc_scheme".to_string(), "commandport".to_string());
    unavailable_sidecar
        .metadata
        .insert("sidecar_pid".to_string(), "4242".to_string());
    unavailable_sidecar.metadata.insert(
        "stdio_log_dir".to_string(),
        "C:/tmp/dcc-sidecar-logs".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "stdio_stdout_path".to_string(),
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stdout.log".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "stdio_stderr_path".to_string(),
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stderr.log".to_string(),
    );
    unavailable_sidecar
        .metadata
        .insert("mcp_url".to_string(), "http://127.0.0.1:9/mcp".to_string());
    file_registry.register(unavailable_sidecar).unwrap();

    let mut ready_sidecar = ServiceEntry::new("maya", "127.0.0.1", 0);
    ready_sidecar.display_name = Some("Maya-Sidecar-Ready".to_string());
    ready_sidecar
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    ready_sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    ready_sidecar
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    let ready_id = ready_sidecar.instance_id.to_string();
    file_registry.register(ready_sidecar).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];
    let list = run_json_with_env(&["list"], &envs);
    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["total"], 3);
    let instances = list["instances"].as_array().unwrap();
    assert!(
        instances
            .iter()
            .any(|instance| instance["display_name"] == "Maya-Diagnostic"),
        "local list should keep diagnostic rows visible"
    );
    assert!(
        instances
            .iter()
            .any(|instance| instance["display_name"] == "Maya-Sidecar-Unavailable"),
        "local list should keep unavailable sidecar rows visible"
    );
    let diagnostic_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Diagnostic")
        .unwrap();
    assert_eq!(diagnostic_row["direct_control"]["ready"], false);
    assert_eq!(diagnostic_row["direct_control"]["reason"], "service_status");
    assert!(
        diagnostic_row["direct_control"]["recommended_next_action"]
            .as_str()
            .unwrap()
            .contains("wait-ready")
    );
    let sidecar_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Sidecar-Unavailable")
        .unwrap();
    assert_eq!(sidecar_row["direct_control"]["ready"], false);
    assert_eq!(sidecar_row["direct_control"]["reason"], "dispatch_status");
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["failure_stage"],
        "host-rpc-connect"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["failure_reason"],
        "connection refused"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["host_rpc_uri"],
        "commandport://127.0.0.1:6000"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["logs"]["stderr_path"],
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stderr.log"
    );
    assert!(
        sidecar_row["direct_control"]["recommended_next_action"]
            .as_str()
            .unwrap()
            .contains("dispatch_status=ready")
    );
    let ready_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Sidecar-Ready")
        .unwrap();
    assert_eq!(ready_row["direct_control"]["ready"], true);
    assert_eq!(ready_row["direct_control"]["route"], "local_mcp");
    assert_eq!(
        ready_row["direct_control"]["recommended_next_action"],
        "Use this instance through the local MCP route."
    );

    let search = run_json_with_env(&["search", "--query", "scene", "--dcc-type", "maya"], &envs);
    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["total"], 2);
    assert!(
        search["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["instance_id"] == ready_id),
        "local search should only route to direct dispatch-ready instances: {search}"
    );
}

#[test]
fn gateway_profiles_select_remote_gateway_for_list() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    let profiles = config.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str())];

    let registered = run_json_with_env(
        &["gateway", "register", &fixture.base_url, "--name", "pcA"],
        &envs,
    );
    assert_eq!(registered["registered"], true);
    assert_eq!(registered["name"], "pcA");
    assert_eq!(registered["base_url"], fixture.base_url);

    let selected = run_json_with_env(&["gateway", "set", "pcA"], &envs);
    assert_eq!(selected["current"], "pcA");
    assert_eq!(selected["mode"], "remote");

    let profiles = run_json_with_env(&["gateway", "list"], &envs);
    assert_eq!(profiles["current"], "pcA");
    assert_eq!(profiles["selected"]["mode"], "remote");
    assert_eq!(profiles["selected"]["base_url"], fixture.base_url);
    assert_eq!(profiles["profiles"][0]["name"], "pcA");
    assert_eq!(profiles["profiles"][0]["base_url"], fixture.base_url);

    let list = run_json_with_env(&["list"], &envs);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["gateway"]["current"]["name"], "Maya-main-15084");

    let local = run_json_with_env(&["gateway", "set", "local"], &envs);
    assert_eq!(local["current"], "local");
    assert_eq!(local["mode"], "local");

    let env_selected = run_json_with_env_removed(
        &["list"],
        &[
            ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
            ("DCC_MCP_GATEWAY_PROFILE", "pcA"),
        ],
        &["DCC_MCP_BASE_URL"],
    );
    assert_eq!(env_selected["total"], 1);
    assert_eq!(
        env_selected["gateway"]["current"]["name"],
        "Maya-main-15084"
    );

    let overridden = run_json_with_env(&["list", "--gateway", "pcA"], &envs);
    assert_eq!(overridden["total"], 1);
    assert_eq!(overridden["gateway"]["current"]["name"], "Maya-main-15084");
}

#[test]
fn gateway_profiles_route_all_dcc_control_commands_to_remote_gateway() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    let profiles = config.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str())];

    let registered = run_json_with_env(
        &["gateway", "register", &fixture.base_url, "--name", "pcA"],
        &envs,
    );
    assert_eq!(registered["registered"], true);
    let selected = run_json_with_env(&["gateway", "set", "pcA"], &envs);
    assert_eq!(selected["mode"], "remote");

    let search = run_json_with_env(
        &[
            "search",
            "--query",
            "sphere",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(search["hits"][0]["scope"], "gateway");
    assert_eq!(search["hits"][0]["instance_id"], "abc12345");

    let describe = run_json_with_env(&["describe", "maya.abc12345.create_sphere"], &envs);
    assert_eq!(
        describe["record"]["tool_slug"],
        "maya.abc12345.create_sphere"
    );

    let loaded = run_json_with_env(
        &[
            "load-skill",
            "workflow",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["skill_name"], "workflow");
    assert_eq!(loaded["dcc_type"], "maya");
    assert_eq!(loaded["instance_id"], "abc12345");

    let call = run_json_with_env(
        &[
            "call",
            "maya.abc12345.create_sphere",
            "--json",
            r#"{"radius":2}"#,
        ],
        &envs,
    );
    assert_eq!(call["success"], true);
    assert_eq!(call["tool_slug"], "maya.abc12345.create_sphere");
    assert_eq!(call["arguments"]["radius"], 2);

    let direct_call = run_json_with_env(
        &[
            "call",
            "maya_scene__get_session_info",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--json",
            r#"{}"#,
        ],
        &envs,
    );
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["backend_tool"], "maya_scene__get_session_info");

    let reload = run_json_with_env(
        &[
            "reload-skills",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(reload["source"], "gateway");
    assert_eq!(reload["count"], 1);
    assert_eq!(
        reload["results"][0]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(
        reload["results"][0]["result"]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(
        reload["results"][0]["result"]["instance_id"],
        "abc12345-0000-0000-0000-000000000000"
    );

    let ready = run_json_with_env(
        &[
            "wait-ready",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--require",
            "skill_catalog,host_execution_bridge",
            "--timeout-secs",
            "1",
        ],
        &envs,
    );
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["readiness_source"], "gateway_readyz");
    assert_eq!(ready["gateway_readyz_error"], Value::Null);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json_with_env(
        &[
            "stop-instance",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--expected-owner",
            "release-smoke-test",
            "--expected-session",
            "test",
        ],
        &envs,
    );
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["stopping"], true);
    assert_eq!(stop["expected_owner"], "release-smoke-test");
}

#[test]
fn gateway_daemon_status_uses_formal_subcommand() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();

    let status = run_json(&[
        "gateway",
        "daemon",
        "status",
        "--host",
        "127.0.0.1",
        "--port",
        &port_s,
        "--registry-dir",
        &registry_s,
    ]);

    assert_eq!(status["healthy"], false);
    assert_eq!(status["running"], false);
    assert_eq!(status["pid"], Value::Null);
    assert_eq!(status["registry_dir"], registry_s);
    assert_eq!(
        status["pidfile"],
        registry
            .path()
            .join("gateway.pid")
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(
        status["health_url"],
        format!("http://127.0.0.1:{port}/health")
    );
    assert!(status["cli_version"].as_str().unwrap().starts_with("0."));
}

#[test]
fn doctor_reports_local_defaults_without_starting_gateway() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
        ],
        &envs,
    );

    assert_eq!(doctor["status"], "ok");
    assert_eq!(doctor["cli"]["name"], "dcc-mcp-cli");
    assert!(doctor["cli"]["version"].as_str().unwrap().starts_with("0."));
    assert_eq!(doctor["profile"]["stored_current"], "local");
    assert_eq!(doctor["profile"]["selected"]["name"], "local");
    assert_eq!(doctor["profile"]["selected"]["mode"], "local");
    assert_eq!(doctor["local"]["registry_dir"], registry_s);
    assert_eq!(doctor["local"]["inventory"]["ok"], true);
    assert_eq!(doctor["local"]["inventory"]["total"], 0);
    assert_eq!(doctor["local"]["inventory"]["direct_control"]["ready"], 0);
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["not_ready"],
        0
    );
    assert_eq!(doctor["gateway"]["auto_start_enabled"], true);
    assert_eq!(
        doctor["gateway"]["default_base_url"],
        format!("http://127.0.0.1:{port}")
    );
    assert_eq!(doctor["gateway"]["status"]["healthy"], false);
    assert_eq!(doctor["server_binary"]["status"], "ok");
    assert_eq!(doctor["server_binary"]["source"], "explicit");
    assert_eq!(doctor["server_binary"]["path"], cli_bin);
    assert_eq!(doctor["server_binary"]["would_download_if_started"], false);
    assert!(
        doctor["server_binary"]["version"]
            .as_str()
            .unwrap()
            .contains("dcc-mcp-cli")
    );
}

#[test]
fn doctor_summarizes_local_direct_control_readiness() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let file_registry = FileRegistry::new(registry.path()).unwrap();

    let mut booting = ServiceEntry::new("maya", "127.0.0.1", 18080);
    booting.status = ServiceStatus::Booting;
    booting
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    booting
        .metadata
        .insert("failure_stage".to_string(), "host-rpc-connect".to_string());
    booting.metadata.insert(
        "failure_reason".to_string(),
        "connection refused".to_string(),
    );
    booting.metadata.insert(
        "host_rpc_uri".to_string(),
        "commandport://127.0.0.1:6000".to_string(),
    );
    file_registry.register(booting).unwrap();

    let mut sidecar = ServiceEntry::new("maya", "127.0.0.1", 18081);
    sidecar
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    file_registry.register(sidecar).unwrap();

    let mut direct = ServiceEntry::new("maya", "127.0.0.1", 18082);
    direct
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    file_registry.register(direct).unwrap();

    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
        ],
        &envs,
    );

    assert_eq!(doctor["local"]["inventory"]["ok"], true);
    assert_eq!(doctor["local"]["inventory"]["total"], 3);
    assert_eq!(doctor["local"]["inventory"]["direct_control"]["ready"], 2);
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["not_ready"],
        1
    );
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["reasons"]["service_status"],
        1
    );
    let not_ready = doctor["local"]["inventory"]["direct_control"]["not_ready_instances"]
        .as_array()
        .unwrap();
    assert_eq!(not_ready.len(), 1);
    assert_eq!(not_ready[0]["reason"], "service_status");
    assert_eq!(
        not_ready[0]["diagnostics"]["failure_stage"],
        "host-rpc-connect"
    );
    assert_eq!(
        not_ready[0]["diagnostics"]["failure_reason"],
        "connection refused"
    );
    assert_eq!(
        not_ready[0]["diagnostics"]["host_rpc_uri"],
        "commandport://127.0.0.1:6000"
    );
    assert!(
        doctor["local"]["inventory"]["direct_control"]["reasons"]
            .get("per_dcc_sidecar")
            .is_none()
    );
}

#[test]
fn update_check_auto_starts_builtin_local_gateway() {
    let manifest_fixture = spawn_gateway_fixture();
    let manifest_url = format!("{}/update-manifest.json", manifest_fixture.base_url);
    let port = unused_loopback_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
        ("DCC_MCP_UPDATE_MANIFEST_URL", manifest_url.as_str()),
    ];
    let _cleanup = AutoGatewayCleanup {
        host: "127.0.0.1",
        port,
        envs: &envs,
    };

    let output = {
        let mut command = cli_command();
        command.args([
            "--base-url",
            &base_url,
            "--auto-gateway-bin",
            cli_bin,
            "--auto-gateway-timeout-secs",
            "15",
            "update",
            "check",
            "--binary",
            "dcc-mcp-server",
            "--current-version",
            "0.0.0",
        ]);
        for (key, value) in &envs {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("auto-started gateway"),
        "update check should auto-start the local gateway before querying updates: {stderr}"
    );
    let update: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(update["current_version"], "0.0.0");
    assert_eq!(update["update_available"], true);
    assert!(
        update["download_url"]
            .as_str()
            .unwrap()
            .contains("dcc-mcp-server")
    );
}

#[test]
fn staged_update_applies_before_version_short_circuit() {
    let temp = TempDir::new().unwrap();
    let source = std::path::Path::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"));
    let cli = temp.path().join(source.file_name().unwrap());
    std::fs::copy(source, &cli).unwrap();

    let appdata = temp.path().join("appdata");
    let xdg_data = temp.path().join("xdg-data");
    let home = temp.path().join("home");
    #[cfg(target_os = "windows")]
    let data_root = appdata.clone();
    #[cfg(target_os = "linux")]
    let data_root = xdg_data.clone();
    #[cfg(target_os = "macos")]
    let data_root = home.join("Library").join("Application Support");

    let staging = data_root.join("update").join("dcc-mcp-cli");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(source, staging.join("pending.bin")).unwrap();
    let marker = staging.join("pending.marker");
    std::fs::write(&marker, "pending").unwrap();

    let output = std::process::Command::new(&cli)
        .arg("--version")
        .env("APPDATA", &appdata)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "--version must apply a staged update before clap exits; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn smoke_with_explicit_url_does_not_auto_start_gateway() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let base_url = format!("http://127.0.0.1:{port}");
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
    ];
    let _cleanup = AutoGatewayCleanup {
        host: "127.0.0.1",
        port,
        envs: &envs,
    };

    let mcp_url = format!("{base_url}/mcp");
    let output = {
        let mut command = cli_command();
        command.args([
            "--base-url",
            &base_url,
            "--auto-gateway-bin",
            cli_bin,
            "--auto-gateway-timeout-secs",
            "2",
            "smoke",
            "--url",
            &mcp_url,
            "--timeout-secs",
            "1",
        ]);
        for (key, value) in &envs {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("auto-started gateway"),
        "explicit smoke URL should not trigger auto-start: {stderr}"
    );
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["base_url"], base_url);
    assert_eq!(value["mcp_url"], mcp_url);
    let checks = value["checks"].as_array().unwrap();
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "health" && check["ok"] == false),
        "expected failed health check without auto-start: {checks:#?}"
    );

    let status = run_json_with_env(
        &[
            "--no-auto-gateway",
            "gateway",
            "status",
            "--host",
            "127.0.0.1",
            "--port",
            &port_s,
        ],
        &envs,
    );
    assert_eq!(status["healthy"], false);
}

#[test]
fn update_check_supports_server_binary_versions() {
    let fixture = spawn_gateway_fixture();

    let update = run_json(&[
        "--base-url",
        &fixture.base_url,
        "update",
        "check",
        "--binary",
        "dcc-mcp-server",
        "--current-version",
        "0.18.16",
    ]);

    assert_eq!(update["update_available"], true);
    assert_eq!(update["current_version"], "0.18.16");
    assert_eq!(update["latest_version"], "0.19.0");
    assert_eq!(
        update["download_url"],
        "https://example.invalid/dcc-mcp-server.zip"
    );
    assert_eq!(update["sha256"], "abc123");
    assert_eq!(update["release_notes"], "Server update");
}

#[test]
fn update_check_preserves_gateway_error_payload() {
    let fixture = spawn_gateway_fixture();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "update",
            "check",
            "--binary",
            "dcc-mcp-cli",
            "--current-version",
            "0.18.16",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "update check should fail when the gateway reports an update error"
    );
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        update["error"],
        "binary 'dcc-mcp-cli' not found in update manifest"
    );
    assert_eq!(update["binary_name"], "dcc-mcp-cli");
    assert_eq!(update["current_version"], "0.18.16");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("missing field"),
        "stderr should not expose serde decode failures"
    );
}

#[test]
fn search_and_load_skill_decode_json_when_gateway_defaults_to_compact() {
    let fixture = spawn_gateway_fixture();

    let search = run_json(&[
        "--base-url",
        &fixture.base_url,
        "search",
        "--query",
        "sphere",
        "--dcc-type",
        "maya",
    ]);
    assert_eq!(search["hits"][0]["slug"], "maya.abc12345.create_sphere");

    let loaded = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "workflow",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
    ]);
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["registered_tools"][0], "workflow__run");
}

#[test]
fn pretty_list_shows_gateway_owner_and_candidates() {
    let fixture = spawn_gateway_fixture();

    let output = run_text(&[
        "--base-url",
        &fixture.base_url,
        "--output",
        "pretty",
        "list",
    ]);

    assert!(output.contains("Gateway"));
    assert!(output.contains("owner      Maya-main-15084"));
    assert!(output.contains("Maya-layout-120920"));
    assert!(output.contains("Instances"));
    assert!(output.contains("maya"));
}

#[test]
fn smoke_checks_gateway_mcp_and_rest_surfaces() {
    let fixture = spawn_gateway_fixture();
    let value = run_json(&[
        "--base-url",
        &fixture.base_url,
        "smoke",
        "--url",
        &format!("{}/mcp", fixture.base_url),
    ]);

    assert_eq!(value["ok"], true);
    assert_eq!(value["mcp_url"], format!("{}/mcp", fixture.base_url));
    let checks = value["checks"].as_array().unwrap();
    for expected in ["health", "mcp_initialize", "mcp_tools_list", "rest_search"] {
        assert!(
            checks
                .iter()
                .any(|check| check["name"] == expected && check["ok"] == true),
            "missing successful smoke check {expected}: {checks:#?}"
        );
    }
}
