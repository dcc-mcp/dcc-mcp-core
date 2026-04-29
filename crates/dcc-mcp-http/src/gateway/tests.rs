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
