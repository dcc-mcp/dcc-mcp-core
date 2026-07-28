//! Criterion benchmarks for the search-scoring strategy seam (issue #765).
//!
//! These benchmarks track the raw scorer throughput and the warm
//! [`CapabilityIndex`] search pipeline. Results are diagnostic trends,
//! not deterministic merge gates.
//!
//! Run with:
//!
//! ```bash
//! cargo bench -p dcc-mcp-gateway --bench search_bench
//! ```

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dcc_mcp_gateway::{
    ScorerFactory, SearchMode, SearchQuery, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer,
    capability::{CapabilityIndex, CapabilityRecord, IndexSnapshot, InstanceFingerprint, search},
    capability_service::search_service,
};
use std::{collections::BTreeMap, hint::black_box, sync::Arc};

// ---------------------------------------------------------------------------
// Shared corpus
// ---------------------------------------------------------------------------

const CANDIDATES: &[&str] = &[
    "create_sphere",
    "delete_sphere",
    "export_fbx",
    "import_obj",
    "render_scene",
    "save_scene",
    "open_scene",
    "close_scene",
    "set_keyframe",
    "remove_keyframe",
    "bake_animation",
    "play_animation",
    "stop_animation",
    "load_plugin",
    "unload_plugin",
    "list_plugins",
    "get_selection",
    "set_selection",
    "clear_selection",
    "duplicate_object",
];

const QUERIES: &[&str] = &[
    "sphere",
    "anim",
    "scene",
    "sel",
    "creat_spher", // intentional typo — fuzzy must survive
    "plugin",
];

// ---------------------------------------------------------------------------
// Helper: score every (query, candidate) pair once
// ---------------------------------------------------------------------------

fn score_all(scorer: &dyn StrategyScorer) -> f32 {
    let mut total = 0.0f32;
    for q in QUERIES {
        for c in CANDIDATES {
            total += scorer.score(black_box(q), black_box(c));
        }
    }
    total
}

fn capability_records(size: usize, instance_count: usize) -> Vec<CapabilityRecord> {
    (0..size)
        .map(|i| {
            let dcc = match i % 4 {
                0 => "maya",
                1 => "blender",
                2 => "photoshop",
                _ => "customhost",
            };
            let family = match i % 6 {
                0 => (
                    "modeling",
                    "create_poly_sphere",
                    "Create a polygon sphere primitive.",
                ),
                1 => (
                    "lookdev",
                    "assign_material",
                    "Assign material and lookdev data.",
                ),
                2 => (
                    "uv",
                    "unwrap_uv_shells",
                    "Unwrap UV shells for texture export.",
                ),
                3 => (
                    "export",
                    "export_fbx",
                    "Export selected assets to FBX destination path.",
                ),
                4 => (
                    "render",
                    "render_preview",
                    "Render a preview frame for review.",
                ),
                _ => ("layers", "select_layer", "Select a layer or document node."),
            };
            let iid = uuid::Uuid::from_u128((i % instance_count) as u128 + 1);
            CapabilityRecord::new(
                format!("{dcc}.{:08x}.{}_{}", i, family.0, i),
                format!("{}_{}", family.1, i),
                format!("{}_{}", family.1, i),
                Some(format!("{dcc}-{}", family.0)),
                family.2,
                vec![family.0.to_string(), format!("schema:field_{}", i % 17)],
                dcc.to_string(),
                iid,
                true,
                true,
                None,
            )
        })
        .collect()
}

fn capability_snapshot(size: usize) -> IndexSnapshot {
    let records = capability_records(size, size);
    IndexSnapshot {
        records: Arc::from(records.into_boxed_slice()),
        fingerprints: Default::default(),
    }
}

fn capability_index(size: usize) -> CapabilityIndex {
    let index = CapabilityIndex::new();
    let mut records_by_instance = BTreeMap::<_, Vec<_>>::new();
    for record in capability_records(size, 4) {
        records_by_instance
            .entry(record.instance_id)
            .or_default()
            .push(record);
    }
    for (instance_id, records) in records_by_instance {
        index.upsert_instance(
            instance_id,
            records,
            InstanceFingerprint(instance_id.as_u128() as u64),
        );
    }
    index
}

fn capability_queries() -> Vec<SearchQuery> {
    [
        "create poly sphere",
        "destination path export",
        "material lookdev",
        "uv unwrap shells",
        "render preview",
        "selct layer", // typo fallback
    ]
    .into_iter()
    .map(|query| SearchQuery {
        query: query.to_string(),
        limit: Some(20),
        ..Default::default()
    })
    .collect()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_fuzzy_direct(c: &mut Criterion) {
    let scorer = StrategyFuzzyScorer;
    c.bench_function("StrategyFuzzyScorer/direct", |b| {
        b.iter(|| score_all(&scorer))
    });
}

fn bench_exact_direct(c: &mut Criterion) {
    let scorer = StrategyExactScorer;
    c.bench_function("StrategyExactScorer/direct", |b| {
        b.iter(|| score_all(&scorer))
    });
}

fn bench_factory_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("ScorerFactory/dyn-dispatch");
    for mode in [SearchMode::Fuzzy, SearchMode::Exact] {
        let label = format!("{mode:?}");
        group.bench_with_input(BenchmarkId::new("mode", &label), &mode, |b, &m| {
            let scorer = ScorerFactory::from_mode(m);
            b.iter(|| score_all(scorer.as_ref()))
        });
    }
    group.finish();
}

fn bench_factory_tag(c: &mut Criterion) {
    let mut group = c.benchmark_group("ScorerFactory/from_tag");
    for tag in ["fuzzy", "exact"] {
        group.bench_with_input(BenchmarkId::new("tag", tag), &tag, |b, &t| {
            let scorer = ScorerFactory::from_tag(t);
            b.iter(|| score_all(scorer.as_ref()))
        });
    }
    group.finish();
}

fn bench_hybrid_full_search_thousands(c: &mut Criterion) {
    let snapshot = capability_snapshot(5_000);
    let queries = [
        "create poly sphere",
        "destination path export",
        "material lookdev",
        "uv unwrap shells",
        "render preview",
        "selct layer", // typo fallback
    ];
    c.bench_function("hybrid_full_search/5000_records", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for query in queries {
                let hits = search(
                    black_box(&snapshot),
                    &SearchQuery {
                        query: query.to_string(),
                        limit: Some(20),
                        ..Default::default()
                    },
                );
                total = total.saturating_add(hits.len());
            }
            black_box(total)
        })
    });
}

fn bench_warm_capability_index(c: &mut Criterion) {
    let index = capability_index(5_000);
    let queries = capability_queries();
    let mut group = c.benchmark_group("warm_capability_index/5000_records");

    group.bench_function("snapshot_with_generation", |b| {
        b.iter(|| black_box(&index).snapshot_with_generation())
    });
    group.bench_function("search_service", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for query in &queries {
                let hits = search_service(black_box(&index), black_box(query));
                total = total.saturating_add(hits.len());
            }
            black_box(total)
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_fuzzy_direct,
    bench_exact_direct,
    bench_factory_dispatch,
    bench_factory_tag,
    bench_hybrid_full_search_thousands,
    bench_warm_capability_index,
);
criterion_main!(benches);
