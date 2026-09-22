//! Criterion benchmarks for the query-efficiency dimension (PIP-3408).
//!
//! Latency here is a **trend signal, not a merge gate** — CI hardware moves it
//! far more than a ranking change does. Run it to compare two heads or to
//! investigate a suspected slowdown; the hard gates live in
//! `tests/hit_rate_gate.rs` and `tests/context_gate.rs`.
//!
//! All three query classes are measured separately because they cost
//! differently: a literal name query is mostly an exact-token lookup, while an
//! intent query falls through to fuzzy matching across the whole catalogue.

use criterion::{Criterion, criterion_group, criterion_main};
use dcc_mcp_skills_bench::Corpus;
use dcc_mcp_skills_bench::corpus::{SCALE_300, SCALE_1000};
use dcc_mcp_skills_bench::queries::{ALL_KINDS, build_queries};
use dcc_mcp_skills_bench::run::{Filter, TOP_K};
use std::hint::black_box;

fn bench_scale(c: &mut Criterion, scale: usize) {
    let corpus = Corpus::build(scale);
    let catalog = corpus.catalog();
    let queries = build_queries(&corpus);

    // Warm the lazily-built index outside the measured loop.
    if let Some(first) = queries.first() {
        black_box(catalog.search_skills(Some(&first.text), &[], None, None, Some(TOP_K)));
    }

    for filter in Filter::ALL {
        for kind in ALL_KINDS {
            // Each query carries its own DCC. Taking one `dcc` for the whole
            // subset would point ~80% of the queries at a shard that cannot
            // contain their answer, which measures an almost-empty candidate
            // set rather than a search.
            let subset: Vec<(&str, Option<&str>)> = queries
                .iter()
                .filter(|query| query.kind == kind)
                .map(|query| {
                    let dcc = match filter {
                        Filter::Dcc => Some(query.dcc.as_str()),
                        Filter::Unfiltered => None,
                    };
                    (query.text.as_str(), dcc)
                })
                .collect();
            if subset.is_empty() {
                continue;
            }

            let mut group = c.benchmark_group(format!("skills_query/{scale}/{filter:?}/{kind:?}"));
            group.bench_function("search_skills", |b| {
                b.iter(|| {
                    let mut hits = 0usize;
                    for (text, dcc) in &subset {
                        let results = catalog.search_skills(
                            Some(text),
                            &[],
                            black_box(*dcc),
                            None,
                            Some(TOP_K),
                        );
                        hits += results.len();
                    }
                    black_box(hits);
                })
            });
            group.finish();
        }
    }
}

fn bench_skills_query(c: &mut Criterion) {
    bench_scale(c, SCALE_300);
    bench_scale(c, SCALE_1000);
}

criterion_group!(benches, bench_skills_query);
criterion_main!(benches);
