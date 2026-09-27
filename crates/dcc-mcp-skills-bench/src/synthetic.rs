//! Synthetic skill corpus generation (PIP-3408).
//!
//! The benchmark starts from the real skills this repository ships (see
//! [`crate::seeds`]) and only *fills* up to the target corpus size with
//! synthetic rows. This module is that filler.
//!
//! # Why the filler owns real vocabulary
//!
//! An earlier revision drew every description from a shared 30-word pool and
//! named every skill `<dcc>-skill-<n>`. That made the filler unanswerable in
//! two distinct ways, and both of them depressed the measured hit rate for
//! reasons that had nothing to do with the ranker:
//!
//! * A truncated-tail literal query for `maya-skill-00042` is `"maya skill"`,
//!   which every `maya-skill-*` in the corpus matches equally. There is no
//!   right answer to grade against.
//! * Because every description used the same 30 words, no descriptive term was
//!   rare enough to identify one skill, so paraphrase and intent queries were
//!   only ever generated for the ~26 real seeds.
//!
//! The filler therefore draws from a structured vocabulary — domain object,
//! action, qualifier, host — so each skill has a lexical fingerprint of its
//! own and its name is answerable at every prefix.
//!
//! [`ChaCha8Rng`] is deliberate. `StdRng` is explicitly *not* guaranteed to be
//! reproducible across `rand` versions, so a patch bump could silently change
//! the corpus and drift every published number while [`CORPUS_SCHEMA_VERSION`]
//! stayed put. A named generator is the reproducibility contract.
//!
//! `crates/dcc-mcp-skills/benches/scoring_bench.rs` keeps its own copy of a
//! synthetic generator and shares only [`SEED`]. That is fine: it measures
//! throughput, so it needs a stable corpus of the right size, not the same
//! text this benchmark grades against.
//!
//! If you change the draw sequence here, the corpus changes and every
//! published hit-rate number changes with it. Bump [`CORPUS_SCHEMA_VERSION`]
//! and re-baseline the gates in [`crate::thresholds`].

use dcc_mcp_models::{RecallContext, SkillMetadata, ToolDeclaration};
use rand::SeedableRng;

/// RNG seed shared with `scoring_bench.rs`.
pub const SEED: u64 = 42;

/// Bumped whenever the generator's draw sequence changes.
///
/// The benchmark report echoes this value, so a stored baseline can be
/// matched against the generator that produced it.
pub const CORPUS_SCHEMA_VERSION: &str = "skills-corpus-v2";

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

/// Domain objects a DCC skill operates on.
const OBJECTS: [&str; 24] = [
    "mesh", "curve", "surface", "volume", "pointcloud", "voxel", "skeleton",
    "joint", "blendshape", "nurb", "camera", "light", "material", "shader",
    "texture", "uv", "hair", "cloth", "particle", "rig", "proxy", "cache",
    "layout", "instance",
];

/// Operations a DCC skill performs on its object.
const ACTIONS: [&str; 24] = [
    "retopologise", "subdivide", "bevel", "sweep", "loft", "deform", "mirror",
    "scatter", "boolean", "unwrap", "bake", "cache", "simulate", "constrain",
    "skin", "morph", "instance", "sequence", "composite", "denoise",
    "relight", "quantise", "validate", "stream",
];

/// Optional trailing qualifier that separates sibling skills.
const QUALIFIERS: [&str; 12] = [
    "batch", "interactive", "export", "import", "diagnose", "preview",
    "delta", "archive", "lod", "template", "realtime", "offline",
];

/// Words carrying an object's material or look-dev character.
const LOOK_WORDS: [&str; 12] = [
    "lambert", "specular", "emissive", "translucent", "anisotropic",
    "subsurface", "displacement", "normal", "roughness", "metallic",
    "ambient", "occlusion",
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
    let look = LOOK_WORDS[rng.random_range(0..LOOK_WORDS.len())];
    let tag = TAG_POOL[rng.random_range(0..TAG_POOL.len())];

    // The head is derived from `i` by mixed radix, not drawn. Random draws
    // collide: at 1000 skills a random (dcc, object, action, qualifier)
    // quadruple repeats an existing combination often enough to break the
    // uniqueness guarantee below, and a shared name prefix is exactly the
    // ambiguity that made the old `<dcc>-skill-<n>` naming unanswerable.
    let (qi, rest) = (i % QUALIFIERS.len(), i / QUALIFIERS.len());
    let (ai, rest) = (rest % ACTIONS.len(), rest / ACTIONS.len());
    let (oi, rest) = (rest % OBJECTS.len(), rest / OBJECTS.len());
    let (di, _rest) = (rest % DCCS.len(), rest / DCCS.len());
    let (dcc, object, action, qualifier) = (
        DCCS[di],
        OBJECTS[oi],
        ACTIONS[ai],
        QUALIFIERS[qi],
    );

    // Every prefix of the name is discriminating: the head spans four
    // independent vocabularies, and the ordinal tail only carries once
    // 5 * 24 * 24 * 12 = 34_560 skills exist — far beyond either scale.
    let name = format!("{dcc}-{object}-{action}-{qualifier}-{i:05}");

    // The fingerprint terms (object / action / qualifier) are what make the
    // skill answerable, so they lead the description rather than being
    // diluted by shared vocabulary. `look` and `tag` come from small pools and
    // are deliberately the *minority* of the text.
    let description = format!(
        "{action} {object} {qualifier} workflow for {dcc} {tag} tasks: \
         {action} each {object} target, check {qualifier} {look} output, \
         and report {object} {action} deltas"
    );
    let search_hint = format!("{dcc} {object} {action} {qualifier}");
    let tags = vec![tag.to_string(), object.to_string(), dcc.to_string()];

    let tool_count = rng.random_range(1..=4);
    let tools: Vec<ToolDeclaration> = (0..tool_count)
        .map(|t| ToolDeclaration {
            name: format!("{action}_{object}_{t}"),
            description: format!(
                "{action} the {object} {look} layer and report {qualifier} {tag} deltas"
            ),
            ..Default::default()
        })
        .collect();

    let alias_count = rng.random_range(0..=2);
    let search_aliases: Vec<String> = (0..alias_count)
        .map(|a| format!("{object}-{action}-{a}"))
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
        recall_context: Some(RecallContext {
            app_type: Some(dcc.to_string()),
            domain: Some(tag.to_string()),
            workflow_stage: Some(object.to_string()),
            task_category: Some(action.to_string()),
        }),
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

    /// Every synthetic skill must be separable from every other one by name
    /// alone, at every prefix. The old `<dcc>-skill-<n>` scheme failed this:
    /// the truncated-tail query for `maya-skill-00042` is `"maya skill"`,
    /// matched by every other `maya-skill-*`, so the query had no right
    /// answer and dragged the measured hit rate down for a corpus reason
    /// rather than a ranking one.
    #[test]
    fn names_are_unique_before_the_ordinal_tail() {
        let corpus = synthetic_corpus(1000);
        let mut fingerprints: std::collections::HashSet<String> = std::collections::HashSet::new();
        for skill in &corpus {
            let segments: Vec<&str> = skill.name.split('-').collect();
            // Drop the numeric tail: what is left must still be unique.
            let head = segments[..segments.len() - 1].join("-");
            assert!(
                fingerprints.insert(head.clone()),
                "name prefix `{head}` is shared by more than one skill"
            );
        }
    }

    #[test]
    fn descriptions_are_not_drawn_from_a_shared_pool() {
        // The property the old 30-word pool destroyed: a description term
        // that identifies one skill. Sample the corpus and require that the
        // median skill owns at least one term no other skill uses.
        let corpus = synthetic_corpus(300);
        let mut df: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for skill in &corpus {
            for term in skill
                .description
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| t.len() > 1)
            {
                *df.entry(term).or_insert(0) += 1;
            }
        }
        let unique_terms = corpus
            .iter()
            .map(|skill| {
                skill
                    .description
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|term| df.get(term).copied().unwrap_or(0) <= 2)
                    .count()
            })
            .min()
            .unwrap_or(0);
        assert!(
            unique_terms >= 2,
            "every synthetic skill must own at least two uncommon terms, \
             the weakest owns {unique_terms}"
        );
    }

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

    #[test]
    fn synthetic_skills_carry_a_complete_recall_context() {
        let corpus = synthetic_corpus(64);
        for skill in &corpus {
            let ctx = skill
                .recall_context
                .as_ref()
                .expect("synthetic skills must populate recall_context");
            assert!(ctx.app_type.is_some());
            assert!(ctx.domain.is_some());
            assert!(ctx.workflow_stage.is_some());
            assert!(ctx.task_category.is_some());
        }
    }

    /// Digest of the first 64 skills, pinned so any change to the draw
    /// sequence fails here instead of silently moving every published number.
    ///
    /// Regenerating this constant is a re-baseline: it must come with a bumped
    /// [`CORPUS_SCHEMA_VERSION`] and updated [`crate::thresholds::BASELINE_TOP1_300`].
    const CORPUS_DIGEST_64: u64 = 0x84dd_1c65_b9b1_7fc0;

    #[test]
    fn draw_sequence_matches_the_shared_generator() {
        // Guard against an accidental re-ordering of the draws above: the
        // first skill is fixed for SEED=42 as long as the sequence is.
        let corpus = synthetic_corpus(1);
        let first = &corpus[0];
        assert_eq!(first.name, "maya-mesh-retopologise-batch-00000");
        assert_eq!(first.dcc, "maya");
        assert_eq!(first.tools.len(), 1);
        assert_eq!(first.search_aliases.len(), 2);
        assert_eq!(first.tags.len(), 3);
        assert_eq!(first.layer.as_deref(), Some("domain"));
        // The fingerprint terms lead the description, so a descriptive query
        // built from them has exactly one right answer.
        assert!(first.description.starts_with("retopologise mesh batch"));
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
