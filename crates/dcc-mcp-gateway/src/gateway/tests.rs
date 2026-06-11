use super::*;
use dcc_mcp_transport::discovery::types::ServiceEntry;

const TEST_OWN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build the `ElectionInfo` representing this process — crate-only, no
/// adapter metadata — used by the issue #228 regressions where adapter
/// info is irrelevant.
fn own_crate_only() -> ElectionInfo<'static> {
    ElectionInfo::new(TEST_OWN_VERSION, None, None)
}

#[test]
fn test_parse_semver_basic() {
    assert_eq!(parse_semver("0.12.29"), (0, 12, 29));
    assert_eq!(parse_semver("v1.2.3"), (1, 2, 3));
    assert_eq!(parse_semver("1.0.0-rc1"), (1, 0, 0));
    assert_eq!(parse_semver("1.2"), (1, 2, 0));
    assert_eq!(parse_semver("abc"), (0, 0, 0));
}

#[test]
fn test_is_newer_version_ordering() {
    assert!(is_newer_version("0.12.29", "0.12.6"));
    assert!(is_newer_version("1.0.0", "0.99.99"));
    assert!(!is_newer_version("0.12.6", "0.12.6"));
    assert!(!is_newer_version("0.12.5", "0.12.6"));
}

// Issue maya#137 — three-tier election order.
#[test]
fn test_is_newer_election_crate_version_dominates() {
    let cand = ElectionInfo::new("0.15.0", Some("0.3.0"), Some("maya"));
    let cur = ElectionInfo::new("0.14.0", Some("9.9.9"), Some("maya"));
    assert!(
        is_newer_election(cand, cur),
        "crate version must dominate adapter version"
    );
}

#[test]
fn test_is_newer_election_adapter_version_breaks_crate_tie() {
    let cand = ElectionInfo::new("0.14.0", Some("0.3.1"), Some("maya"));
    let cur = ElectionInfo::new("0.14.0", Some("0.3.0"), Some("maya"));
    assert!(is_newer_election(cand, cur));
    assert!(!is_newer_election(cur, cand));
}

#[test]
fn test_is_newer_election_real_dcc_beats_unknown() {
    // The reproduction from maya#137: standalone server pinned to a
    // newer crate (0.14.18) is `unknown`; Maya plugin (0.3.0 adapter)
    // ships with a real DCC. After the fix the Maya plugin should win
    // when the crate versions are equal.
    let cand = ElectionInfo::new("0.14.18", Some("0.3.0"), Some("maya"));
    let cur = ElectionInfo::new("0.14.18", None, Some("unknown"));
    assert!(
        is_newer_election(cand, cur),
        "real DCC must preempt unknown standalone at equal versions"
    );
}

#[test]
fn test_is_newer_election_two_real_dccs_remain_tied() {
    // Two real DCCs at identical crate+adapter versions must not flip
    // each other — the first-wins port-bind contract takes over.
    let a = ElectionInfo::new("0.14.18", Some("0.3.0"), Some("maya"));
    let b = ElectionInfo::new("0.14.18", Some("0.3.0"), Some("houdini"));
    assert!(!is_newer_election(a, b));
    assert!(!is_newer_election(b, a));
}

#[test]
fn test_is_newer_election_missing_adapter_loses_to_present() {
    let cand = ElectionInfo::new("0.14.0", Some("0.3.0"), Some("maya"));
    let cur = ElectionInfo::new("0.14.0", None, Some("maya"));
    assert!(is_newer_election(cand, cur));
    assert!(!is_newer_election(cur, cand));
}

// Regression test for issue #228: Maya's host version ("2024") must not
// be mistaken for a newer gateway-crate version. Only the __gateway__
// sentinel row contributes to the self-yield decision.
#[test]
fn test_has_newer_sentinel_ignores_dcc_host_version() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    // A Maya instance registering itself with its host version — this
    // must never trigger a gateway self-yield even though "2024" parses
    // to (2024, 0, 0) which is numerically larger than the crate version.
    let mut maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
    maya.version = Some("2024".to_string());
    reg.register(maya).unwrap();

    assert!(
        !has_newer_sentinel(&reg, own_crate_only(), Duration::from_secs(30)),
        "Maya 2024 host version must not appear as a newer gateway"
    );
}

// Regression test for issue #228 (positive case): an actual newer
// __gateway__ sentinel entry MUST still trigger the voluntary yield.
#[test]
fn test_has_newer_sentinel_detects_newer_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some("99.0.0".to_string());
    reg.register(sentinel).unwrap();

    assert!(
        has_newer_sentinel(&reg, own_crate_only(), Duration::from_secs(30)),
        "a newer-version sentinel must trigger yield"
    );
}

// Regression test for issue #228: own sentinel write must not cause a
// self-yield (same version → not newer).
#[test]
fn test_has_newer_sentinel_ignores_own_version() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some(TEST_OWN_VERSION.to_string());
    reg.register(sentinel).unwrap();

    assert!(
        !has_newer_sentinel(&reg, own_crate_only(), Duration::from_secs(30)),
        "identical version sentinel must not trigger yield"
    );
}

// Regression test for issue #228: a stale sentinel (older gateway
// crashed without cleanup) must not block us from becoming gateway.
#[test]
fn test_has_newer_sentinel_ignores_stale_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some("9.9.9".to_string());
    sentinel.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
    reg.register(sentinel).unwrap();

    assert!(
        !has_newer_sentinel(&reg, own_crate_only(), Duration::from_secs(30)),
        "stale sentinel (crashed gateway) must not block newer takeover"
    );
}

// Issue maya#137 — `has_newer_sentinel` must yield to a peer that has
// the same crate version, the same adapter version, but a real DCC type
// while the resident sentinel is `unknown`.
#[test]
fn test_has_newer_sentinel_real_dcc_preempts_unknown_standalone() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some(TEST_OWN_VERSION.to_string());
    sentinel.adapter_version = Some("0.3.0".to_string());
    sentinel.adapter_dcc = Some("maya".to_string());
    reg.register(sentinel).unwrap();

    // We are an `unknown` standalone at the same crate + adapter version
    // → must defer to the resident Maya gateway.
    let own = ElectionInfo::new(TEST_OWN_VERSION, Some("0.3.0"), Some("unknown"));
    assert!(
        has_newer_sentinel(&reg, own, Duration::from_secs(30)),
        "real DCC sentinel must preempt unknown standalone"
    );
}

// Issue maya#137 (negative): an `unknown` resident sentinel must not
// trip self-yield for a real-DCC owner of the same crate version.
#[test]
fn test_has_newer_sentinel_real_dcc_owner_ignores_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some(TEST_OWN_VERSION.to_string());
    sentinel.adapter_dcc = Some("unknown".to_string());
    reg.register(sentinel).unwrap();

    let own = ElectionInfo::new(TEST_OWN_VERSION, Some("0.3.0"), Some("maya"));
    assert!(
        !has_newer_sentinel(&reg, own, Duration::from_secs(30)),
        "real DCC owner must not yield to an unknown peer at equal crate version"
    );
}

// Regression test for issue #229: sentinel heartbeat must be refreshable
// via `FileRegistry::heartbeat`, which is what the cleanup loop calls.
#[test]
fn test_gateway_sentinel_heartbeat_advances() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some(TEST_OWN_VERSION.to_string());
    // Age the heartbeat so the before/after delta is observable.
    sentinel.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(120);
    let key = sentinel.key();
    reg.register(sentinel).unwrap();

    let before = reg.get(&key).unwrap().last_heartbeat;
    assert!(reg.heartbeat(&key).unwrap(), "heartbeat must find sentinel");
    let after = reg.get(&key).unwrap().last_heartbeat;

    assert!(
        after > before,
        "sentinel heartbeat must advance after heartbeat() call (before={before:?}, after={after:?})"
    );
    // And after heartbeating it must NOT be considered stale anymore.
    let entry = reg.get(&key).unwrap();
    assert!(!entry.is_stale(Duration::from_secs(30)));
}

// ── Regression tests for issue #303 ──────────────────────────────────
//
// `start_gateway_tasks` must not report `is_gateway = true` when the
// listener's accept loop never comes up. `self_probe_listener` is the
// mechanism that detects this, so verify both the success and failure
// paths.

// Success path: probing an address with a real, running accept-loop
// must return Ok well within the retry budget.
#[tokio::test]
async fn test_self_probe_listener_succeeds_for_live_socket() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Drive accept in the background so connect() completes.
    tokio::spawn(async move {
        // One accept is enough for the probe; after that just hold the
        // listener so the OS doesn't drop it.
        let _ = listener.accept().await;
        // Keep the listener alive until the task is aborted.
        std::future::pending::<()>().await;
    });

    super::self_probe_listener(addr)
        .await
        .expect("probe must succeed against a live listener");
}

// Failure path: probing a definitely-unbound port must return Err after
// exhausting the retry budget, not hang forever.
#[tokio::test]
async fn test_self_probe_listener_fails_for_dead_port() {
    // Bind-then-drop gives us a port the OS *just* released. We combine
    // that with a fresh IPv4 loopback addr so the probe sees either
    // "refused" or "timed out" — both must surface as Err.
    let ephemeral = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = ephemeral.local_addr().unwrap();
    drop(ephemeral);

    // Entire probe (10 attempts * (200 ms timeout + 100 ms backoff)) must
    // finish in well under 5 s; cap the test at 5 s to catch regressions
    // that accidentally make the probe block indefinitely.
    let result = tokio::time::timeout(Duration::from_secs(5), super::self_probe_listener(addr))
        .await
        .expect("self-probe must not hang past its budget");

    assert!(
        result.is_err(),
        "probe must fail when nothing is listening on {addr}"
    );
}

// ── Regression tests for issue #718 ──────────────────────────────────
//
// `GatewayHandle::Drop` must call `FileRegistry::deregister` for every
// key it owns (instance row + sentinel for winners) so peers don't see
// a zombie "available" row for the full `stale_timeout_secs` window.

/// Build a `GatewayRunner` with a dedicated registry dir and a chosen
/// gateway port so the tests can run in parallel without colliding.
fn make_runner_in(dir: &std::path::Path, port: u16) -> GatewayRunner {
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        // Disable the heartbeat loop — we don't want a background task
        // touching `services.json` while we assert on its contents.
        heartbeat_secs: 0,
        registry_dir: Some(dir.to_path_buf()),
        ..GatewayConfig::default()
    };
    GatewayRunner::new(cfg).unwrap()
}

/// Pick an unused high port the OS just released. Good enough for
/// test-local first-wins elections; the runner's `try_bind_port_opt`
/// will still behave correctly under a race.
fn ephemeral_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

async fn gateway_initialize_version(port: u16) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|err| format!("failed to build client: {err}"))?;
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "gateway-test", "version": "0.0.0"}
            }
        }))
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|err| format!("invalid JSON response: {err}"))?;
    body.pointer("/result/serverInfo/version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing serverInfo.version in {body}"))
}

async fn wait_gateway_initialize_version(port: u16, expected: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let observed = match gateway_initialize_version(port).await {
            Ok(version) if version == expected => return,
            Ok(version) => format!("version {version}"),
            Err(err) => err,
        };
        assert!(
            tokio::time::Instant::now() < deadline,
            "gateway on port {port} did not advertise version {expected} before timeout; last observed: {observed}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_gateway_sentinel_version(runner: &GatewayRunner, expected: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let observed = {
            let reg = runner.registry.read().await;
            let versions: Vec<_> = reg
                .list_instances(GATEWAY_SENTINEL_DCC_TYPE)
                .into_iter()
                .map(|entry| entry.version.unwrap_or_else(|| "<missing>".to_string()))
                .collect();
            if versions.iter().any(|version| version == expected) {
                return;
            }
            if !versions.is_empty() {
                versions.join(", ")
            } else {
                "no sentinel row".to_string()
            }
        };
        assert!(
            tokio::time::Instant::now() < deadline,
            "gateway sentinel did not advertise version {expected} before timeout; last observed: {observed}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_gateway_winner_serves_optional_remote_listener() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();
    let remote_port = ephemeral_port();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: gw_port,
        remote_host: Some("127.0.0.1".to_string()),
        remote_gateway_port: remote_port,
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();

    let outcome = runner.run_election().await.unwrap();

    assert!(outcome.is_gateway, "free local port must win election");
    let resp = reqwest::get(format!("http://127.0.0.1:{remote_port}/health"))
        .await
        .expect("remote gateway listener should accept connections");
    assert!(resp.status().is_success());

    if let Some(abort) = outcome.gateway_abort {
        abort.abort();
    }
}

#[tokio::test]
async fn test_gateway_winner_stamps_human_readable_name_on_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: gw_port,
        gateway_name: Some("maya-main-window".to_string()),
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();

    let outcome = runner.run_election().await.unwrap();

    assert!(outcome.is_gateway, "free local port must win election");
    let reg = runner.registry.read().await;
    let sentinels = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
    assert_eq!(sentinels.len(), 1);
    let sentinel = &sentinels[0];
    assert_eq!(sentinel.display_name.as_deref(), Some("maya-main-window"));
    assert_eq!(
        sentinel.metadata.get("gateway_name").map(String::as_str),
        Some("maya-main-window")
    );
    assert_eq!(
        sentinel.metadata.get("gateway_role").map(String::as_str),
        Some("active")
    );
    let expected_health_url = format!("http://127.0.0.1:{gw_port}/health");
    let expected_mcp_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let expected_pid = std::process::id().to_string();
    assert_eq!(
        sentinel
            .metadata
            .get("gateway_health_url")
            .map(String::as_str),
        Some(expected_health_url.as_str())
    );
    assert_eq!(
        sentinel.metadata.get("gateway_mcp_url").map(String::as_str),
        Some(expected_mcp_url.as_str())
    );
    assert_eq!(
        sentinel
            .metadata
            .get("gateway_process_pid")
            .map(String::as_str),
        Some(expected_pid.as_str())
    );
    assert!(
        sentinel
            .metadata
            .get("gateway_process_exe")
            .is_some_and(|value| !value.trim().is_empty()),
        "sentinel should expose the executable that owns the gateway"
    );
    assert!(
        sentinel
            .metadata
            .get("gateway_started_at_unix")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|value| value > 0),
        "sentinel should expose a machine-readable start timestamp"
    );
    assert_eq!(
        sentinel.metadata.get("gateway_persist").map(String::as_str),
        Some("false")
    );
    assert_eq!(
        sentinel
            .metadata
            .get("gateway_idle_timeout_secs")
            .map(String::as_str),
        Some("30")
    );
    drop(reg);

    if let Some(abort) = outcome.gateway_abort {
        abort.abort();
    }
}

#[tokio::test]
async fn test_newer_gateway_does_not_preempt_healthy_local_and_remote_listeners() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();
    let remote_port = ephemeral_port();

    let old_runner = GatewayRunner::new(GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: gw_port,
        remote_host: Some("127.0.0.1".to_string()),
        remote_gateway_port: remote_port,
        server_version: "0.1.0".to_string(),
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    })
    .unwrap();
    let old = old_runner.run_election().await.unwrap();
    assert!(old.is_gateway, "old runner should win the initial election");
    wait_gateway_initialize_version(gw_port, "0.1.0", Duration::from_secs(10)).await;
    wait_gateway_initialize_version(remote_port, "0.1.0", Duration::from_secs(10)).await;

    let new_runner = GatewayRunner::new(GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: gw_port,
        remote_host: Some("127.0.0.1".to_string()),
        remote_gateway_port: remote_port,
        server_version: "9.9.9".to_string(),
        heartbeat_secs: 0,
        challenger_poll_interval_secs: 1,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    })
    .unwrap();
    let new = new_runner.run_election().await.unwrap();

    assert!(
        !new.is_gateway,
        "new runner must not win while a healthy resident owns the port"
    );
    assert!(
        new.challenger_abort.is_none(),
        "newer runner must stay a plain peer while the resident gateway is healthy"
    );
    wait_gateway_sentinel_version(&old_runner, "0.1.0", Duration::from_secs(10)).await;
    wait_gateway_initialize_version(gw_port, "0.1.0", Duration::from_secs(10)).await;
    wait_gateway_initialize_version(remote_port, "0.1.0", Duration::from_secs(10)).await;

    if let Some(abort) = new.challenger_abort {
        abort.abort();
    }
    if let Some(abort) = old.gateway_abort {
        abort.abort();
    }
}

#[tokio::test]
async fn test_gateway_handle_drop_deregisters_instance_row() {
    // Point gateway_port at something that is already bound so we land
    // on the `port taken` branch and the handle holds only the instance
    // row (no sentinel). Drop must still remove that row.
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();

    let runner = make_runner_in(dir.path(), port);
    let entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    let key = entry.key();
    let handle = runner.start(entry, None).await.unwrap();
    assert!(!handle.is_gateway, "port is occupied — must be non-winner");

    // Row must exist while the handle is alive.
    {
        let reg = runner.registry.read().await;
        assert!(
            reg.get(&key).is_some(),
            "instance row must be present before Drop"
        );
    }

    // Drop the handle → `Drop::drop` must deregister the instance row.
    drop(handle);

    let reg = runner.registry.read().await;
    assert!(
        reg.get(&key).is_none(),
        "GatewayHandle::Drop must remove the instance row from FileRegistry (issue #718)"
    );
    drop(occupied);
}

#[tokio::test]
async fn test_gateway_heartbeat_merges_live_instance_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();

    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 1,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    let key = entry.key();
    let provider: MetadataProvider = std::sync::Arc::new(|| LiveSnapshot {
        metadata: std::collections::HashMap::from([
            (
                "gateway_runtime_mode".to_string(),
                "daemon-backed".to_string(),
            ),
            ("gateway_guardian_enabled".to_string(), "true".to_string()),
        ]),
        ..LiveSnapshot::default()
    });
    let handle = runner.start(entry, Some(provider)).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        {
            let reg = runner.registry.read().await;
            let row = reg.get(&key).expect("registered row");
            if row
                .metadata
                .get("gateway_guardian_enabled")
                .is_some_and(|value| value == "true")
            {
                assert_eq!(
                    row.metadata.get("gateway_runtime_mode").map(String::as_str),
                    Some("daemon-backed")
                );
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "heartbeat did not merge live metadata into FileRegistry"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    drop(handle);
    drop(occupied);
}

#[tokio::test]
async fn test_explicit_deregister_all_is_idempotent() {
    // `McpServerHandle::shutdown` calls `deregister_all` explicitly
    // before dropping the gateway. Verify both the explicit path and
    // the follow-up Drop produce the same end state and do not double-
    // log / double-remove.
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();

    let runner = make_runner_in(dir.path(), port);
    let entry = ServiceEntry::new("blender", "127.0.0.1", 0);
    let key = entry.key();
    let mut handle = runner.start(entry, None).await.unwrap();

    // Explicit shutdown path — does NOT rely on Drop.
    handle.deregister_all();

    {
        let reg = runner.registry.read().await;
        assert!(
            reg.get(&key).is_none(),
            "explicit deregister_all must remove instance row immediately (issue #718)"
        );
    }

    // Idempotency: a subsequent Drop must be a clean no-op.
    drop(handle);
    let reg = runner.registry.read().await;
    assert!(
        reg.get(&key).is_none(),
        "second deregister (via Drop) must remain a no-op"
    );
    drop(occupied);
}

#[tokio::test]
async fn test_gateway_winner_drop_deregisters_instance_and_sentinel() {
    // Winner path: the handle must carry both the instance key and the
    // `__gateway__` sentinel key, and Drop must purge both rows.
    //
    // We bind a real loopback listener and register the instance under
    // that port so the gateway's startup `probe_and_evict_dead_instances`
    // sweep keeps the row alive. Without this the probe would evict our
    // fake port=0 instance before we could observe it.
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();

    // Keep an instance listener alive for the duration of the test so
    // the gateway's port probe finds it reachable.
    let instance_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let instance_port = instance_listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            // Accept and immediately drop — all we need is a listening socket.
            let _ = instance_listener.accept().await;
        }
    });

    let runner = make_runner_in(dir.path(), gw_port);
    let entry = ServiceEntry::new("maya", "127.0.0.1", instance_port);
    let instance_key = entry.key();
    let handle = runner.start(entry, None).await.unwrap();

    // In CI the port may have been snatched back by another test before
    // `try_bind_port_opt` got there; only assert the sentinel-deregister
    // semantics when we actually won.
    if handle.is_gateway {
        {
            let reg = runner.registry.read().await;
            assert!(
                reg.get(&instance_key).is_some(),
                "instance row must exist before shutdown"
            );
            assert_eq!(
                reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE).len(),
                1,
                "winner must have written one __gateway__ sentinel row"
            );
        }

        drop(handle);

        let reg = runner.registry.read().await;
        assert!(
            reg.get(&instance_key).is_none(),
            "winner Drop must remove the instance row (issue #718)"
        );
        assert_eq!(
            reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE).len(),
            0,
            "winner Drop must remove the __gateway__ sentinel (issue #718)"
        );
    } else {
        // Non-winner fallback — at least the instance row must go.
        drop(handle);
        let reg = runner.registry.read().await;
        assert!(reg.get(&instance_key).is_none());
    }
}

// ── Regression tests for issue #998 follow-up (2026-05-16) ────────────
//
// Maya gateway crashes leave a stale ``__gateway__`` sentinel in the
// FileRegistry. When peers fail to bind the port (TIME_WAIT) the
// election used to read the dead sentinel, decide "same-or-stronger",
// and stay as plain instances forever — even after TIME_WAIT cleared.
// The fix prunes dead entries before reading the sentinel.

/// Inject a ghost ``__gateway__`` sentinel into the registry directory
/// the way an external (now-dead) process would have left it. We bypass
/// ``FileRegistry::register`` deliberately: ``register`` would attach a
/// sentinel file owned by the current process, and ``sentinel_is_dead``
/// short-circuits on locally-held sentinels (returns ``false`` even
/// when the PID lookup says otherwise). Writing ``services.json``
/// directly with ``sentinel_path = None`` and a dead PID falls through
/// to the ``is_pid_alive`` check, which is the path real cross-process
/// ghost rows take after a gateway crash.
fn write_dead_sentinel(
    registry_dir: &std::path::Path,
    host: &str,
    port: u16,
    version: &str,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut entry = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, host, port);
    entry.version = Some(version.to_string());
    // ``u32::MAX`` is guaranteed unused on every platform we ship —
    // ``sysinfo``'s ``System::process(...)`` will report ``None``,
    // ``is_pid_alive`` will return ``false``, and ``prune_dead_entries``
    // will sweep the row.
    entry.pid = Some(u32::MAX);
    entry.sentinel_path = None; // Force the PID path in sentinel_is_dead.

    // services.json schema: a JSON array of ServiceEntry rows at the
    // top level (see ``FileRegistry::reload_from_file``).
    let path = registry_dir.join("services.json");
    let mut f = std::fs::File::create(&path)?;
    f.write_all(
        serde_json::to_string_pretty(&vec![entry])
            .unwrap()
            .as_bytes(),
    )?;
    f.sync_all()?;
    Ok(())
}

#[tokio::test]
async fn test_run_election_prunes_dead_gateway_sentinel_before_resident_lookup() {
    // Set-up: a stale __gateway__ sentinel pointing at our test port,
    // owned by a non-existent PID, written directly to services.json
    // the way an external dead gateway would have left it. The port is
    // NOT bound by anyone, so ``try_bind_port_opt`` will succeed — the
    // assertion is that the ghost row is gone afterwards regardless of
    // which branch ``run_election`` takes.
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();
    write_dead_sentinel(dir.path(), "127.0.0.1", gw_port, "0.99.99").unwrap();

    let runner = make_runner_in(dir.path(), gw_port);

    let outcome = runner.run_election().await.unwrap();

    // After election: the ghost sentinel MUST be gone. The win branch
    // would have written its own sentinel beside it without pruning;
    // the loss branch's resident lookup would have returned the ghost
    // and refused to challenge. Either way, "0.99.99" must not survive.
    let reg = runner.registry.read().await;
    let surviving: Vec<_> = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
    assert!(
        surviving
            .iter()
            .all(|e| e.version.as_deref() != Some("0.99.99")),
        "ghost sentinel from dead PID u32::MAX must have been pruned, \
         survivors: {:?}",
        surviving
            .iter()
            .map(|e| e.version.as_deref().unwrap_or("None"))
            .collect::<Vec<_>>()
    );
    if outcome.is_gateway {
        assert_eq!(
            surviving.len(),
            1,
            "winner must have left exactly one sentinel (its own)"
        );
    }

    // Clean up the abort handles so the test exits cleanly.
    if let Some(abort) = outcome.gateway_abort {
        abort.abort();
    }
    if let Some(abort) = outcome.challenger_abort {
        abort.abort();
    }
}

/// Plant a ``__gateway__`` sentinel owned by the CURRENT process (so
/// ``is_pid_alive`` returns ``true``, ``sentinel_is_dead`` returns
/// ``false`` — exactly the path a live peer that previously held the
/// gateway role takes). This row will SURVIVE ``prune_dead_entries``
/// and must be cleaned up by the WIN-path sentinel sweep.
fn write_live_sentinel(
    registry_dir: &std::path::Path,
    host: &str,
    port: u16,
    version: &str,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut entry = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, host, port);
    entry.version = Some(version.to_string());
    // Current process PID — guaranteed alive for the duration of the
    // test, so ``prune_dead_entries`` will NOT remove this row.
    entry.pid = Some(std::process::id());
    entry.sentinel_path = None;

    let path = registry_dir.join("services.json");
    let mut f = std::fs::File::create(&path)?;
    f.write_all(
        serde_json::to_string_pretty(&vec![entry])
            .unwrap()
            .as_bytes(),
    )?;
    f.sync_all()?;
    Ok(())
}

#[tokio::test]
async fn test_run_election_win_clears_stale_live_owner_sentinels() {
    // Reproduces the "3 Mayas, 3 __gateway__ rows, nobody on 9765"
    // pollution observed in a live session on 2026-05-16. A live peer
    // that previously held the gateway role left its sentinel behind
    // when it lost the role (process is still alive → prune_dead
    // doesn't touch it). The winner of the next election MUST replace
    // those rows instead of co-existing with them, otherwise peers'
    // ``list_instances(__gateway__).next()`` picks up a phantom version
    // and the registry grows one stale row per role rotation.
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();

    // Plant TWO stale sentinels (versions "1.0" and "2.0") owned by
    // our PID, which is unambiguously alive.
    write_live_sentinel(dir.path(), "127.0.0.1", gw_port, "1.0").unwrap();
    {
        // Append a second row by re-writing services.json directly.
        use std::io::Write;
        let mut e1 = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", gw_port);
        e1.version = Some("1.0".to_string());
        e1.pid = Some(std::process::id());
        e1.sentinel_path = None;
        let mut e2 = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", gw_port);
        e2.version = Some("2.0".to_string());
        e2.pid = Some(std::process::id());
        e2.sentinel_path = None;
        let path = dir.path().join("services.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(
            serde_json::to_string_pretty(&vec![e1, e2])
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        f.sync_all().unwrap();
    }

    let runner = make_runner_in(dir.path(), gw_port);

    let outcome = runner.run_election().await.unwrap();

    // Election must have won the free port (no other process is bound).
    assert!(
        outcome.is_gateway,
        "free port + clean registry must produce a winner"
    );

    // Critical assertion: exactly ONE __gateway__ sentinel survives.
    // Pre-fix this was 3 (both ghosts + ours). The winner's sweep
    // (RFC #998 follow-up) drops the two ghosts before registering.
    let reg = runner.registry.read().await;
    let surviving: Vec<_> = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
    assert_eq!(
        surviving.len(),
        1,
        "winner must have left exactly one __gateway__ sentinel, found {} (versions: {:?})",
        surviving.len(),
        surviving
            .iter()
            .map(|e| e.version.as_deref().unwrap_or("None"))
            .collect::<Vec<_>>()
    );
    // The survivor must be ours, not one of the ghosts.
    assert_ne!(
        surviving[0].version.as_deref(),
        Some("1.0"),
        "ghost sentinel v1.0 should have been swept"
    );
    assert_ne!(
        surviving[0].version.as_deref(),
        Some("2.0"),
        "ghost sentinel v2.0 should have been swept"
    );

    if let Some(abort) = outcome.gateway_abort {
        abort.abort();
    }
}

#[tokio::test]
async fn test_run_election_spawns_challenger_when_bind_fails_with_no_resident() {
    // The TIME_WAIT recovery case: bind fails AND after pruning there
    // is no resident sentinel (the dead gateway's sentinel was just
    // dropped because its PID isn't alive). The old code would fall
    // into the "same-or-stronger" branch and return as a plain
    // instance — leaving 9765 dark forever. The fix spawns the
    // challenger loop so we keep polling for the port to free up.
    let dir = tempfile::tempdir().unwrap();
    let gw_port = ephemeral_port();

    // Simulate the kernel holding the port (TIME_WAIT or active LISTEN
    // we don't own) by occupying it with a sibling listener.
    let occupier = std::net::TcpListener::bind(("127.0.0.1", gw_port)).unwrap();

    // Plant a ghost sentinel from a "previous gateway crash" that the
    // election must prune. After pruning there must be no resident.
    write_dead_sentinel(dir.path(), "127.0.0.1", gw_port, "0.1.0").unwrap();

    let runner = make_runner_in(dir.path(), gw_port);

    let outcome = runner.run_election().await.unwrap();

    assert!(
        !outcome.is_gateway,
        "port is held by `occupier` — must not win"
    );
    assert!(
        outcome.challenger_abort.is_some(),
        "bind failed + no resident-after-prune must spawn the challenger loop \
         (regression for #998 follow-up: previously stayed as plain instance forever)"
    );

    // Cleanup so the challenger task does not outlive the test.
    if let Some(abort) = outcome.challenger_abort {
        abort.abort();
    }
    drop(occupier);
}
