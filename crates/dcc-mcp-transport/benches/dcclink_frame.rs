//! Criterion benchmarks for DccLinkFrame encode/decode (pure CPU, no I/O).

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use dcc_mcp_transport::{DccLinkFrame, DccLinkType};

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("dcclink_frame/encode");

    for body_size in [0, 64, 256, 1024, 4096] {
        let frame = DccLinkFrame {
            msg_type: DccLinkType::Call,
            seq: 42,
            body: vec![0xAB; body_size],
        };

        group.throughput(Throughput::Bytes((4 + 1 + 8 + body_size) as u64));
        group.bench_with_input(BenchmarkId::new("body", body_size), &frame, |b, frame| {
            b.iter(|| {
                let encoded = black_box(frame).encode().unwrap();
                black_box(encoded);
            });
        });
    }
    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("dcclink_frame/decode");

    for body_size in [0, 64, 256, 1024, 4096] {
        let frame = DccLinkFrame {
            msg_type: DccLinkType::Call,
            seq: 42,
            body: vec![0xAB; body_size],
        };
        let encoded = frame.encode().unwrap();

        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("body", body_size),
            &encoded,
            |b, encoded| {
                b.iter(|| {
                    let decoded = DccLinkFrame::decode(black_box(encoded)).unwrap();
                    black_box(decoded);
                });
            },
        );
    }
    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("dcclink_frame/roundtrip");

    for body_size in [0, 64, 256, 1024, 4096] {
        let frame = DccLinkFrame {
            msg_type: DccLinkType::Call,
            seq: 42,
            body: vec![0xAB; body_size],
        };

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("body", body_size), &frame, |b, frame| {
            b.iter(|| {
                let encoded = black_box(frame).encode().unwrap();
                let decoded = DccLinkFrame::decode(&encoded).unwrap();
                black_box(decoded);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_encode, bench_decode, bench_roundtrip);
criterion_main!(benches);
