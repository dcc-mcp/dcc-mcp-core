//! Criterion benchmarks for the instance query pipeline (PIP-2725).
//!
//! Pin latency of the parse → filter → project path:
//!
//! - URI parse throughput
//! - Compact instance JSON serialization (~500B target)
//! - Substring query matching across display_id/version/scene
//! - Full filter pipeline with realistic instance counts (≤100, ≤1000)
//!
//! Run with:
//!
//! ```bash
//! cargo bench -p dcc-mcp-gateway --bench instance_query_bench
//! ```

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dcc_mcp_gateway::gateway::native_resources::instances;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use std::hint::black_box;
use std::time::Duration;

// ── Shared helpers ─────────────────────────────────────────────────────

const STALE_TIMEOUT: Duration = Duration::from_secs(30);

fn make_instance(dcc_type: &str, port: u16, version: &str, status: ServiceStatus) -> ServiceEntry {
    let mut entry = ServiceEntry::new(dcc_type, "127.0.0.1", port);
    entry.version = Some(version.to_string());
    entry.status = status;
    entry
}

fn make_corpus(count: usize) -> Vec<ServiceEntry> {
    let types = ["blender", "maya", "houdini", "photoshop", "substance"];
    let versions = ["4.2.0", "2024.2", "21.0", "25.0", "8.0"];
    (0..count)
        .map(|i| {
            let idx = i % types.len();
            let mut entry = ServiceEntry::new(types[idx], "127.0.0.1", 9000 + (i as u16));
            entry.version = Some(format!("{}.{}", versions[idx], i % 10));
            entry.status = if i % 5 == 0 {
                ServiceStatus::Busy
            } else {
                ServiceStatus::Available
            };
            if i % 3 == 0 {
                entry.scene = Some(format!("/projects/scene_{}.blend", i));
            }
            entry
        })
        .collect()
}

// ── URI parse benchmarks ────────────────────────────────────────────────

fn bench_parse_root(c: &mut Criterion) {
    c.bench_function("instances/parse/root_defaults", |b| {
        b.iter(|| instances::parse(black_box("gateway://instances")))
    });
}

fn bench_parse_with_filters(c: &mut Criterion) {
    let uris = [
        "gateway://instances?dcc_type=blender&query=4.2&limit=10",
        "gateway://instances?dcc_type=maya&status=available&verbose=true&limit=20&offset=5",
        "gateway://instances?query=2024&include_stale=false&limit=5",
    ];
    let mut group = c.benchmark_group("instances/parse/with_filters");
    for (i, uri) in uris.iter().enumerate() {
        group.bench_with_input(BenchmarkId::new("uri", i), uri, |b, uri| {
            b.iter(|| instances::parse(black_box(uri)))
        });
    }
    group.finish();
}

// ── Compact JSON serialization ──────────────────────────────────────────

fn bench_compact_json(c: &mut Criterion) {
    let entry = make_instance("blender", 9876, "4.2.0", ServiceStatus::Available);
    c.bench_function("instances/compact_json/single", |b| {
        b.iter(|| instances::compact_instance_json(black_box(&entry), STALE_TIMEOUT))
    });
}

fn bench_compact_json_bulk(c: &mut Criterion) {
    let corpus = make_corpus(100);
    c.bench_function("instances/compact_json/100_instances", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for entry in &corpus {
                let json = instances::compact_instance_json(black_box(entry), STALE_TIMEOUT);
                total += serde_json::to_string(&json).unwrap().len();
            }
            black_box(total)
        })
    });
}

// ── Query matching benchmarks ───────────────────────────────────────────

fn bench_query_matching(c: &mut Criterion) {
    let corpus = make_corpus(1000);
    let queries = ["blender", "4.2", "2024", "scene_", "nonexistent"];

    c.bench_function("instances/query_match/1000_instances", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for query in queries {
                let ql = query.to_ascii_lowercase();
                total += corpus
                    .iter()
                    .filter(|e| instances::instance_matches_query(black_box(e), black_box(&ql)))
                    .count();
            }
            black_box(total)
        })
    });
}

// ── Full filter pipeline (parse + filter, without GatewayState) ─────────

fn bench_filter_pipeline(c: &mut Criterion) {
    let corpus = make_corpus(100);
    let stale_timeout = STALE_TIMEOUT;

    let mut group = c.benchmark_group("instances/filter_pipeline");

    // dcc_type exact match
    group.bench_function("dcc_type_exact/100_instances", |b| {
        b.iter(|| {
            let count = corpus
                .iter()
                .filter(|e| e.dcc_type.eq_ignore_ascii_case("blender"))
                .count();
            black_box(count)
        })
    });

    // Combined: dcc_type + query + stale check
    group.bench_function("combined_filter/100_instances", |b| {
        b.iter(|| {
            let ql = "4.2".to_ascii_lowercase();
            let count = corpus
                .iter()
                .filter(|e| {
                    e.dcc_type.eq_ignore_ascii_case("blender")
                        && !e.is_stale(stale_timeout)
                        && instances::instance_matches_query(black_box(e), black_box(&ql))
                })
                .count();
            black_box(count)
        })
    });

    // Large corpus
    let large = make_corpus(1000);
    group.bench_function("combined_filter/1000_instances", |b| {
        b.iter(|| {
            let ql = "2024".to_ascii_lowercase();
            let count = large
                .iter()
                .filter(|e| {
                    e.dcc_type.eq_ignore_ascii_case("maya")
                        && !e.is_stale(stale_timeout)
                        && instances::instance_matches_query(black_box(e), black_box(&ql))
                })
                .count();
            black_box(count)
        })
    });

    group.finish();
}

// ── Index health computation ────────────────────────────────────────────

fn bench_index_health(c: &mut Criterion) {
    c.bench_function("instances/index_health", |b| {
        b.iter(|| instances::compute_index_health(black_box(100), black_box(5), STALE_TIMEOUT))
    });
}

// ── Size assertion (not a benchmark, but runs in the benchmark harness) ──

#[test]
fn compact_json_size_target() {
    let corpus = make_corpus(100);
    let mut max_size = 0usize;
    let mut total_size = 0usize;

    for entry in &corpus {
        let json = instances::compact_instance_json(entry, STALE_TIMEOUT);
        let size = serde_json::to_string(&json).unwrap().len();
        max_size = max_size.max(size);
        total_size += size;
    }

    let avg = total_size / corpus.len();
    eprintln!(
        "Compact JSON size: max={}B, avg={}B (target ≤500B/hit)",
        max_size, avg
    );
    assert!(
        avg < 500,
        "Average compact JSON size {}B exceeds 500B target",
        avg
    );
}

criterion_group!(
    benches,
    bench_parse_root,
    bench_parse_with_filters,
    bench_compact_json,
    bench_compact_json_bulk,
    bench_query_matching,
    bench_filter_pipeline,
    bench_index_health,
);
criterion_main!(benches);
