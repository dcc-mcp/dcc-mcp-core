//! Criterion benchmarks for IPC round-trip performance (IpcChannelAdapter and GracefulIpcChannelAdapter).
//!
//! Measures send_frame/recv_frame latency and throughput over local IPC.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dcc_mcp_transport::{DccLinkFrame, DccLinkType, GracefulIpcChannelAdapter, IpcChannelAdapter};

/// Sample frame with configurable body size.
fn sample_frame(body_size: usize) -> DccLinkFrame {
    DccLinkFrame {
        msg_type: DccLinkType::Call,
        seq: 1,
        body: vec![0xAB; body_size],
    }
}

/// Benchmark IpcChannelAdapter round-trip: client sends, server echoes back.
fn bench_ipc_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc/roundtrip");

    for body_size in [0, 64, 256, 1024] {
        let frame = sample_frame(body_size);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("IpcChannelAdapter", body_size),
            &frame,
            |b, frame| {
                let name_suffix = format!("ipc-{body_size}-{}", std::process::id());

                let mut server =
                    IpcChannelAdapter::create(&format!("bench-{name_suffix}")).unwrap();
                let client = IpcChannelAdapter::connect(&format!("bench-{name_suffix}")).unwrap();
                server.wait_for_client().unwrap();

                let server = Arc::new(parking_lot::Mutex::new(server));
                let client = Arc::new(parking_lot::Mutex::new(client));
                let running = Arc::new(AtomicBool::new(true));

                // Spawn echo thread: server receives and sends back.
                let server_echo = server.clone();
                let running_echo = running.clone();
                let echo_handle = std::thread::spawn(move || {
                    while running_echo.load(Ordering::Relaxed) {
                        let mut s = server_echo.lock();
                        let Ok(recv) = s.recv_frame() else {
                            break;
                        };
                        let reply = DccLinkFrame {
                            msg_type: DccLinkType::Reply,
                            seq: recv.seq,
                            body: recv.body,
                        };
                        if s.send_frame(&reply).is_err() {
                            break;
                        }
                    }
                });

                b.iter(|| {
                    let mut c = client.lock();
                    c.send_frame(frame).unwrap();
                    let reply = c.recv_frame().unwrap();
                    assert_eq!(reply.msg_type, DccLinkType::Reply);
                    assert_eq!(reply.body, frame.body);
                });

                // Shutdown: signal echo thread, then drop to close IPC.
                running.store(false, Ordering::Relaxed);
                drop(client);
                drop(server);
                let _ = echo_handle.join();
            },
        );
    }
    group.finish();
}

/// Benchmark GracefulIpcChannelAdapter round-trip.
fn bench_graceful_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc/roundtrip");

    for body_size in [0, 64, 256, 1024] {
        let frame = sample_frame(body_size);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("GracefulIpcChannelAdapter", body_size),
            &frame,
            |b, frame| {
                let name_suffix = format!("graceful-{body_size}-{}", std::process::id());

                // Use IpcChannelAdapter for the echo side to avoid
                // GracefulIpcChannel's blocking recv not being interruptible.
                // The benchmark measures the Graceful client → Ipc server
                // round-trip, which reflects real usage (DCC host uses
                // Graceful, tool/client uses plain IpcChannel).
                let mut server =
                    IpcChannelAdapter::create(&format!("bench-{name_suffix}")).unwrap();
                let client =
                    GracefulIpcChannelAdapter::connect(&format!("bench-{name_suffix}")).unwrap();
                server.wait_for_client().unwrap();

                let server = Arc::new(parking_lot::Mutex::new(server));
                let client = Arc::new(parking_lot::Mutex::new(client));
                let running = Arc::new(AtomicBool::new(true));

                let server_echo = server.clone();
                let running_echo = running.clone();
                let echo_handle = std::thread::spawn(move || {
                    while running_echo.load(Ordering::Relaxed) {
                        let mut s = server_echo.lock();
                        let Ok(recv) = s.recv_frame() else {
                            break;
                        };
                        let reply = DccLinkFrame {
                            msg_type: DccLinkType::Reply,
                            seq: recv.seq,
                            body: recv.body,
                        };
                        if s.send_frame(&reply).is_err() {
                            break;
                        }
                    }
                });

                b.iter(|| {
                    let mut c = client.lock();
                    c.send_frame(frame).unwrap();
                    let reply = c.recv_frame().unwrap();
                    assert_eq!(reply.msg_type, DccLinkType::Reply);
                    assert_eq!(reply.body, frame.body);
                });

                // Graceful shutdown.
                {
                    let c = client.lock();
                    c.shutdown();
                }
                running.store(false, Ordering::Relaxed);
                drop(client);
                drop(server);
                let _ = echo_handle.join();
            },
        );
    }
    group.finish();
}

/// Benchmark submit_reentrant + pump_pending on GracefulIpcChannelAdapter.
fn bench_submit_reentrant(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc/submit_reentrant");

    group.bench_function("inline_from_affinity_thread", |b| {
        let name = format!("bench-reentrant-inline-{}", std::process::id());
        let mut server = GracefulIpcChannelAdapter::create(&name).unwrap();
        let _client = GracefulIpcChannelAdapter::connect(&name).unwrap();
        server.wait_for_client().unwrap();

        server.bind_affinity_thread();
        let server = Arc::new(server);

        b.iter(|| {
            let val = server.submit_reentrant(|| 42_u64).unwrap();
            assert_eq!(val, 42);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ipc_roundtrip,
    bench_graceful_roundtrip,
    bench_submit_reentrant,
);
criterion_main!(benches);
