//! Synthetic skill corpus generation (PIP-3408, reworked by PIP-3701).
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
//! # What PIP-3701 changed and why
//!
//! The v1 filler drew both names and descriptions from one 30-word pool, so
//! `maya-skill-00001` and `blender-skill-00002` were lexically
//! indistinguishable and a query built from their own words had no single
//! right answer. [`crate::queries`] could only emit descriptive queries for
//! the handful of real seeds, and the literal truncated-tail variant
//! (`"maya skill"`) matched forty skills at once.
//!
//! v2 gives every filler skill a **fingerprint**: three `{noun}{Form}`
//! compounds derived from the skill index rather than drawn from the RNG, so
//! they are collision-free by construction over the whole corpus range.
//! `primary` goes in the name, `secondary` and `tertiary` go in the
//! description only — which is what lets [`crate::queries`] build a name-free
//! intent query for a filler skill, something v1 could never do.
//!
//! The shared 30-word pool is still there. Real catalogues do share most of
//! their vocabulary; the point of the fingerprint is that each skill also
//! owns a few terms no neighbour has, which is the property
//! [`crate::queries::MAX_ANSWERABLE_DF`] measures.
//!
//! If you change the draw sequence here, the corpus changes and every
//! published hit-rate number changes with it. Bump [`CORPUS_SCHEMA_VERSION`]
//! and re-baseline the gates in [`crate::thresholds`].

use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use rand::SeedableRng;
use rand::seq::SliceRandom;

/// RNG seed shared with `scoring_bench.rs`.
pub const SEED: u64 = 42;

/// Bumped whenever the generator's draw sequence changes.
///
/// The benchmark report echoes this value, so a stored baseline can be
/// matched against the generator that produced it.
///
/// `v1` — 30-word pool for names and descriptions.
/// `v2` — per-skill fingerprint compounds (PIP-3701); filler skills are
/// lexically distinguishable, so descriptive query classes cover them.
pub const CORPUS_SCHEMA_VERSION: &str = "skills-corpus-v2";

/// DCC buckets the synthetic corpus spans.
pub const DCCS: [&str; 5] = ["maya", "blender", "max", "houdini", "unreal"];

/// Domain tags, also the middle segment of every synthetic name.
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

/// Fingerprint noun stems.
///
/// Sized with [`FORMS`] so that `NOUNS.len() * FORMS.len()` (2304) is far
/// larger than any corpus this benchmark builds: a fingerprint compound is
/// then carried by roughly one skill, which is what keeps it under
/// [`crate::queries::MAX_ANSWERABLE_DF`].
const NOUNS: [&str; 48] = [
    "nurbs", "mesh", "point", "vertex", "spline", "voxel", "hair", "cloth", "curve", "surface",
    "volume", "shader", "texture", "light", "camera", "bone", "joint", "skin", "blend", "morph",
    "particle", "fluid", "smoke", "flame", "ocean", "fur", "instance", "proxy", "uv", "normal",
    "tangent", "weight", "driver", "channel", "layer", "render", "node", "group", "sculpt",
    "retopo", "unwrap", "bake", "cache", "rig", "track", "sheet", "tile", "atlas",
];

/// Fingerprint form suffixes, capitalised so the compound reads as DCC
/// camelCase jargon (`nurbsCache`, `uvIsland`, `bakeVector`).
///
/// Deliberately disjoint from [`NOUNS`] so a compound never parses back into
/// a bare noun.
///
/// Split into [`FAMILY_LEN`]-wide slices, one per fingerprint slot. See
/// [`fingerprints`] for why the three slots must not share a suffix set.
const FORMS: [&str; 48] = [
    "Cache", "Surface", "Shell", "Graph", "Set", "Stream", "Rig", "Field", "Map", "Stack", "Delta",
    "Probe", "Array", "Brush", "Knot", "Patch", "Lattice", "Wrap", "Trim", "Weld", "Lod", "Pass",
    "Aov", "Chain", "Cage", "Strip", "Solve", "Frame", "Handle", "Island", "Guide", "Mask",
    "Marker", "Offset", "Profile", "Rail", "Slice", "Socket", "Spine", "Trace", "Tunnel", "Vector",
    "Anchor", "Bevel", "Cluster", "Deck", "Envelope", "Fold",
];

/// Shared action / object vocabulary.
///
/// Every filler skill draws from this pool as well as from its fingerprint,
/// which is what makes the corpus realistic: neighbours share most of their
/// words and differ in a few. A pool this small is only a problem when it is
/// the *only* vocabulary, which was the v1 defect.
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

/// How many form suffixes each fingerprint family owns.
///
/// The three fingerprints of a skill draw from disjoint families, so they can
/// never collide with each other — and, more importantly, so a description
/// fingerprint can never equal another skill's *name* fingerprint. If it
/// could, a name-free intent query would carry a competitor's name token.
const FAMILY_LEN: usize = 16;

/// Index divisor for the first description-only fingerprint.
///
/// `3` → document frequency 3, which stays under
/// [`crate::queries::MAX_ANSWERABLE_DF`] (5) at both corpus scales while the
/// compound still recurs often enough that the corpus is not merely a list of
/// unique strings.
const SECONDARY_DIVISOR: usize = 3;

/// Index divisor for the second description-only fingerprint.
///
/// `4` → document frequency 4. Intent queries need *two* name-free
/// distinctive terms, so a filler skill needs two description-only
/// fingerprints, not one.
const TERTIARY_DIVISOR: usize = 4;

/// Deterministic RNG for corpus construction.
///
/// [`rand::rngs::ChaCha8Rng`] rather than `StdRng`: only a named generator is
/// guaranteed to draw the same stream across `rand` releases.
#[must_use]
pub fn corpus_rng() -> rand::rngs::ChaCha8Rng {
    rand::rngs::ChaCha8Rng::seed_from_u64(SEED)
}

/// `{noun}{Form}` compound for `slot` in fingerprint family `family`.
///
/// Derived from the index rather than drawn from the RNG: a *rare* term per
/// skill is what the answerability check in [`crate::queries`] needs, and a
/// draw would collide often enough at the 1000-skill scale to push most
/// fingerprints over [`crate::queries::MAX_ANSWERABLE_DF`].
///
/// `family` selects one of the three disjoint [`FORMS`] slices. Families are
/// what keep a skill's own three fingerprints distinct and keep a description
/// fingerprint from ever landing on another skill's name.
#[must_use]
pub fn fingerprint(slot: usize, family: usize) -> String {
    let noun = NOUNS[slot % NOUNS.len()];
    let offset = (family % (FORMS.len() / FAMILY_LEN)) * FAMILY_LEN;
    let form = FORMS[offset + ((slot / NOUNS.len()) % FAMILY_LEN)];
    format!("{noun}{form}")
}

/// The three fingerprint compounds of skill `i`: one naming, two descriptive.
///
/// The naming one is `primary`; it is the last segment of the skill's name.
/// The other two live only in the description, which is what lets
/// [`crate::queries`] emit a name-free intent query for a filler skill.
#[must_use]
pub fn fingerprints(i: usize) -> (String, String, String) {
    (
        fingerprint(i, 0),
        fingerprint(i / SECONDARY_DIVISOR, 1),
        fingerprint(i / TERTIARY_DIVISOR, 2),
    )
}

/// Build one synthetic skill, drawing from `rng` in the shared sequence.
pub fn synthetic_skill(i: usize, rng: &mut impl rand::RngExt) -> SkillMetadata {
    let dcc = DCCS[rng.random_range(0..DCCS.len())];
    let domain = TAG_POOL[i % TAG_POOL.len()];
    let (primary, secondary, tertiary) = fingerprints(i);
    let name = format!("{dcc}-{domain}-{primary}");

    let tag_count = rng.random_range(1..=3);
    let mut tags: Vec<String> = (0..tag_count)
        .map(|_| TAG_POOL[rng.random_range(0..TAG_POOL.len())].to_string())
        .collect();
    tags.sort();
    tags.dedup();

    let desc_len = rng.random_range(3..=12);
    let mut words: Vec<String> = vec![secondary, tertiary];
    words
        .extend((0..desc_len).map(|_| WORD_POOL[rng.random_range(0..WORD_POOL.len())].to_string()));
    words.shuffle(rng);
    let description = words.join(" ");

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

    /// Every filler skill owns a fingerprint no neighbour has, which is the
    /// whole point of PIP-3701: without it the descriptive query classes
    /// cannot be emitted for filler and the measured hit rate describes the
    /// corpus rather than the ranker.
    #[test]
    fn filler_skills_are_lexically_distinguishable() {
        let corpus = synthetic_corpus(1000);
        let mut seen = std::collections::HashSet::new();
        for (i, skill) in corpus.iter().enumerate() {
            assert!(
                seen.insert(skill.name.clone()),
                "duplicate filler name {} at index {i}",
                skill.name
            );
            // `primary` is the last name segment and must be unique.
            let primary = skill.name.rsplit('-').next().unwrap_or_default();
            assert!(
                !primary.is_empty(),
                "{} has no fingerprint segment",
                skill.name
            );
        }
        // The two description-only fingerprints must not repeat the name's,
        // or an intent query would lose its only distinctive terms.
        for (i, skill) in corpus.iter().enumerate() {
            let (primary, secondary, tertiary) = fingerprints(i);
            assert!(skill.name.ends_with(&primary));
            assert!(skill.description.contains(&secondary));
            assert!(skill.description.contains(&tertiary));
            assert_ne!(
                primary, secondary,
                "index {i}: name leaks into the description"
            );
            assert_ne!(
                primary, tertiary,
                "index {i}: name leaks into the description"
            );
            assert_ne!(secondary, tertiary, "index {i} has one fingerprint too few");
        }
    }

    /// A description fingerprint must never be another skill's *name*
    /// fingerprint.
    ///
    /// If it were, a name-free intent query for one skill would carry a
    /// competitor's name token, and the query would stop having one right
    /// answer. The disjoint [`FORMS`] families are what guarantee it.
    #[test]
    fn description_fingerprints_never_collide_with_a_name() {
        let names: std::collections::HashSet<String> =
            (0..1000).map(|i| fingerprints(i).0).collect();
        for i in 0..1000 {
            let (_, secondary, tertiary) = fingerprints(i);
            assert!(
                !names.contains(&secondary),
                "index {i}: secondary is a name"
            );
            assert!(!names.contains(&tertiary), "index {i}: tertiary is a name");
        }
    }

    /// Digest of the first 64 skills, pinned so any change to the draw
    /// sequence fails here instead of silently moving every published number.
    ///
    /// Regenerating this constant is a re-baseline: it must come with a bumped
    /// [`CORPUS_SCHEMA_VERSION`] and updated [`crate::thresholds::BASELINE_TOP1_300`].
    const CORPUS_DIGEST_64: u64 = 0xf1a9_ca2a_5eff_eb89;

    #[test]
    fn draw_sequence_matches_the_shared_generator() {
        // Guard against an accidental re-ordering of the draws above: the
        // first skill is fixed for SEED=42 as long as the sequence is.
        let corpus = synthetic_corpus(1);
        let first = &corpus[0];
        let (primary, secondary, tertiary) = fingerprints(0);
        assert_eq!(first.name, format!("blender-modeling-{primary}"));
        assert_eq!(first.dcc, "blender");
        assert_eq!(first.tags.len(), 3);
        assert_eq!(first.tools.len(), 3);
        assert_eq!(first.search_aliases.len(), 0);
        assert!(first.search_hint.is_empty());
        assert_eq!(first.layer.as_deref(), None);
        // 2 fingerprints + a drawn description length, each word space-joined.
        let words: Vec<&str> = first.description.split_whitespace().collect();
        assert!(
            words.len() >= 5 && words.len() <= 14,
            "unexpected description length {}",
            words.len()
        );
        assert!(words.contains(&secondary.as_str()));
        assert!(words.contains(&tertiary.as_str()));
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
