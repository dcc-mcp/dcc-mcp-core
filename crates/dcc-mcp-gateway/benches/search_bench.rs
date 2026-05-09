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
    ScorerFactory, SearchMode, StrategyExactScorer, StrategyFuzzyScorer, StrategyScorer,
};
use std::hint::black_box;

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

criterion_group!(
    benches,
    bench_fuzzy_direct,
    bench_exact_direct,
    bench_factory_dispatch,
    bench_factory_tag,
);
criterion_main!(benches);
