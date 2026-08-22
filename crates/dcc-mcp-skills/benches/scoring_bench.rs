//! Criterion benchmarks for the shared skill search scorer.
//!
//! Generates synthetic skill catalogues at 1k / 5k / 10k scale and measures
//! the production shared-ranking path plus the optional exact-token index.

use criterion::{Criterion, criterion_group, criterion_main};
use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::catalog::inverted_index::InvertedIndex;
use dcc_mcp_skills::catalog::scoring::FieldTokens;
use rand::RngExt;
use std::hint::black_box;
use std::sync::Arc;

// ── Synthetic skill generation ──────────────────────────────────────────

fn synthetic_skill(i: usize, rng: &mut impl rand::Rng) -> SkillMetadata {
    let dcc = ["maya", "blender", "max", "houdini", "unreal"][rng.random_range(0..5)];
    let mut name = format!("{dcc}-skill-{i:05}");
    if rng.random_bool(0.2) {
        name.push_str("-advanced");
    }

    let tags_pool = [
        "modeling",
        "rigging",
        "animation",
        "rendering",
        "texturing",
        "lighting",
        "simulation",
        "cfx",
        "fx",
        "layout",
    ];
    let tag_count = rng.random_range(1..=3);
    let mut tags: Vec<String> = (0..tag_count)
        .map(|_| tags_pool[rng.random_range(0..tags_pool.len())].to_string())
        .collect();
    tags.sort();
    tags.dedup();

    let desc_words = [
        "create",
        "edit",
        "manage",
        "process",
        "export",
        "import",
        "generate",
        "apply",
        "transform",
        "analyse",
        "compute",
        "render",
        "polygon",
        "mesh",
        "curve",
        "surface",
        "volume",
        "light",
        "camera",
        "material",
        "texture",
        "shader",
        "bone",
        "skin",
        "blend",
        "shape",
        "morph",
        "deform",
        "simulate",
        "bake",
    ];
    let desc_len = rng.random_range(3..=12);
    let description: String = (0..desc_len)
        .map(|_| desc_words[rng.random_range(0..desc_words.len())])
        .collect::<Vec<_>>()
        .join(" ");

    let hint = if rng.random_bool(0.5) {
        let hint_len = rng.random_range(1..=5);
        (0..hint_len)
            .map(|_| desc_words[rng.random_range(0..desc_words.len())])
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };

    let tool_count = rng.random_range(1..=4);
    let tools: Vec<ToolDeclaration> = (0..tool_count)
        .map(|t| ToolDeclaration {
            name: format!("{name}-tool-{t}"),
            description: (0..rng.random_range(2..=6))
                .map(|_| desc_words[rng.random_range(0..desc_words.len())])
                .collect::<Vec<_>>()
                .join(" "),
            ..Default::default()
        })
        .collect();

    let alias_count = rng.random_range(0..=2);
    let search_aliases: Vec<String> = (0..alias_count)
        .map(|_| format!("alias-{}-{}", name, rng.random_range(0..999)))
        .collect();

    let layer = match rng.random_range(0u8..100) {
        0..=59 => None,
        60..=79 => Some("domain".to_string()),
        80..=89 => Some("infrastructure".to_string()),
        90..=94 => Some("thin-harness".to_string()),
        _ => Some("example".to_string()),
    };

    SkillMetadata {
        name,
        description,
        search_hint: hint,
        tags,
        dcc: dcc.to_string(),
        version: "1.0.0".to_string(),
        tools,
        search_aliases,
        layer,
        ..Default::default()
    }
}

fn synthetic_catalogue(
    n: usize,
) -> (
    Vec<SkillMetadata>,
    Vec<FieldTokens>,
    Vec<usize>,
    Vec<String>,
) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let metas: Vec<SkillMetadata> = (0..n).map(|i| synthetic_skill(i, &mut rng)).collect();
    let names: Vec<String> = metas.iter().map(|m| m.name.clone()).collect();
    let fields: Vec<FieldTokens> = metas.iter().map(FieldTokens::from_metadata).collect();
    let doc_lens: Vec<usize> = fields.iter().map(|f| f.doc_len()).collect();
    (metas, fields, doc_lens, names)
}

// ── Benchmark groups ────────────────────────────────────────────────────

fn bench_inverted_index(c: &mut Criterion) {
    for n in [1_000usize, 5_000, 10_000] {
        let (_metas, fields, doc_lens, names) = synthetic_catalogue(n);
        let group_label = format!("search_index_{n}_skills");
        black_box(doc_lens);

        // ── PIP-2469: inverted index vs linear scan ──
        let names_and_fields: Vec<(&str, &FieldTokens)> = names
            .iter()
            .zip(fields.iter())
            .map(|(n, f)| (n.as_str(), f))
            .collect();
        let idx = InvertedIndex::build(&names_and_fields);
        let query_tokens = vec!["polygon".to_string(), "bevel".to_string()];

        c.bench_function(&format!("inverted_index_build/{group_label}"), |b| {
            b.iter(|| {
                let _ = InvertedIndex::build(&names_and_fields);
            })
        });

        c.bench_function(&format!("inverted_index_query/{group_label}"), |b| {
            b.iter(|| {
                let mut total = 0usize;
                for token in &query_tokens {
                    if let Some(postings) = idx.get(token) {
                        total += postings.count();
                    }
                }
                black_box(total);
            })
        });

        c.bench_function(&format!("inverted_index_upsert/{group_label}"), |b| {
            b.iter(|| {
                idx.upsert("benchmark-updated-skill", &fields[0]);
            })
        });
    }
}

fn bench_catalog_search(c: &mut Criterion) {
    for n in [1_000usize, 10_000] {
        let (metas, _, _, _) = synthetic_catalogue(n);
        let catalog = SkillCatalog::new(Arc::new(ToolRegistry::new()));
        for metadata in metas {
            catalog.add_skill(metadata);
        }

        // Build the lazy index outside the measured loop. The benchmark then
        // captures candidate selection + scoring without catalog-wide clones.
        black_box(catalog.search_skills(Some("polygon bevel"), &[], None, None, Some(25)));
        c.bench_function(&format!("catalog_search/{n}_skills"), |b| {
            b.iter(|| {
                black_box(catalog.search_skills(Some("polygon bevel"), &[], None, None, Some(25)));
            })
        });
    }
}

criterion_group!(benches, bench_inverted_index, bench_catalog_search);
criterion_main!(benches);
