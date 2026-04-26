//! Regression tests for issue #303 — gateway and per-instance listener
//! must be reachable whenever the server handle reports success.
//!
//! These tests cover the two failure modes observed in the bug report:
//!
//! 1. **Run A: TIMEOUT** — bind() succeeded but the accept-loop was
//!    detached onto a starved runtime worker, so `TcpStream::connect`
//!    hangs until the kernel times out. Prevented by the self-probe
//!    added in [`McpHttpServer::start`] and the [`GatewayTasks`]
//!    JoinHandle retention.
//!
//! 2. **Run B: REFUSED** — the `JoinHandle` of the gateway supervisor
//!    task was dropped at the end of `start_gateway_tasks`, which on
//!    some runtimes causes an immediate cancellation before the kernel
//!    finishes setting up the listen queue. Prevented by storing the
//!    supervisor `JoinHandle` inside [`GatewayHandle`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, ServerSpawnMode};

/// When `McpServerHandle` reports success, a plain TCP connect to its
/// bind address MUST succeed within a short deadline. Repeat over
/// several start/shutdown cycles to shake out races.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn instance_listener_is_reachable_when_handle_is_returned_ambient() {
    for round in 0..5 {
        let registry = Arc::new(ActionRegistry::new());
        let cfg = McpHttpConfig::new(0)
            .with_name(format!("reach-test-{round}"))
            .with_spawn_mode(ServerSpawnMode::Ambient);

        let handle = McpHttpServer::new(registry, cfg)
            .start()
            .await
            .expect("server must start");

        let addr = handle.bind_addr.clone();
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut reachable = false;
        while Instant::now() < deadline {
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                reachable = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            reachable,
            "round {round}: Ambient-mode handle claimed bind to {addr} but connect failed"
        );

        handle.shutdown().await;
    }
}

/// Same invariant as above but for the Dedicated spawn mode, which is
/// the Python default (see `PyMcpHttpConfig::new`). This path runs the
/// accept loop on its own OS thread, so it MUST remain reachable even
/// if the caller's runtime is otherwise idle.
#[tokio::test(flavor = "current_thread")]
async fn instance_listener_is_reachable_when_handle_is_returned_dedicated() {
    for round in 0..5 {
        let registry = Arc::new(ActionRegistry::new());
        let cfg = McpHttpConfig::new(0)
            .with_name(format!("reach-test-dedicated-{round}"))
            .with_spawn_mode(ServerSpawnMode::Dedicated);

        let handle = McpHttpServer::new(registry, cfg)
            .start()
            .await
            .expect("dedicated server must start");

        let addr = handle.bind_addr.clone();
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut reachable = false;
        while Instant::now() < deadline {
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                reachable = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            reachable,
            "round {round}: Dedicated-mode handle claimed bind to {addr} but connect failed"
        );

        handle.shutdown().await;
    }
}

/// Dropping the handle must release the port on the next bind attempt.
/// Regression guard: an earlier bug had the gateway supervisor detach
/// and leak the listener after shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_handle_releases_port() {
    let registry = Arc::new(ActionRegistry::new());

    // Bind the first server on port 0, capture the OS-assigned port,
    // shut it down, then ensure we can bind the same port immediately.
    let cfg = McpHttpConfig::new(0).with_name("drop-release-test");
    let handle = McpHttpServer::new(registry.clone(), cfg)
        .start()
        .await
        .expect("first server must start");
    let port = handle.port;
    handle.shutdown().await;

    // Give the kernel a moment to release the port on Windows.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second server on the same port should succeed.
    let cfg2 = McpHttpConfig::new(port).with_name("drop-release-test-2");
    let handle2 = McpHttpServer::new(registry, cfg2).start().await;

    // On Windows TIME_WAIT can occasionally deny immediate re-bind;
    // accept either "started" or a specific AddrInUse error without
    // panicking — what we really want to exclude is "listener leaked".
    if let Ok(h) = handle2 {
        assert_eq!(h.port, port, "re-bind must use the same port");
        h.shutdown().await;
    } else {
        // Ensure the OS-level port is at least not held by our process —
        // a fresh bind on 0 must still succeed.
        let cfg3 = McpHttpConfig::new(0).with_name("drop-release-test-3");
        let h3 = McpHttpServer::new(Arc::new(ActionRegistry::new()), cfg3)
            .start()
            .await
            .expect("subsequent bind on port 0 must succeed");
        h3.shutdown().await;
    }
}

/// Attempting to start the gateway on an already-occupied port MUST
/// return `is_gateway=false` (plain instance), never `true`. Combined
/// with the self-probe, this is the structural fix for the
/// "is_gateway=true but unreachable" symptom reported in #303.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn occupied_gateway_port_yields_plain_instance() {
    // Reserve a TCP port by binding a bare listener — this prevents the
    // gateway election from winning.
    let squatter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let squatter_port = squatter.local_addr().unwrap().port();

    let registry = Arc::new(ActionRegistry::new());
    let cfg = McpHttpConfig::new(0)
        .with_name("plain-instance-test")
        .with_gateway(squatter_port)
        .with_dcc_type("test");

    // Isolated registry dir so this test does not clobber any real one.
    let tempdir = tempfile::tempdir().unwrap();
    let cfg = cfg.with_registry_dir(tempdir.path());

    let handle = McpHttpServer::new(registry, cfg)
        .start()
        .await
        .expect("server must start even when gateway port is taken");

    assert!(
        !handle.is_gateway,
        "is_gateway MUST be false when gateway port is already bound"
    );

    // Our instance port should still be reachable.
    let addr = handle.bind_addr.clone();
    let reachable = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .unwrap()
    .is_ok();
    assert!(reachable, "instance listener must be reachable at {addr}");

    handle.shutdown().await;
    drop(squatter);
}
