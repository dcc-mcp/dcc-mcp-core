//! Criterion benchmarks for the search-scoring strategy seam (issue #765).
//!
//! These benchmarks pin the raw throughput of [`StrategyFuzzyScorer`] and
//! [`StrategyExactScorer`] so that adding trait dispatch overhead is
//! detected before merging. The acceptance criterion is that either
//! scorer stays within 10 % of today's baseline when called via a
//! `Box<dyn StrategyScorer>` obtained from [`ScorerFactory`].
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
    capability::{CapabilityRecord, IndexSnapshot, search},
};
use std::{hint::black_box, sync::Arc};

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

fn capability_snapshot(size: usize) -> IndexSnapshot {
    let records: Vec<CapabilityRecord> = (0..size)
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
            let iid = uuid::Uuid::from_u128((i as u128) + 1);
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
        .collect();
    IndexSnapshot {
        records: Arc::from(records.into_boxed_slice()),
        fingerprints: Default::default(),
    }
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

criterion_group!(
    benches,
    bench_fuzzy_direct,
    bench_exact_direct,
    bench_factory_dispatch,
    bench_factory_tag,
    bench_hybrid_full_search_thousands,
);
criterion_main!(benches);
