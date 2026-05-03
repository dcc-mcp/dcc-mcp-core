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
