//! Synthetic skill corpus generation (PIP-3408).
//!
//! This is the corpus generator the skills benchmark shares with
//! `crates/dcc-mcp-skills/benches/scoring_bench.rs`. Both call sites feed the
//! same `ChaCha8Rng::seed_from_u64(SEED)` through the same draw sequence, so a
//! corpus of a given size is byte-identical wherever it is built. That
//! property is what makes the hit-rate numbers comparable between the
//! throughput bench and this benchmark, and it is why the generator lives
//! here instead of being rewritten per caller.
//!
//! [`ChaCha8Rng`] is deliberate. `StdRng` is explicitly *not* guaranteed to be
//! reproducible across `rand` versions, so a patch bump could silently change
//! the corpus and drift every published number while [`CORPUS_SCHEMA_VERSION`]
//! stayed put. A named generator is the reproducibility contract.
//!
//! If you change the draw sequence here, the corpus changes and every
//! published hit-rate number changes with it. Bump [`CORPUS_SCHEMA_VERSION`]
//! and re-baseline the gates in [`crate::thresholds`].

use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use rand::SeedableRng;

/// RNG seed shared with `scoring_bench.rs`.
pub const SEED: u64 = 42;

/// Bumped whenever the generator's draw sequence changes.
///
/// The benchmark report echoes this value, so a stored baseline can be
/// matched against the generator that produced it.
pub const CORPUS_SCHEMA_VERSION: &str = "skills-corpus-v1";

/// DCC buckets the synthetic corpus spans.
pub const DCCS: [&str; 5] = ["maya", "blender", "max", "houdini", "unreal"];

const TAG_POOL: [&str; 10] = [
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

const WORD_POOL: [&str; 30] = [
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

/// Deterministic RNG for corpus construction.
///
/// [`rand::rngs::ChaCha8Rng`] rather than `StdRng`: only a named generator is
/// guaranteed to draw the same stream across `rand` releases.
#[must_use]
pub fn corpus_rng() -> rand::rngs::ChaCha8Rng {
    rand::rngs::ChaCha8Rng::seed_from_u64(SEED)
}

/// Build one synthetic skill, drawing from `rng` in the shared sequence.
pub fn synthetic_skill(i: usize, rng: &mut impl rand::RngExt) -> SkillMetadata {
    let dcc = DCCS[rng.random_range(0..DCCS.len())];
    let mut name = format!("{dcc}-skill-{i:05}");
    if rng.random_bool(0.2) {
        name.push_str("-advanced");
    }

    let tag_count = rng.random_range(1..=3);
    let mut tags: Vec<String> = (0..tag_count)
        .map(|_| TAG_POOL[rng.random_range(0..TAG_POOL.len())].to_string())
        .collect();
    tags.sort();
    tags.dedup();

    let desc_len = rng.random_range(3..=12);
    let description: String = (0..desc_len)
        .map(|_| WORD_POOL[rng.random_range(0..WORD_POOL.len())])
        .collect::<Vec<_>>()
        .join(" ");

    let search_hint = if rng.random_bool(0.5) {
        let hint_len = rng.random_range(1..=5);
        (0..hint_len)
            .map(|_| WORD_POOL[rng.random_range(0..WORD_POOL.len())])
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
                .map(|_| WORD_POOL[rng.random_range(0..WORD_POOL.len())])
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
        search_hint,
        tags,
        dcc: dcc.to_string(),
        version: "1.0.0".to_string(),
        tools,
        search_aliases,
        layer,
        ..Default::default()
    }
}

/// Build `n` synthetic skills from a fresh seeded RNG.
///
/// The first `n` draws are identical to the first `n` draws of any larger
/// corpus, so the 300-skill corpus is a prefix of the 1000-skill corpus.
#[must_use]
pub fn synthetic_corpus(n: usize) -> Vec<SkillMetadata> {
    let mut rng = corpus_rng();
    (0..n).map(|i| synthetic_skill(i, &mut rng)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_deterministic_and_prefix_stable() {
        let small = synthetic_corpus(16);
        let large = synthetic_corpus(64);
        assert_eq!(small.len(), 16);
        assert_eq!(small[..], large[..16], "small corpus must be a prefix");
    }

    #[test]
    fn synthetic_names_carry_a_known_dcc() {
        let corpus = synthetic_corpus(64);
        for skill in &corpus {
            assert!(
                DCCS.contains(&skill.dcc.as_str()),
                "unexpected dcc {}",
                skill.dcc
            );
            assert!(skill.name.starts_with(&format!("{}-", skill.dcc)));
        }
    }

    /// Digest of the first 64 skills, pinned so any change to the draw
    /// sequence fails here instead of silently moving every published number.
    ///
    /// Regenerating this constant is a re-baseline: it must come with a bumped
    /// [`CORPUS_SCHEMA_VERSION`] and updated [`crate::thresholds::BASELINE_TOP1_300`].
    const CORPUS_DIGEST_64: u64 = 0xcd51_fce7_153a_4c9c;

    #[test]
    fn draw_sequence_matches_the_shared_generator() {
        // Guard against an accidental re-ordering of the draws above: the
        // first skill is fixed for SEED=42 as long as the sequence is.
        let corpus = synthetic_corpus(1);
        let first = &corpus[0];
        assert_eq!(first.name, "blender-skill-00000-advanced");
        assert_eq!(first.dcc, "blender");
        assert_eq!(first.description.split_whitespace().count(), 9);
        assert_eq!(first.tools.len(), 1);
        assert_eq!(first.search_aliases.len(), 2);
        assert_eq!(first.tags.len(), 3);
        assert!(first.search_hint.is_empty());
        assert_eq!(first.layer.as_deref(), Some("infrastructure"));
    }

    #[test]
    fn corpus_digest_is_pinned() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let corpus = synthetic_corpus(64);
        let mut hasher = DefaultHasher::new();
        for skill in &corpus {
            for field in [&skill.name, &skill.description, &skill.search_hint] {
                field.hash(&mut hasher);
            }
            skill.tags.hash(&mut hasher);
        }
        let digest = hasher.finish();
        assert_eq!(
            digest, CORPUS_DIGEST_64,
            "the synthetic corpus changed; every hit-rate number moves with it, so bump CORPUS_SCHEMA_VERSION and re-baseline BASELINE_*_300"
        );
    }
}
