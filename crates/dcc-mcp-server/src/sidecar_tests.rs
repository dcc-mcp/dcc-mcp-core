use super::*;
use dcc_mcp_transport::discovery::types::ServiceKey;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;
use tempfile::TempDir;

// ── Regression: ``default_registry_dir`` must match GatewayRunner's ──

static REGISTRY_ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(feature = "gateway-daemon")]
fn guardian_test_args() -> SidecarArgs {
    SidecarArgs {
        dcc: "maya".to_string(),
        host_rpc: "stub://localhost:0".to_string(),
        watch_pid: std::process::id(),
        registry_dir: None,
        instance_id: Some(Uuid::nil()),
        display_name: Some("Maya-Test".to_string()),
        adapter_version: Some("0.0.0-test".to_string()),
        connect_timeout_secs: 2,
        allow_stub_dispatch_ready: false,
        ppid_poll_ms: Some(50),
        gateway_port: 9765,
        no_ensure_gateway: false,
        legacy_gateway_election: false,
        host: "127.0.0.1".to_string(),
        gateway_host: None,
        gateway_name: None,
        gateway_remote_host: "0.0.0.0".to_string(),
        gateway_remote_port: 59765,
    }
}

#[test]
fn default_registry_dir_matches_gateway_runner_fallback() {
    let _guard = REGISTRY_ENV_LOCK.lock().expect("registry env lock");
    // ``GatewayRunner::new`` (crates/dcc-mcp-gateway/src/gateway/
    // runner.rs) falls back to ``std::env::temp_dir().join("dcc-mcp-
    // registry")``. The sidecar binary MUST agree, otherwise an
    // adapter that spawns a sidecar without forwarding
    // ``--registry-dir`` will split-brain the registry.
    //
    // Wipe ``DCC_MCP_REGISTRY_DIR`` for this assertion so we hit the
    // fallback path (the env-var path is tested separately below).
    // Other parallel tests may also touch the env, but the value is
    // restored at the end so the suite stays clean.
    let saved = std::env::var("DCC_MCP_REGISTRY_DIR").ok();
    // SAFETY: single-threaded mutation guarded by ``saved``/restore
    // immediately after the call. Other tests in this file that
    // touch ``DCC_MCP_REGISTRY_DIR`` would have set their own values
    // and we don't disturb those.
    unsafe { std::env::remove_var("DCC_MCP_REGISTRY_DIR") };

    let got = default_registry_dir();
    let expected = std::env::temp_dir().join("dcc-mcp-registry");

    if let Some(prev) = saved {
        unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", prev) };
    }

    assert_eq!(
        got, expected,
        "sidecar default_registry_dir must match GatewayRunner::new \
             fallback (<tempdir>/dcc-mcp-registry). Mismatch split-brains \
             the FileRegistry and produces a dark gateway port."
    );
}

#[test]
fn default_registry_dir_honours_env_var_override() {
    let _guard = REGISTRY_ENV_LOCK.lock().expect("registry env lock");
    let saved = std::env::var("DCC_MCP_REGISTRY_DIR").ok();
    let custom = std::env::temp_dir().join("dcc-mcp-custom-registry-test");
    unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", &custom) };

    let got = default_registry_dir();

    if let Some(prev) = saved {
        unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", prev) };
    } else {
        unsafe { std::env::remove_var("DCC_MCP_REGISTRY_DIR") };
    }

    assert_eq!(
        got, custom,
        "DCC_MCP_REGISTRY_DIR must win over the fallback path"
    );
}

#[cfg(feature = "gateway-daemon")]
#[test]
fn gateway_daemon_guardian_runs_only_in_daemon_backed_mode() {
    let mut args = guardian_test_args();
    assert!(
        should_use_gateway_daemon(&args),
        "default sidecar mode should ensure the daemon"
    );
    assert!(
        should_start_gateway_daemon_guardian(&args),
        "default sidecar mode should keep a guardian alive"
    );

    args.gateway_port = 0;
    assert!(
        !should_start_gateway_daemon_guardian(&args),
        "gateway_port=0 explicitly disables gateway participation"
    );

    args.gateway_port = 9765;
    args.no_ensure_gateway = true;
    assert!(
        !should_start_gateway_daemon_guardian(&args),
        "--no-ensure-gateway opts out of daemon launch and guardian"
    );

    args.no_ensure_gateway = false;
    args.legacy_gateway_election = true;
    assert!(
        !should_use_gateway_daemon(&args),
        "legacy embedded election must not auto-launch a standalone daemon"
    );
    assert!(
        !should_start_gateway_daemon_guardian(&args),
        "legacy embedded election must not keep a daemon guardian alive"
    );
}

#[cfg(feature = "gateway-daemon")]
#[test]
fn gateway_daemon_options_preserve_host_name_and_registry() {
    let mut args = guardian_test_args();
    args.gateway_host = Some("0.0.0.0".to_string());
    args.gateway_name = Some("studio-gateway".to_string());
    let registry_dir = PathBuf::from("/tmp/dcc-mcp-registry-test");

    let opts = build_gateway_daemon_options(&args, registry_dir.clone());
    assert_eq!(opts.host, "0.0.0.0");
    assert_eq!(opts.name.as_deref(), Some("studio-gateway"));
    assert_eq!(opts.registry_dir, registry_dir);
    assert_eq!(opts.remote_host, "0.0.0.0");
    assert_eq!(opts.remote_port, 59765);

    let mut display_name_args = guardian_test_args();
    display_name_args.display_name = Some("Blender-Lookdev".to_string());
    let opts = build_gateway_daemon_options(&display_name_args, PathBuf::from("registry"));
    assert_eq!(opts.host, "127.0.0.1");
    assert_eq!(opts.name.as_deref(), Some("gateway-for-Blender-Lookdev"));
}

#[tokio::test]
async fn sidecar_heartbeat_keeps_registry_row_fresh() {
    let registry_dir = TempDir::new().expect("tempdir");
    let registry = Arc::new(FileRegistry::new(registry_dir.path()).expect("registry"));
    let entry = ServiceEntry::new("3dsmax", "127.0.0.1", 55201).with_pid(std::process::id());
    let key = entry.key();
    registry.register(entry).expect("register sidecar row");
    let before = registry.get(&key).expect("registered row").last_heartbeat;

    let handle = spawn_sidecar_heartbeat(registry.clone(), key.clone(), Duration::from_millis(10));
    tokio::time::sleep(Duration::from_millis(40)).await;
    handle.abort();

    let after = registry.get(&key).expect("heartbeat row").last_heartbeat;
    assert!(
        after > before,
        "sidecar heartbeat must advance while the sidecar process is alive"
    );
}

/// PPID-watch happy path: spawn a real child process, register a sidecar
/// pinned to that child's PID, kill the child, assert the sidecar exits
/// quickly and the FileRegistry row is gone.
///
/// Uses a real OS process (not the current pid) to avoid the "watch_pid
/// is the sidecar itself" footgun.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ppid_watch_exits_on_parent_death() {
    let registry_dir = TempDir::new().expect("tempdir");

    // Spawn a long-sleeping child; we'll kill it to simulate DCC death.
    let mut child = std::process::Command::new(sleep_cmd())
        .args(sleep_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep child");

    let parent_pid = child.id();
    let key_dcc = "test-dcc".to_string();
    let args = SidecarArgs {
        dcc: key_dcc.clone(),
        // Use the `stub` scheme so the HostRpcClient connects
        // immediately (no I/O) and the focus of this test stays
        // on the PPID-watch path. The commandport scheme is
        // exercised separately by `commandport_connects_to_fake_server`.
        host_rpc: "stub://localhost:0".to_string(),
        watch_pid: parent_pid,
        registry_dir: Some(registry_dir.path().to_path_buf()),
        instance_id: Some(Uuid::new_v4()),
        display_name: Some("test-sidecar".to_string()),
        adapter_version: Some("0.0.0-test".to_string()),
        connect_timeout_secs: 2,
        allow_stub_dispatch_ready: false,
        ppid_poll_ms: Some(50),
        gateway_port: 0,
        no_ensure_gateway: false,
        legacy_gateway_election: false,
        host: "127.0.0.1".to_string(),
        gateway_host: None,
        gateway_name: None,
        gateway_remote_host: "0.0.0.0".to_string(),
        gateway_remote_port: 59765,
    };
    let pinned_uuid = args.instance_id.unwrap();

    // Run the sidecar in the background; it should register itself,
    // then exit shortly after we kill the parent.
    let sidecar_handle = tokio::spawn(async move { run(args).await });

    // Wait for registration to land before killing the parent — gives
    // the sidecar a fair shot at writing to FileRegistry.
    wait_for_registration(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(2),
    )
    .await
    .expect("sidecar registered itself within 2s");

    // Kill the parent.
    child.kill().expect("kill sleep child");
    let _ = child.wait();

    // Sidecar should exit within ~250ms of detecting parent death
    // (50ms poll + a couple of ticks of slack on slow CI).
    let exit_deadline = Instant::now() + Duration::from_secs(3);
    let result = tokio::time::timeout_at(
        tokio::time::Instant::from_std(exit_deadline),
        sidecar_handle,
    )
    .await
    .expect("sidecar did not exit within 3s of parent death")
    .expect("sidecar task did not panic");
    result.expect("sidecar run returned an error");

    // FileRegistry row must be gone (deregister ran in the shutdown path).
    let registry = FileRegistry::new(registry_dir.path()).expect("reopen registry");
    let key = ServiceKey {
        dcc_type: key_dcc,
        instance_id: pinned_uuid,
    };
    assert!(
        registry.get(&key).is_none(),
        "sidecar should have deregistered itself; row still present"
    );
}

/// `stub://` is a test placeholder, not proof that a real DCC dispatcher
/// is callable. A production sidecar using it must stay non-routable so
/// plugin startup code cannot mistake process registration for tool
/// readiness.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stub_host_rpc_is_unavailable_without_test_opt_in() {
    let registry_dir = TempDir::new().expect("tempdir");

    let mut child = std::process::Command::new(sleep_cmd())
        .args(sleep_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep child");

    let parent_pid = child.id();
    let key_dcc = "maya".to_string();
    let pinned_uuid = Uuid::new_v4();
    let args = SidecarArgs {
        dcc: key_dcc.clone(),
        host_rpc: "stub://localhost:0".to_string(),
        watch_pid: parent_pid,
        registry_dir: Some(registry_dir.path().to_path_buf()),
        instance_id: Some(pinned_uuid),
        display_name: Some("stub-sidecar".to_string()),
        adapter_version: Some("0.0.0-test".to_string()),
        connect_timeout_secs: 1,
        allow_stub_dispatch_ready: false,
        ppid_poll_ms: Some(50),
        gateway_port: 0,
        no_ensure_gateway: false,
        legacy_gateway_election: false,
        host: "127.0.0.1".to_string(),
        gateway_host: None,
        gateway_name: None,
        gateway_remote_host: "0.0.0.0".to_string(),
        gateway_remote_port: 59765,
    };

    let sidecar_handle = tokio::spawn(async move { run(args).await });

    wait_for_registration(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(2),
    )
    .await
    .expect("sidecar registered itself within 2s");
    let failed_row = wait_for_unavailable_listener(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(3),
    )
    .await
    .expect("stub sidecar should publish an unavailable diagnostic listener");
    assert_eq!(failed_row.status, ServiceStatus::Booting);
    assert_eq!(
        failed_row
            .metadata
            .get(HOST_RPC_SCHEME_METADATA_KEY)
            .map(String::as_str),
        Some("stub")
    );
    assert_eq!(
        failed_row
            .metadata
            .get(FAILURE_STAGE_METADATA_KEY)
            .map(String::as_str),
        Some("host-rpc-stub")
    );
    assert!(
        failed_row
            .metadata
            .get(FAILURE_REASON_METADATA_KEY)
            .is_some_and(|reason| reason.contains("test-only")),
        "stub failure reason should tell installers it is not dispatch-ready"
    );

    let mcp_url = failed_row
        .metadata
        .get("mcp_url")
        .expect("diagnostic listener should publish mcp_url")
        .clone();
    let base_url = mcp_url
        .strip_suffix("/mcp")
        .expect("sidecar mcp_url should end with /mcp");
    let ready_response = reqwest::Client::new()
        .get(format!("{base_url}/v1/readyz"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("GET diagnostic /v1/readyz");
    assert_eq!(
        ready_response.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    let ready_body: serde_json::Value = ready_response
        .json()
        .await
        .expect("parse diagnostic /v1/readyz");
    assert_eq!(ready_body["dispatcher"], false);

    let body: serde_json::Value = post_mcp(
        &mcp_url,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "stub-not-ready",
            "method": "tools/call",
            "params": {
                "name": "maya_diagnostics__ping",
                "arguments": {}
            }
        }),
    )
    .await
    .json()
    .await
    .expect("parse diagnostic tools/call response");
    assert_eq!(body["error"]["message"], "transport-error");
    assert_eq!(body["error"]["data"]["kind"], "transport-error");
    assert!(
        body["error"]["data"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("stub://"),
        "diagnostic tools/call should preserve the stub URI failure: {body}"
    );

    child.kill().expect("kill sleep child");
    let _ = child.wait();

    let result = tokio::time::timeout(Duration::from_secs(3), sidecar_handle)
        .await
        .expect("sidecar exited after parent death")
        .expect("no panic");
    result.expect("run() returned ok");
}

/// End-to-end commandport happy path: spawn a fake TCP server,
/// spawn the sidecar with ``commandport://127.0.0.1:<port>``,
/// assert the fake server observes the bootstrap line (proving
/// the URI router picked CommandPortClient AND that connect()'s
/// bootstrap-injection step ran), then kill the parent surrogate
/// and assert clean exit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commandport_connects_to_fake_server() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    let registry_dir = TempDir::new().expect("tempdir");

    // Bind a fake "Maya commandPort" on an OS-assigned port.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 0");
    let port = listener.local_addr().expect("local_addr").port();
    let (connect_tx, connect_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        // Accept one connection, reply to the bootstrap line, then
        // hold the socket open until teardown.
        if let Ok((mut stream, _)) = listener.accept().await {
            let _ = connect_tx.send(());
            let (read_half, mut write_half) = stream.split();
            let mut reader = BufReader::new(read_half);
            let mut bootstrap_line = String::new();
            let _ = reader.read_line(&mut bootstrap_line).await;
            // `exec()` evaluates to None in commandPort's reply path.
            let _ = write_half.write_all(b"None\n").await;
            let _ = write_half.flush().await;
            // Keep the socket alive until the sidecar tears down.
            // 5s is more than enough for this test's lifetime.
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    let mut child = std::process::Command::new(sleep_cmd())
        .args(sleep_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep child");

    let parent_pid = child.id();
    let key_dcc = "maya".to_string();
    let pinned_uuid = Uuid::new_v4();
    let args = SidecarArgs {
        dcc: key_dcc.clone(),
        host_rpc: format!("commandport://127.0.0.1:{port}"),
        watch_pid: parent_pid,
        registry_dir: Some(registry_dir.path().to_path_buf()),
        instance_id: Some(pinned_uuid),
        display_name: Some("test-maya".to_string()),
        adapter_version: Some("0.0.0-test".to_string()),
        connect_timeout_secs: 2,
        allow_stub_dispatch_ready: false,
        ppid_poll_ms: Some(50),
        gateway_port: 0,
        no_ensure_gateway: false,
        legacy_gateway_election: false,
        host: "127.0.0.1".to_string(),
        gateway_host: None,
        gateway_name: None,
        gateway_remote_host: "0.0.0.0".to_string(),
        gateway_remote_port: 59765,
    };

    let sidecar_handle = tokio::spawn(async move { run(args).await });

    // Confirm the sidecar's CommandPortClient actually connected
    // — this proves the URI router picked the right impl AND
    // that the connect() path is wired through end-to-end.
    tokio::time::timeout(Duration::from_secs(3), connect_rx)
        .await
        .expect("sidecar must connect to fake commandPort within 3s")
        .expect("connect channel closed without firing");

    // Confirm the registry row landed too (orthogonal to the
    // connect — the row is written before connect attempts).
    wait_for_registration(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(2),
    )
    .await
    .expect("sidecar registered itself within 2s");
    let ready_row = wait_for_dispatch_status(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        DISPATCH_STATUS_READY,
        Duration::from_secs(3),
    )
    .await
    .expect("sidecar must publish dispatch-ready metadata");
    assert_eq!(ready_row.status, ServiceStatus::Available);
    assert_ne!(ready_row.port, 0);
    assert_eq!(
        ready_row
            .metadata
            .get(HOST_RPC_SCHEME_METADATA_KEY)
            .map(String::as_str),
        Some("commandport")
    );
    assert!(ready_row.metadata.contains_key("mcp_url"));
    assert!(
        ready_row
            .metadata
            .contains_key(DISPATCH_READY_AT_UNIX_METADATA_KEY),
        "dispatch-ready row should include a timestamp"
    );

    // Kill the parent and assert clean shutdown.
    child.kill().expect("kill sleep child");
    let _ = child.wait();

    let result = tokio::time::timeout(Duration::from_secs(3), sidecar_handle)
        .await
        .expect("sidecar exited within 3s of parent death")
        .expect("sidecar task did not panic");
    result.expect("sidecar run returned ok");

    let registry = FileRegistry::new(registry_dir.path()).expect("reopen");
    let key = ServiceKey {
        dcc_type: key_dcc,
        instance_id: pinned_uuid,
    };
    assert!(
        registry.get(&key).is_none(),
        "sidecar must have deregistered itself"
    );
}

/// Soft-failure path: when the URI's host:port is dead, the sidecar
/// logs a warning but **keeps running** so its FileRegistry row
/// stays visible and PPID-watch can still detect parent death.
/// The gateway sees a registered-but-disconnected backend and
/// routes around it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sidecar_survives_failed_initial_connect() {
    use tokio::net::TcpListener;

    let registry_dir = TempDir::new().expect("tempdir");

    // Allocate a port and immediately drop the listener so any
    // connect attempt sees ECONNREFUSED quickly.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dead_port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let mut child = std::process::Command::new(sleep_cmd())
        .args(sleep_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep child");

    let parent_pid = child.id();
    let key_dcc = "maya".to_string();
    let pinned_uuid = Uuid::new_v4();
    let args = SidecarArgs {
        dcc: key_dcc.clone(),
        host_rpc: format!("commandport://127.0.0.1:{dead_port}"),
        watch_pid: parent_pid,
        registry_dir: Some(registry_dir.path().to_path_buf()),
        instance_id: Some(pinned_uuid),
        display_name: None,
        adapter_version: None,
        // 300ms is plenty for ECONNREFUSED on Windows; bumps any
        // slow CI well above the noise floor while keeping the
        // test snappy in the common case.
        connect_timeout_secs: 1,
        allow_stub_dispatch_ready: false,
        ppid_poll_ms: Some(50),
        gateway_port: 0,
        no_ensure_gateway: false,
        legacy_gateway_election: false,
        host: "127.0.0.1".to_string(),
        gateway_host: None,
        gateway_name: None,
        gateway_remote_host: "0.0.0.0".to_string(),
        gateway_remote_port: 59765,
    };

    let sidecar_handle = tokio::spawn(async move { run(args).await });

    // Even with connect failed, the sidecar must register itself
    // — that's the whole point of the soft-failure contract.
    wait_for_registration(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(3),
    )
    .await
    .expect("sidecar must register even when connect fails");
    let failed_row = wait_for_unavailable_listener(
        registry_dir.path(),
        &key_dcc,
        pinned_uuid,
        Duration::from_secs(3),
    )
    .await
    .expect("sidecar should expose host-rpc failure metadata and diagnostic listener");
    assert_eq!(failed_row.status, ServiceStatus::Booting);
    assert_ne!(
        failed_row.port, 0,
        "unavailable sidecar still publishes a diagnostic listener"
    );
    assert_eq!(
        failed_row
            .metadata
            .get(HOST_RPC_SCHEME_METADATA_KEY)
            .map(String::as_str),
        Some("commandport")
    );
    assert_eq!(
        failed_row
            .metadata
            .get(DISPATCH_STATUS_METADATA_KEY)
            .map(String::as_str),
        Some(DISPATCH_STATUS_UNAVAILABLE)
    );
    assert_eq!(
        failed_row
            .metadata
            .get(FAILURE_STAGE_METADATA_KEY)
            .map(String::as_str),
        Some("host-rpc-connect")
    );
    assert!(
        failed_row
            .metadata
            .get(FAILURE_REASON_METADATA_KEY)
            .is_some_and(|reason| reason.contains("host-rpc connect"))
    );
    let mcp_url = failed_row
        .metadata
        .get("mcp_url")
        .expect("diagnostic listener should publish mcp_url")
        .clone();
    let body: serde_json::Value = post_mcp(
        &mcp_url,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "failed-connect",
            "method": "tools/call",
            "params": {
                "name": "maya_primitives__create_sphere",
                "arguments": {}
            }
        }),
    )
    .await
    .json()
    .await
    .expect("parse diagnostic tools/call response");
    assert_eq!(body["error"]["message"], "transport-error");
    assert_eq!(body["error"]["data"]["kind"], "transport-error");
    assert!(
        body["error"]["data"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("host-rpc connect"),
        "diagnostic listener should preserve startup failure: {body}"
    );

    child.kill().expect("kill sleep child");
    let _ = child.wait();

    let result = tokio::time::timeout(Duration::from_secs(4), sidecar_handle)
        .await
        .expect("sidecar exited after parent death")
        .expect("no panic");
    result.expect("run() returned ok");
}

fn sleep_cmd() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "sleep"
    }
}

fn sleep_args() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]
    } else {
        vec!["60"]
    }
}

async fn wait_for_registration(
    registry_dir: &std::path::Path,
    dcc: &str,
    instance_id: Uuid,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("registry row never appeared");
        }
        // Reopening the registry forces a reload from disk; the
        // background sidecar writes through `flush_to_file`.
        let registry =
            FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
        let key = ServiceKey {
            dcc_type: dcc.to_string(),
            instance_id,
        };
        if registry.get(&key).is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_dispatch_status(
    registry_dir: &std::path::Path,
    dcc: &str,
    instance_id: Uuid,
    expected: &str,
    timeout: Duration,
) -> anyhow::Result<ServiceEntry> {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("registry row never reached dispatch_status={expected}");
        }
        let registry =
            FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
        let key = ServiceKey {
            dcc_type: dcc.to_string(),
            instance_id,
        };
        if let Some(row) = registry.get(&key)
            && row
                .metadata
                .get(DISPATCH_STATUS_METADATA_KEY)
                .is_some_and(|status| status == expected)
        {
            return Ok(row);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_unavailable_listener(
    registry_dir: &std::path::Path,
    dcc: &str,
    instance_id: Uuid,
    timeout: Duration,
) -> anyhow::Result<ServiceEntry> {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("registry row never recorded unavailable diagnostic listener");
        }
        let registry =
            FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
        let key = ServiceKey {
            dcc_type: dcc.to_string(),
            instance_id,
        };
        if let Some(row) = registry.get(&key)
            && row.metadata.contains_key(FAILURE_REASON_METADATA_KEY)
            && row
                .metadata
                .get(DISPATCH_STATUS_METADATA_KEY)
                .is_some_and(|status| status == DISPATCH_STATUS_UNAVAILABLE)
            && row.port != 0
            && row.metadata.contains_key("mcp_url")
        {
            return Ok(row);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn post_mcp(url: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("POST diagnostic /mcp")
}

#[test]
fn role_metadata_key_is_stable() {
    // Pin the public constant so downstream tools that grep for it
    // (admin UI / observability dashboards) cannot break silently.
    assert_eq!(ROLE_METADATA_KEY, "dcc_mcp_role");
    assert_eq!(ROLE_PER_DCC_SIDECAR, "per-dcc-sidecar");
}
