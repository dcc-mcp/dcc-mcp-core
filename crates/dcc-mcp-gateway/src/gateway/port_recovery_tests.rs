//! Unit tests for `gateway::port_recovery` (#2405).

use super::*;

use crate::gateway::runner::{ResidentGatewayHealth, probe_resident_gateway_health};

// ── Service-dead port holder recovery (#2405) ──────────────────────

/// Bind an ephemeral port, hand it to a service-dead holder child, and
/// wait until that child is actually serving the port.
///
/// The parent listener is closed only after the child reports it holds the
/// port, so the two never race for the bind.
async fn start_service_dead_holder() -> (u16, std::process::Child) {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let holder = spawn_port_holder(port);
    // The child cannot bind while the parent listener is alive unless
    // SO_REUSEADDR lets it, so stop accepting and close first.
    drop(occupied);
    wait_for_port_holder(port).await;
    (port, holder)
}

#[test]
fn unattributed_port_holder_is_never_reaped() {
    // No pidfile and no autolaunch manifest: the PID holding the port may
    // belong to an unrelated application, so recovery must fall back to
    // waiting rather than terminating it.
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "an unattributed port holder must never be selected for termination"
    );
    drop(occupied);
}

#[tokio::test]
async fn pidfile_attributed_port_holder_is_selected_for_reaping() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;
    let pidfile = dir.path().join("gateway.pid");
    dcc_mcp_gateway_ensure::write_pidfile(&pidfile, holder.id()).unwrap();

    assert_eq!(
        service_dead_port_holder_pid(port, dir.path(), Some(&pidfile)),
        Some(holder.id())
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_attributes_the_port_holder() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    let manifest = serde_json::json!({
        "pid": holder.id(),
        "port": port,
        "executable": "dcc-mcp-server",
    });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert_eq!(
        service_dead_port_holder_pid(port, dir.path(), None),
        Some(holder.id())
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_for_another_pid_does_not_attribute() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    let manifest = serde_json::json!({ "pid": holder.id() + 1 });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "a manifest naming a different PID must not authorize a kill"
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_for_another_port_does_not_attribute() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    // The manifest names the holder but a different port, so it is evidence
    // that *some* launcher started a gateway, not that this process holds the
    // port being recovered. Attributing on the PID alone would let a stale
    // manifest — or a recycled PID — authorize killing an unrelated process.
    // `+ 1` would overflow at the top of the ephemeral range.
    let other_port = port.checked_add(1).unwrap_or(port - 1);
    let manifest = serde_json::json!({ "pid": holder.id(), "port": other_port });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "a manifest for another port must not authorize a kill"
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn service_dead_holder_is_reaped_and_the_port_is_released() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let (port, mut holder) = start_service_dead_holder().await;

    // The holder accepts TCP but never answers HTTP — exactly the
    // adversarial state from #2405. The bind fails while it lives.
    assert!(
        try_bind_port_opt("127.0.0.1", port).await.is_none(),
        "the service-dead holder must still own the port before recovery"
    );
    assert_eq!(
        probe_resident_gateway_health("127.0.0.1", port, Duration::from_millis(300)).await,
        ResidentGatewayHealth::Unhealthy,
        "a holder that never answers must probe as unhealthy"
    );

    let released =
        reap_service_dead_port_holder("127.0.0.1", port, holder.id(), Duration::from_secs(10))
            .await;

    assert!(released, "reaping the holder must release the port");
    // Collect the child. Until its parent waits, an exited child keeps its
    // process-table slot on Unix, so the assertion below would be checking
    // zombie bookkeeping instead of whether the holder is gone.
    let _ = holder.try_wait();
    assert!(
        !dcc_mcp_gateway_ensure::is_process_alive(holder.id()),
        "the stale holder must no longer be alive"
    );
    assert!(
        try_bind_port_opt("127.0.0.1", port).await.is_some(),
        "a healthy challenger can now bind the port"
    );
}

#[tokio::test]
async fn reaping_a_nonexistent_holder_reports_failure_without_panicking() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let (port, holder) = start_service_dead_holder().await;

    // Stopping an already-dead PID is idempotent, but the port stays held
    // by the live holder, so the release can never be confirmed.
    let released =
        reap_service_dead_port_holder("127.0.0.1", port, 9_999_999, Duration::from_millis(300))
            .await;

    assert!(
        !released,
        "release must not be reported while the port is still held"
    );
    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

/// Spawn a helper process that binds `port` and accepts connections
/// without ever responding — the "live but service-dead" holder.
///
/// The helper runs as a filtered libtest case, so the filter must be the
/// fully-qualified test path with `--exact`; a bare name matches nothing
/// and the child exits without binding.
fn spawn_port_holder(port: u16) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "gateway::port_recovery::port_recovery_tests::port_holder_child_process",
            "--exact",
            "--nocapture",
        ])
        .env("DCC_MCP_TEST_PORT_HOLDER_PORT", port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

/// Block until the port-holder child has actually bound `port`.
///
/// Spawning a process does not mean it has reached `bind()`, so tests must
/// wait rather than assume. Panics with a bounded deadline so a broken
/// helper fails the test instead of hanging the suite.
async fn wait_for_port_holder(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if dcc_mcp_gateway_ensure::listener_pids_on_port(port)
            .first()
            .is_some_and(|pid| dcc_mcp_gateway_ensure::is_process_alive(*pid))
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "port holder child never bound port {port}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Child-process body for the service-dead port holder.
///
/// The test harness only runs this as a real holder when the parent set
/// `DCC_MCP_TEST_PORT_HOLDER_PORT`; in a normal test run the variable is
/// absent and this returns immediately so the suite stays green.
#[test]
fn port_holder_child_process() {
    let Ok(raw_port) = std::env::var("DCC_MCP_TEST_PORT_HOLDER_PORT") else {
        return;
    };
    let Ok(port) = raw_port.parse::<u16>() else {
        return;
    };

    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .expect("port holder must bind the contested port");

    // Accept connections and hold them open without ever writing a
    // response: clients connect successfully but never get an HTTP reply,
    // which is precisely the adversarial state described in #2405.
    let mut accepted = Vec::new();
    loop {
        match listener.accept() {
            Ok((stream, _peer)) => accepted.push(stream),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
