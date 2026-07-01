//! Criterion benchmarks for BM25 skill search scoring.
//!
//! Measures the impact of pre-computed `FieldTokens` caching (PIP-2467).
//! Generates synthetic skill catalogues at 1k / 5k / 10k scale and runs
//! `score_skills` (re-tokenise every call) vs `score_skills_with_tokens`
//! (cached tokens) to quantify the allocation/CPU saving.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use dcc_mcp_models::{SkillMetadata, SkillScope, ToolDeclaration};
use dcc_mcp_skills::catalog::inverted_index::InvertedIndex;
use dcc_mcp_skills::catalog::scoring::{FieldTokens, score_skills, score_skills_with_tokens};

// ── Synthetic skill generation ──────────────────────────────────────────

fn synthetic_skill(i: usize, rng: &mut impl rand::Rng) -> SkillMetadata {
    let dcc = ["maya", "blender", "max", "houdini", "unreal"][rng.gen_range(0..5)];
    let mut name = format!("{dcc}-skill-{i:05}");
    if rng.gen_bool(0.2) {
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
    let tag_count = rng.gen_range(1..=3);
    let mut tags: Vec<String> = (0..tag_count)
        .map(|_| tags_pool[rng.gen_range(0..tags_pool.len())].to_string())
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
    let desc_len = rng.gen_range(3..=12);
    let description: String = (0..desc_len)
        .map(|_| desc_words[rng.gen_range(0..desc_words.len())])
        .collect::<Vec<_>>()
        .join(" ");

    let hint = if rng.gen_bool(0.5) {
        let hint_len = rng.gen_range(1..=5);
        (0..hint_len)
            .map(|_| desc_words[rng.gen_range(0..desc_words.len())])
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };

    let tool_count = rng.gen_range(1..=4);
    let tools: Vec<ToolDeclaration> = (0..tool_count)
        .map(|t| ToolDeclaration {
            name: format!("{name}-tool-{t}"),
            description: (0..rng.gen_range(2..=6))
                .map(|_| desc_words[rng.gen_range(0..desc_words.len())])
                .collect::<Vec<_>>()
                .join(" "),
            ..Default::default()
        })
        .collect();

    let alias_count = rng.gen_range(0..=2);
    let search_aliases: Vec<String> = (0..alias_count)
        .map(|_| format!("alias-{}-{}", name, rng.gen_range(0..999)))
        .collect();

    let layer = match rng.gen_range(0u8..100) {
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

fn synthetic_catalogue(n: usize) -> (Vec<SkillMetadata>, Vec<FieldTokens>, Vec<usize>) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let metas: Vec<SkillMetadata> = (0..n).map(|i| synthetic_skill(i, &mut rng)).collect();
    let fields: Vec<FieldTokens> = metas.iter().map(FieldTokens::from_metadata).collect();
    let doc_lens: Vec<usize> = fields.iter().map(|f| f.doc_len()).collect();
    (metas, fields, doc_lens)
}

// ── Benchmark groups ────────────────────────────────────────────────────

fn bench_score_skills(c: &mut Criterion) {
    for n in [1_000usize, 5_000, 10_000] {
        let (metas, fields, doc_lens) = synthetic_catalogue(n);
        let skill_refs: Vec<&SkillMetadata> = metas.iter().collect();
        let scopes: Vec<SkillScope> = vec![SkillScope::Repo; n];

        let group_label = format!("score_skills_{n}_skills");
        c.bench_function(&format!("re_tokenize/{group_label}"), |b| {
            b.iter(|| {
                let _ = score_skills("polygon bevel", &skill_refs, &scopes, false, None);
            })
        });

        c.bench_function(&format!("cached_tokens/{group_label}"), |b| {
            b.iter(|| {
                let field_refs: Vec<&FieldTokens> = fields.iter().collect();
                let _ = score_skills_with_tokens(
                    "polygon bevel",
                    &skill_refs,
                    &scopes,
                    false,
                    None,
                    &field_refs,
                    &doc_lens,
                );
            })
        });

        // ── PIP-2469: inverted index vs linear scan ──
        let idx = InvertedIndex::build(&fields);
        let query_tokens = vec!["polygon".to_string(), "bevel".to_string()];

        c.bench_function(&format!("inverted_index_build/{group_label}"), |b| {
            b.iter(|| {
                let _ = InvertedIndex::build(&fields);
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
    }
}

criterion_group!(benches, bench_score_skills);
criterion_main!(benches);
