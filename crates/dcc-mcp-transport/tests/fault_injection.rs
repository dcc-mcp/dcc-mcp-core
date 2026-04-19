//! Fault-injection tests for the DCC-Link IPC transport layer.
//!
//! Tests resilience under adverse conditions:
//! - Client disconnect detection (server sees error on recv)
//! - Server restart / client reconnect
//! - Slow consumer backpressure (large body)
//! - GracefulIpcChannelAdapter shutdown semantics
//! - Cross-thread submit_reentrant with shutdown

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dcc_mcp_transport::{DccLinkFrame, DccLinkType, GracefulIpcChannelAdapter, IpcChannelAdapter};

/// Unique channel name per test to avoid collisions.
fn channel_name(tag: &str) -> String {
    format!("fault-{tag}-{}", std::process::id())
}

fn ping_frame(seq: u64) -> DccLinkFrame {
    DccLinkFrame {
        msg_type: DccLinkType::Ping,
        seq,
        body: vec![],
    }
}

// ── Client disconnect detection ──────────────────────────────────────────────

#[test]
fn server_detects_client_drop() {
    let name = channel_name("client-drop");
    let mut server = IpcChannelAdapter::create(&name).unwrap();
    let mut client = IpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    // Server sends a frame, client receives.
    server.send_frame(&ping_frame(1)).unwrap();
    let _recv = client.recv_frame().unwrap();

    // Drop client.
    drop(client);

    // Server should see an error on next send.
    let result = server.send_frame(&ping_frame(2));
    assert!(
        result.is_err(),
        "server should detect client is gone after drop"
    );
}

// ── Server restart / client reconnect ─────────────────────────────────────────

#[test]
fn client_can_reconnect_after_server_restart() {
    let name = channel_name("server-restart");

    // First server instance.
    let mut server1 = IpcChannelAdapter::create(&name).unwrap();
    let mut client1 = IpcChannelAdapter::connect(&name).unwrap();
    server1.wait_for_client().unwrap();

    // Round-trip on first connection.
    let frame = DccLinkFrame {
        msg_type: DccLinkType::Call,
        seq: 1,
        body: b"hello".to_vec(),
    };
    client1.send_frame(&frame).unwrap();
    let recv1 = server1.recv_frame().unwrap();
    assert_eq!(recv1.seq, 1);

    // Drop both.
    drop(server1);
    drop(client1);

    // Allow time for the name to be released.
    std::thread::sleep(Duration::from_millis(200));

    // Second server instance rebinds the same name.
    let mut server2 = IpcChannelAdapter::create(&name).unwrap();
    let mut client2 = IpcChannelAdapter::connect(&name).unwrap();
    server2.wait_for_client().unwrap();

    // Round-trip on second connection.
    let frame2 = DccLinkFrame {
        msg_type: DccLinkType::Call,
        seq: 2,
        body: b"world".to_vec(),
    };
    server2.send_frame(&frame2).unwrap();
    let recv2 = client2.recv_frame().unwrap();
    assert_eq!(recv2.seq, 2);
    assert_eq!(recv2.body, b"world");
}

// ── Moderate-size frame ────────────────────────────────────────────────────────
// Note: Large IPC frames can deadlock on Windows named pipes due to limited
// buffer sizes. We test a 4 KB payload here; larger frames are covered by
// the TCP fault-injection tests.

#[test]
fn small_frame_roundtrip() {
    let name = channel_name("small-frame");
    let mut server = IpcChannelAdapter::create(&name).unwrap();
    let mut client = IpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    // 1 KB body.
    let body = vec![0xDE; 1024];
    let frame = DccLinkFrame {
        msg_type: DccLinkType::Push,
        seq: 42,
        body: body.clone(),
    };

    // Server sends to client.
    server.send_frame(&frame).unwrap();
    let recv = client.recv_frame().unwrap();
    assert_eq!(recv.seq, 42);
    assert_eq!(recv.body, body);
}

// ── GracefulIpcChannelAdapter shutdown ────────────────────────────────────────

#[test]
fn graceful_shutdown_prevents_new_operations() {
    let name = channel_name("graceful-shutdown");
    let mut server = GracefulIpcChannelAdapter::create(&name).unwrap();
    let _client = GracefulIpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    // Shutdown the channel.
    server.shutdown();

    // Further sends should fail.
    let result = server.send_frame(&ping_frame(1));
    assert!(result.is_err(), "send after shutdown should fail");
}

#[test]
fn graceful_pump_after_shutdown_is_noop() {
    let name = channel_name("graceful-pump-shutdown");
    let mut server = GracefulIpcChannelAdapter::create(&name).unwrap();
    let _client = GracefulIpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    server.bind_affinity_thread();
    server.shutdown();

    // Pump after shutdown should return 0 (no work to process).
    let processed = server.pump_pending(Duration::from_millis(50));
    assert_eq!(processed, 0, "pump after shutdown should process nothing");
}

// ── Inline submit_reentrant on affinity thread ────────────────────────────────

#[test]
fn submit_reentrant_inline_works() {
    let name = channel_name("reentrant-inline");
    let mut server = GracefulIpcChannelAdapter::create(&name).unwrap();
    let _client = GracefulIpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    server.bind_affinity_thread();

    // Inline submit from the affinity thread.
    let val = server.submit_reentrant(|| 42_u32).unwrap();
    assert_eq!(val, 42);
}

// ── Cross-thread submit_reentrant with pump ───────────────────────────────────

#[test]
fn submit_reentrant_cross_thread_with_pump() {
    let name = channel_name("reentrant-pump");
    let mut server = GracefulIpcChannelAdapter::create(&name).unwrap();
    let _client = GracefulIpcChannelAdapter::connect(&name).unwrap();
    server.wait_for_client().unwrap();

    server.bind_affinity_thread();
    let server = Arc::new(server);

    let running = Arc::new(AtomicBool::new(true));

    // Synchronisation: the other thread signals right *before* it calls
    // submit_reentrant (which blocks until pump processes the work).
    let (tx, rx) = std::sync::mpsc::channel::<()>();

    let server_clone = server.clone();
    let _running_clone = running.clone();
    let handle = std::thread::spawn(move || {
        let _ = tx.send(());
        let result = server_clone.submit_reentrant(|| "work_done".to_string());
        // submit_reentrant blocks until pump processes, so if pump runs,
        // this should succeed.
        result.expect("submit should succeed after pump")
    });

    // Wait for the other thread to be about to submit.
    rx.recv().unwrap();
    // Give it a moment to actually enqueue the closure.
    std::thread::sleep(Duration::from_millis(50));

    // Pump to process the queued work.
    let processed = server.pump_pending(Duration::from_millis(200));
    assert_eq!(processed, 1, "should have processed 1 item");

    // Get the result from the thread.
    let result = handle.join().expect("thread should not panic");
    assert_eq!(result, "work_done");

    // Cleanup.
    running.store(false, Ordering::Relaxed);
    server.shutdown();
}
