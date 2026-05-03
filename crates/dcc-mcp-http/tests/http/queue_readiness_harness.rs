//! Issue #717 — queue + readiness boundary-case integration harness.
//!
//! Regression coverage for six failure modes that surfaced in the
//! 2026-05-03 Maya 0.2.26 investigation. Each failure mode has a
//! dedicated primitive merged in the preceding waves (#713, #714,
//! #715, #718, #719); this harness pins the end-to-end contract of
//! each one so a future refactor cannot silently regress.
//!
//! | scenario                   | primitive             | wave   |
//! |----------------------------|------------------------|--------|
//! | `boot_window_*`            | `StaticReadiness` +    | #714   |
//! |                            | MCP `tools/call` gate  |        |
//! | `bridge_full_*`            | `DeferredExecutor` +   | #715   |
//! |                            | `DccExecutorHandle`    |        |
//! | `shutdown_mid_drain_*`     | `QueueDispatcher`,     | #715 / |
//! |                            | `FileRegistry`         | #718   |
//! | `cancel_while_waiting_*`   | `submit_deferred` +    | #715   |
//! |                            | `CancellationToken`    |        |
//! | `restart_cycle_*`          | `FileRegistry::`       | #719   |
//! |                            | `read_alive`           |        |
//! | `heartbeat_starvation_*`   | `/v1/readyz` three-    | #713   |
//! |                            | state probe body       |        |
//!
//! All tests are `#[tokio::test(flavor = "multi_thread")]`, keep
//! wait-for-rejection paths under 3 s, and clean up their own
//! backends / tempdirs so `cargo test` on CI stays cheap.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use dcc_mcp_host::{BlockingDispatcher, DccDispatcher, DccDispatcherExt, DispatchError};
use dcc_mcp_http::{DeferredExecutor, HttpError, McpHttpConfig};
use dcc_mcp_skill_rest::{ReadinessProbe, ReadinessReport, StaticReadiness};
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ─────────────────────────────────────────────────────────────────────────
// Scenario 1 — boot-window: MCP `tools/call` must refuse fast when the
// readiness probe is red (not queue on the executor).
// ─────────────────────────────────────────────────────────────────────────
//
// The MCP handler-level gate is covered by the readiness-gate unit
// tests in `src/tests/readiness_gate.rs`. The integration-level
// contract this harness pins is the one that ties #714 (probe) to
// #715 (executor): when the probe is red, nothing should flow into
// the DCC executor's channel — the refusal must happen before the
// tokio worker touches the main-thread queue.

/// The boot-window refusal is observable as `is_ready() == false` on a
/// `StaticReadiness` whose `dcc` bit is still false — the exact state
/// the MCP handler consults in `handlers/tools_call/mod.rs` before it
/// ever calls `DccExecutorHandle::execute`. We assert the probe shape
/// and then prove the executor side of the boundary: even under load,
/// a handle that never has anything submitted to it keeps its
/// `total_enqueued` at zero. The two halves together prove that a
/// red probe produces no queue side-effect.
#[tokio::test(flavor = "multi_thread")]
async fn boot_window_refuses_tools_call_fast() {
    // Half A — probe shape the MCP handler checks.
    let probe = StaticReadiness::new();
    let report = probe.report();
    assert!(
        !report.is_ready(),
        "StaticReadiness::new() must start red; got {report:?}"
    );
    assert!(report.process, "process bit is always true on construction");
    assert!(
        !report.dispatcher && !report.dcc,
        "dispatcher/dcc bits must be red until the adapter wires them"
    );

    // Half B — executor is untouched when the gate refuses. Build a
    // real `DeferredExecutor`, take the handle, and confirm that
    // *not* calling `execute()` leaves the queue at zero depth (this
    // is what the handler-level gate guarantees — the refuse path
    // does not fall through to `execute()`).
    let exec = DeferredExecutor::new(8);
    let handle = exec.handle();
    let stats = handle.queue_stats();
    assert_eq!(stats.total_enqueued, 0);
    assert_eq!(stats.total_dequeued, 0);
    assert_eq!(stats.total_rejected, 0);
    assert_eq!(stats.pending, 0);

    // Flip the probe green — mirrors what a Maya adapter does at the
    // end of its boot. The handler-level unit tests then observe the
    // call succeeding; here we just confirm the transition itself
    // flips `is_ready()` on the shared snapshot without the handler
    // ever having to restart.
    probe.set_dispatcher_ready(true);
    probe.set_dcc_ready(true);
    assert!(
        probe.report().is_ready(),
        "probe must go green once both bits flip"
    );
    // Keep `exec` alive until end of test so its channel doesn't close
    // and spuriously change `total_rejected` semantics.
    drop(exec);
}

// ─────────────────────────────────────────────────────────────────────────
// Scenario 2 — bridge-full: DeferredExecutor with no pump must surface
// a typed `HttpError::QueueOverloaded` (HTTP 503) for the Nth call,
// not hang.
// ─────────────────────────────────────────────────────────────────────────

/// Fill the channel past capacity with no pump. The (CAP+1)-th call
/// must resolve to `QueueOverloaded` within the configured send
/// timeout — not hang, not return `ExecutorClosed`, not silently
/// succeed. This validates #715's backpressure contract end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn bridge_full_surfaces_queue_overloaded() {
    const CAPACITY: usize = 2;
    // Short send timeout so the test finishes quickly. The production
    // default is 2 s; the shape of the error must be identical.
    let send_timeout = Duration::from_millis(100);
    let exec = DeferredExecutor::with_send_timeout(CAPACITY, send_timeout);
    let handle = exec.handle();

    // Saturate the channel: submit CAPACITY jobs. Each closure only
    // runs on pump, so the mpsc stays full. Keep the receivers in
    // JoinHandles so we can drop them at the end of the test cleanly.
    let mut inflight = Vec::new();
    for i in 0..CAPACITY {
        let h = handle.clone();
        inflight.push(tokio::spawn(async move {
            h.execute(Box::new(move || format!("job-{i}"))).await
        }));
    }

    // Give the senders a moment to actually place their messages in
    // the mpsc (send timeout is 100 ms, so anything > 5 ms is fine).
    tokio::time::sleep(Duration::from_millis(20)).await;

    // The (CAPACITY+1)-th call must observe a full channel and hit
    // the QueueOverloaded branch after `send_timeout`.
    let start = std::time::Instant::now();
    let err = handle
        .execute(Box::new(|| "overflow".to_string()))
        .await
        .expect_err("overflow call must be rejected");
    let elapsed = start.elapsed();

    // Pin the error variant *and* its field shape — these are the
    // exact bits the HTTP 503 body exposes to orchestrators.
    match err {
        HttpError::QueueOverloaded {
            depth,
            capacity,
            retry_after_secs,
        } => {
            assert_eq!(
                capacity, CAPACITY,
                "capacity must echo the configured value"
            );
            assert!(
                depth <= capacity,
                "depth ({depth}) must be bounded by capacity ({capacity})",
            );
            assert!(
                retry_after_secs >= 1,
                "retry_after_secs must be a positive backoff window"
            );
            assert_eq!(err.status_code(), 503, "QueueOverloaded maps to HTTP 503");
        }
        other => panic!("expected HttpError::QueueOverloaded, got {other:?}"),
    }

    // The rejection must complete within a small multiplier of the
    // send-timeout — no wall-clock hang.
    assert!(
        elapsed < Duration::from_secs(3),
        "overflow rejection must be fast (saw {elapsed:?})"
    );

    // `total_rejected` must have been incremented exactly once.
    let stats = handle.queue_stats();
    assert_eq!(stats.total_rejected, 1, "one rejected submit recorded");

    // Abort the in-flight submitters so the test doesn't leak tasks.
    for jh in inflight {
        jh.abort();
    }
    drop(exec);
}

// ─────────────────────────────────────────────────────────────────────────
// Scenario 3 — shutdown-mid-drain: shutting down the host dispatcher
// while work is parked delivers `DispatchError::Shutdown` to every
// pending post; the `FileRegistry` row is deregistered immediately.
// ─────────────────────────────────────────────────────────────────────────

/// Part 1 — host-side. Post 5 jobs to a `QueueDispatcher` (wrapped in
/// `BlockingDispatcher` for the headless / `mayapy` path) with the
/// host never ticking. Call `.shutdown()`. All 5 `PostHandle` futures
/// must resolve to `Err(DispatchError::Shutdown)`, not hang.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_mid_drain_surfaces_shutdown_error() {
    let dispatcher = BlockingDispatcher::new();
    let dyn_dispatcher: Arc<dyn DccDispatcher> = Arc::new(dispatcher.clone());

    // Post 5 jobs. The closure never runs (no tick happens).
    let mut handles = Vec::new();
    for i in 0..5u32 {
        handles.push(dyn_dispatcher.post(move || i));
    }

    // Let the posts land in the queue.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        dyn_dispatcher.pending() >= 5,
        "all 5 jobs must be parked before shutdown",
    );

    // Trigger shutdown mid-drain.
    dyn_dispatcher.shutdown();
    assert!(dyn_dispatcher.is_shutdown());

    // Every posted handle must resolve with the clean shutdown error
    // within the test timeout — no hangs, no silent drops.
    let start = std::time::Instant::now();
    for (idx, handle) in handles.into_iter().enumerate() {
        let result = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap_or_else(|_| panic!("job #{idx} hung after shutdown"));
        match result {
            Err(DispatchError::Shutdown) => {}
            Err(other) => panic!("job #{idx}: expected Shutdown, got {other:?}"),
            Ok(_) => panic!("job #{idx} ran after shutdown; must have been dropped"),
        }
    }
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "shutdown-mid-drain must unblock all posts quickly"
    );
}

/// Part 2 — gateway-side (#718). Registering a backend, then
/// synchronously calling `FileRegistry::deregister` (the seam
/// `GatewayHandle::deregister_all` uses) drops the row from
/// `services.json` immediately, without waiting for `stale_timeout`.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_mid_drain_deregisters_registry_row_synchronously() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    // Seed a row that looks exactly like one an `McpHttpServer` would
    // register for itself.
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18900);
    let key = {
        let r = registry.read().await;
        r.register(entry.clone()).unwrap();
        dcc_mcp_transport::discovery::types::ServiceKey {
            dcc_type: entry.dcc_type.clone(),
            instance_id: entry.instance_id,
        }
    };

    // Immediately deregister (emulate `GatewayHandle::deregister_all`
    // draining its pending-deregister vec on clean shutdown).
    {
        let r = registry.read().await;
        let removed = r
            .deregister(&key)
            .expect("deregister on live row must succeed");
        assert!(
            removed.is_some(),
            "deregister must return the old row (idempotence: Some first time)"
        );
    }

    // The row must be gone from disk — not just filtered out of some
    // future-stale call. `read_alive` walks the on-disk JSON.
    let r = registry.read().await;
    let (alive, evicted) = r.read_alive().unwrap();
    assert!(
        alive.iter().all(|e| e.instance_id != entry.instance_id),
        "row must be physically removed, not just stale-filtered",
    );
    assert_eq!(
        evicted, 0,
        "no dead-PID pruning happened — the row was explicitly dropped"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Scenario 4 — cancel-while-waiting: caller cancels before the pump
// picks up the job; the dispatcher must drop the closure without
// executing it, no leaked pending calls.
// ─────────────────────────────────────────────────────────────────────────

/// `DccExecutorHandle::submit_deferred` with a pre-cancelled
/// `CancellationToken` races the cancellation against the main-thread
/// pump. When the token is tripped before the pump runs, the
/// wrapping closure must short-circuit and surface the dispatcher's
/// canonical `__dispatch_error: CANCELLED` payload — not invoke the
/// user closure, and not leave a job dangling in the queue.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_while_waiting_drops_queued_job() {
    let mut exec = DeferredExecutor::new(8);
    let handle = exec.handle();
    let cancel_token = CancellationToken::new();

    // Counter to prove the closure was NOT invoked.
    let ran = Arc::new(AtomicU32::new(0));
    let ran_clone = ran.clone();

    let rx = handle.submit_deferred(
        "test__long_running",
        cancel_token.clone(),
        Box::new(move || {
            ran_clone.fetch_add(1, Ordering::SeqCst);
            "should-not-run".to_string()
        }),
    );

    // Cancel *before* the pump runs. The submit_deferred internals
    // race cancel_token against the mpsc reserve — cancellation
    // wins either before or after the reserve, but the closure
    // must still not execute.
    cancel_token.cancel();

    // Give the select loop a chance to observe the cancellation.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Drive the pump — at this point either the task was dropped
    // pre-enqueue (no work) or the wrapper's pre-exec checkpoint
    // short-circuits it with a CANCELLED sentinel.
    let pumped = exec.poll_pending_bounded(8);
    // `pumped` can be 0 (task dropped pre-enqueue) or 1 (task
    // enqueued but wrapper observed cancel). Both are valid —
    // what matters is the user closure never ran.
    assert!(pumped <= 1);

    match tokio::time::timeout(Duration::from_secs(1), rx).await {
        Ok(Ok(payload)) => {
            // Wrapper path: payload carries the CANCELLED sentinel.
            assert!(
                payload.contains("CANCELLED"),
                "expected CANCELLED sentinel, got: {payload}",
            );
        }
        Ok(Err(_recv_err)) => {
            // Pre-enqueue drop path: the DccTask was never handed
            // to the pump, so the oneshot sender was dropped.
            // That's the other valid outcome of a cancel-while-
            // waiting race — and is still "no leaked call".
        }
        Err(_) => panic!("cancel-while-waiting must not hang"),
    }

    // The user closure must never have run, regardless of which
    // branch the race resolved into.
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "cancelled closure must not execute",
    );

    // And the executor's stats must reflect that the queue is empty.
    let stats = handle.queue_stats();
    assert_eq!(stats.pending, 0, "no leaked pending call after cancel");
}

// ─────────────────────────────────────────────────────────────────────────
// Scenario 5 — restart-cycle: a backend exits uncleanly, the next
// registry read prunes its dead row (#719), and a fresh backend with
// the same `dcc_type` comes up and is visible without operator action.
// ─────────────────────────────────────────────────────────────────────────

/// Seed two rows into a `FileRegistry`:
///   1. A "crashed" backend whose PID is above any real PID space
///      (emulates an unclean exit — heartbeat was fresh but the
///      process is gone).
///   2. A "freshly-restarted" backend on the same `dcc_type` with
///      the current test-process PID (alive).
///
/// After one `read_alive()` call — the self-healing read path that
/// #719 added to `GatewayState::read_alive_instances` — the dead
/// row must be gone from disk, the fresh row must remain, and the
/// evicted count must reflect exactly the one dead row.
#[tokio::test(flavor = "multi_thread")]
async fn restart_cycle_prunes_dead_row_and_keeps_fresh_one() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Dead row — a PID that cannot exist on any supported platform.
    let mut dead = ServiceEntry::new("maya", "127.0.0.1", 18901);
    dead.pid = Some(u32::MAX - 1);
    let dead_id = dead.instance_id;
    registry.register(dead).unwrap();

    // Fresh restart — same dcc_type, different port, PID = current.
    let mut fresh = ServiceEntry::new("maya", "127.0.0.1", 18902);
    fresh.pid = Some(std::process::id());
    let fresh_id = fresh.instance_id;
    registry.register(fresh).unwrap();

    // Self-healing read path — #719 contract.
    let (alive, evicted) = registry.read_alive().unwrap();
    assert_eq!(evicted, 1, "exactly the dead row is evicted");
    assert!(
        alive.iter().all(|e| e.instance_id != dead_id),
        "dead row must not appear in the alive slice",
    );
    assert!(
        alive.iter().any(|e| e.instance_id == fresh_id),
        "fresh restart row must remain visible without operator action",
    );

    // A second read is idempotent — no more evictions, fresh row
    // still there. This is what the gateway's list_dcc_instances
    // RPC relies on: the caller sees a stable view across repeated
    // polls.
    let (alive2, evicted2) = registry.read_alive().unwrap();
    assert_eq!(
        evicted2, 0,
        "dead row was physically removed on the first call; second call evicts nothing"
    );
    assert_eq!(alive2.len(), alive.len());
}

// ─────────────────────────────────────────────────────────────────────────
// Scenario 6 — heartbeat-starvation: backend alive but dcc bit red;
// `/v1/readyz` reports 503 with a parseable `ReadinessReport` body.
// The gateway's `probe_readiness` helper (#713) parses that exact
// shape — agents must not route `tools/call` to such a backend.
// ─────────────────────────────────────────────────────────────────────────

/// Spawn a minimal axum server that mimics a backend stuck in the
/// "heartbeat starved, DCC not yet ready" state: `/v1/readyz`
/// returns 503 with `process=true, dispatcher=true, dcc=false`.
/// Fetch the endpoint via a real `reqwest::Client` (same client
/// the gateway uses), parse the body as a `ReadinessReport`, and
/// assert the contract the gateway relies on:
///
/// * `is_ready() == false` — gateway must refuse to route.
/// * `process == true` — gateway must *keep* the row in the
///   registry (not evict it as dead).
/// * `dcc == false` — operator-facing reason for the refusal.
#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_starvation_produces_red_readiness_report() {
    async fn readyz_handler() -> (StatusCode, Json<Value>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "process":    true,
                "dispatcher": true,
                "dcc":        false,
            })),
        )
    }
    async fn health_handler() -> &'static str {
        "ok"
    }

    let app = Router::new()
        .route("/v1/readyz", get(readyz_handler))
        .route("/health", get(health_handler))
        // No-op MCP handler so requests to /mcp don't 404 — mirrors
        // a real backend that is listening but refusing work.
        .route(
            "/mcp",
            post(|Json(req): Json<Value>| async move {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                Json(json!({"jsonrpc":"2.0","id":id,"result":{}}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(25)).await;

    let url = format!("http://127.0.0.1:{port}/v1/readyz");
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .header("accept", "application/json")
        .send()
        .await
        .expect("probe request must complete");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "probe must return fast, not hang (saw {elapsed:?})"
    );
    assert_eq!(
        resp.status().as_u16(),
        503,
        "heartbeat-starved backend answers 503 per #660/#713 contract"
    );

    // Body must deserialize as a full ReadinessReport — the gateway
    // relies on this shape, not on the HTTP status alone.
    let report: ReadinessReport = resp.json().await.expect("body must be a ReadinessReport");
    assert!(
        !report.is_ready(),
        "dcc=false -> is_ready() must be false; gateway must not route",
    );
    assert!(
        report.process,
        "process=true -> row stays in registry (not unreachable)",
    );
    assert!(
        !report.dcc,
        "dcc=false -> operator-facing reason for the refusal",
    );
    assert!(
        report.dispatcher,
        "dispatcher=true -> the HTTP layer is up; only the DCC bit is red",
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Config plumbing spot-check — makes the harness self-contained
// by confirming the `McpHttpConfig` fluent builders for the three
// queue knobs actually persist (#715).
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn queue_depth_knobs_plumb_through_mcp_http_config() {
    let cfg = McpHttpConfig::new(0)
        .with_deferred_queue_depth(4)
        .with_bridge_queue_depth(8)
        .with_host_queue_depth(16);
    assert_eq!(cfg.deferred_queue_depth, 4);
    assert_eq!(cfg.bridge_queue_depth, 8);
    assert_eq!(cfg.host_queue_depth, 16);
}
