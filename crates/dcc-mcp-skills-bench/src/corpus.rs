//! Corpus assembly: real seeds, synthetic fill, and hard negatives (PIP-3408).
//!
//! A catalogue of randomly named skills measures almost nothing: every
//! `maya-skill-*` differs from every `blender-skill-*` in its very first
//! token, so the DCC prefix alone separates them and the hit rate comes back
//! flattering. Real catalogues are not like that. They contain
//! `maya-mesh-ops` next to `maya-mesh`, and `maya-mesh-ops` next to
//! `blender-mesh-ops`.
//!
//! [`Corpus::build`] therefore injects both shapes deliberately:
//!
//! * [`HardNegativeKind::SameDccNearName`] — one name is a prefix of the other
//!   inside the same DCC (`maya-mesh` / `maya-mesh-ops`). Discriminating them
//!   needs the tail of the name, not the prefix.
//! * [`HardNegativeKind::CrossDccSameContent`] — the same skill content under a
//!   different DCC prefix. Indexed fields are identical except `dcc`, so the
//!   pair is only separable when the query carries a DCC filter. That is the
//!   pair that makes the "with `dcc`" and "without `dcc`" splits differ, which
//!   is exactly why the benchmark reports them separately.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_skills::SkillCatalog;
use rand::SeedableRng;

use crate::synthetic::{self, DCCS};

/// Small corpus scale — the one the CI regression gate runs (PIP-3408).
pub const SCALE_300: usize = 300;
/// Large corpus scale — reported as a trend, not gated (PIP-3408).
pub const SCALE_1000: usize = 1000;

/// Share of corpus members that get a hard-negative twin, in percent.
///
/// High enough to move the aggregate numbers, low enough that the aggregate
/// still describes the catalogue rather than the injected pairs.
pub const HARD_NEGATIVE_RATE_PCT: usize = 15;

/// Why a corpus member is hard to separate from its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HardNegativeKind {
    /// Same DCC, and one name is a prefix of the other.
    SameDccNearName,
    /// Identical indexed content under a different DCC prefix.
    CrossDccSameContent,
}

impl HardNegativeKind {
    /// Stable label used in reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SameDccNearName => "same_dcc_near_name",
            Self::CrossDccSameContent => "cross_dcc_same_content",
        }
    }
}

/// A twin injected next to `target` to make ranking it harder.
#[derive(Debug, Clone)]
pub struct HardNegative {
    /// Name of the injected twin.
    pub twin: String,
    /// Name of the skill the twin competes with.
    pub target: String,
    /// Why the pair is confusing.
    pub kind: HardNegativeKind,
}

/// A benchmark corpus: the catalogue plus the metadata needed to grade it.
pub struct Corpus {
    /// Requested corpus size, hard negatives included.
    pub scale: usize,
    /// Every skill in the catalogue, in a stable order.
    pub skills: Vec<SkillMetadata>,
    /// Number of leading entries in [`Self::skills`] that are real seeds.
    pub seeds: usize,
    /// Twins appended after the base catalogue.
    pub hard_negatives: Vec<HardNegative>,
    /// Names that carry at least one twin.
    targets: Vec<String>,
    by_name: HashMap<String, usize>,
}

impl Corpus {
    /// Build the corpus for `scale` (see [`SCALE_300`] / [`SCALE_1000`]).
    #[must_use]
    pub fn build(scale: usize) -> Self {
        let seeds = crate::seeds::seeds();
        let seed_names: HashSet<String> = seeds.iter().map(|s| s.name.clone()).collect();
        let seeds_len = seeds.len();

        let chosen = scale * HARD_NEGATIVE_RATE_PCT / 100;
        let twins_budget = chosen * 2;
        // Keep the base short enough that base + twins lands on `scale`.
        let keep = scale.saturating_sub(twins_budget).max(seeds_len);

        let pool = synthetic::synthetic_corpus(scale);
        let mut base = seeds;
        // `keep` is the whole base, seeds included — the synthetic fill is
        // only what is left after them.
        let fill = keep.saturating_sub(seeds_len);
        base.extend(pool.iter().take(fill).cloned());
        dedup_by_name(&mut base);

        let hard_targets = select_targets(&base, seeds_len, chosen);
        let (twins, hard_negatives) = build_twins(&base, &hard_targets);

        // Query targets are the hard-negative set *plus* a clean control set,
        // so `hard_negative` and `clean` are two comparable halves rather than
        // the whole thing and the empty set.
        let hard_set: HashSet<String> = hard_targets.iter().cloned().collect();
        let mut targets = hard_targets;
        targets.extend(select_clean_targets(&base, &hard_set, chosen));

        let mut skills = base;
        skills.extend(twins);
        // A twin is dropped when its name collides, and dedup can shrink the
        // base, so top up from the unused tail of the pool to land on `scale`.
        for extra in pool.iter().skip(fill) {
            if skills.len() >= scale {
                break;
            }
            skills.push(extra.clone());
        }

        let seed_count = skills
            .iter()
            .filter(|skill| seed_names.contains(&skill.name))
            .count();
        let by_name = skills
            .iter()
            .enumerate()
            .map(|(index, skill)| (skill.name.clone(), index))
            .collect();

        Self {
            scale,
            skills,
            seeds: seed_count,
            hard_negatives,
            targets,
            by_name,
        }
    }

    /// Names the benchmark generates queries for.
    #[must_use]
    pub fn targets(&self) -> &[String] {
        &self.targets
    }

    /// Look up a skill by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SkillMetadata> {
        self.by_name.get(name).map(|index| &self.skills[*index])
    }

    /// Targets that have at least one hard-negative twin.
    #[must_use]
    pub fn hard_negative_targets(&self) -> HashSet<&str> {
        self.hard_negatives
            .iter()
            .map(|negative| negative.target.as_str())
            .collect()
    }

    /// Whether `name` has a twin whose name is a prefix-neighbour of it.
    ///
    /// Query generation uses this to avoid emitting a query that the injected
    /// twin answers better: for a target `maya-mesh-ops` with a `maya-mesh`
    /// twin, `"maya mesh"` is no longer a query with one right answer.
    #[must_use]
    pub fn has_near_name_twin(&self, name: &str) -> bool {
        self.hard_negatives.iter().any(|negative| {
            negative.target == name && negative.kind == HardNegativeKind::SameDccNearName
        })
    }

    /// A [`SkillCatalog`] holding this corpus, ready to search.
    #[must_use]
    pub fn catalog(&self) -> SkillCatalog {
        let catalog = SkillCatalog::new(Arc::new(ToolRegistry::new()));
        for skill in &self.skills {
            catalog.add_skill(skill.clone());
        }
        catalog
    }
}

/// Drop later entries whose name repeats an earlier one, preserving order.
fn dedup_by_name(skills: &mut Vec<SkillMetadata>) {
    let mut seen = HashSet::new();
    skills.retain(|skill| seen.insert(skill.name.clone()));
}

/// Pick the `chosen` query targets: every real seed first, then an even spread
/// across the synthetic remainder.
fn select_targets(base: &[SkillMetadata], seeds_len: usize, chosen: usize) -> Vec<String> {
    if chosen == 0 {
        return Vec::new();
    }
    let mut targets: Vec<String> = base
        .iter()
        .take(seeds_len.min(chosen))
        .map(|skill| skill.name.clone())
        .collect();

    let synthetic = &base[seeds_len.min(base.len())..];
    if synthetic.is_empty() {
        return targets;
    }
    let remaining = chosen.saturating_sub(targets.len());
    // Even spread (`step`) keeps the sample representative of the whole
    // synthetic range instead of clustering at its start.
    let step = (synthetic.len() / remaining.max(1)).max(1);
    for index in (0..synthetic.len()).step_by(step) {
        if targets.len() >= chosen {
            break;
        }
        targets.push(synthetic[index].name.clone());
    }
    targets
}

/// Pick up to `limit` clean control targets: an even spread across the base,
/// excluding everything that already carries a twin.
fn select_clean_targets(
    base: &[SkillMetadata],
    exclude: &HashSet<String>,
    limit: usize,
) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let candidates: Vec<&SkillMetadata> = base
        .iter()
        .filter(|skill| !exclude.contains(&skill.name))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let step = (candidates.len() / limit).max(1);
    (0..candidates.len())
        .step_by(step)
        .take(limit)
        .map(|index| candidates[index].name.clone())
        .collect()
}

/// Build the twins for `targets`, skipping any that would collide with a name
/// already in `base`.
fn build_twins(
    base: &[SkillMetadata],
    targets: &[String],
) -> (Vec<SkillMetadata>, Vec<HardNegative>) {
    let mut taken: HashSet<String> = base.iter().map(|skill| skill.name.clone()).collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(synthetic::SEED ^ SAME_DCC_TWIN_INDEX as u64);
    let mut twins = Vec::new();
    let mut negatives = Vec::new();

    for (ordinal, target_name) in targets.iter().enumerate() {
        let Some(target) = base.iter().find(|skill| skill.name == *target_name) else {
            continue;
        };

        if let Some(twin) = near_name_twin(target, &mut rng)
            && taken.insert(twin.name.clone())
        {
            negatives.push(HardNegative {
                twin: twin.name.clone(),
                target: target_name.clone(),
                kind: HardNegativeKind::SameDccNearName,
            });
            twins.push(twin);
        }

        if let Some(twin) = cross_dcc_twin(target, ordinal)
            && taken.insert(twin.name.clone())
        {
            negatives.push(HardNegative {
                twin: twin.name.clone(),
                target: target_name.clone(),
                kind: HardNegativeKind::CrossDccSameContent,
            });
            twins.push(twin);
        }
    }

    (twins, negatives)
}

/// Same DCC, and one name is a prefix of the other.
///
/// Content is drawn independently so the pair is a *name* confusion: a query
/// built from the target's description still has a correct answer.
fn near_name_twin(target: &SkillMetadata, rng: &mut impl rand::RngExt) -> Option<SkillMetadata> {
    let near_name = near_name(&target.name)?;
    if near_name == target.name {
        return None;
    }
    let mut twin = synthetic::synthetic_skill(SAME_DCC_TWIN_INDEX, rng);
    twin.name = near_name;
    twin.dcc = target.dcc.clone();
    // `synthetic_skill` derives aliases from its generated name.
    twin.search_aliases.clear();
    Some(twin)
}

/// Draw index for twin content — far outside the base corpus range so twin
/// bodies never reuse a base skill's exact text by accident.
const SAME_DCC_TWIN_INDEX: usize = 900_000;

/// Drop the last `-` segment, or extend with `-lite` when too short to trim.
/// Either way one name is a prefix of the other.
fn near_name(name: &str) -> Option<String> {
    let segments: Vec<&str> = name.split('-').filter(|part| !part.is_empty()).collect();
    if segments.len() >= 3 {
        Some(segments[..segments.len() - 1].join("-"))
    } else if segments.len() == 2 {
        Some(format!("{name}-lite"))
    } else {
        None
    }
}

/// Identical indexed content under a different DCC prefix.
fn cross_dcc_twin(target: &SkillMetadata, ordinal: usize) -> Option<SkillMetadata> {
    let other_dcc = other_dcc(&target.dcc, ordinal)?;
    let base_name = strip_dcc_prefix(&target.name, &target.dcc);
    let twin_name = format!("{other_dcc}-{base_name}");
    if twin_name == target.name {
        return None;
    }
    let mut twin = target.clone();
    twin.name = twin_name;
    twin.dcc = other_dcc;
    Some(twin)
}

/// Pick a DCC other than `dcc`, rotating by `ordinal` so twins spread across
/// hosts instead of piling onto one.
fn other_dcc(dcc: &str, ordinal: usize) -> Option<String> {
    let known = DCCS.contains(&dcc);
    if known {
        let offset = 1 + (ordinal % (DCCS.len() - 1));
        let index = DCCS.iter().position(|value| *value == dcc)?;
        Some(DCCS[(index + offset) % DCCS.len()].to_string())
    } else {
        // `any` and other non-host buckets: any host is a valid contrast.
        Some(DCCS[ordinal % DCCS.len()].to_string())
    }
}

/// Remove a leading DCC prefix from `name` when it is present.
fn strip_dcc_prefix<'a>(name: &'a str, dcc: &str) -> &'a str {
    let prefix = format!("{dcc}-");
    name.strip_prefix(&prefix).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_lands_on_the_requested_scale() {
        for scale in [SCALE_300, SCALE_1000] {
            let corpus = Corpus::build(scale);
            assert_eq!(corpus.skills.len(), scale, "scale {scale}");
        }
    }

    #[test]
    fn every_name_is_unique() {
        let corpus = Corpus::build(SCALE_300);
        let unique: HashSet<&str> = corpus.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(unique.len(), corpus.skills.len(), "duplicate skill names");
    }

    #[test]
    fn hard_negatives_are_injected_for_both_shapes() {
        let corpus = Corpus::build(SCALE_300);
        let kinds: HashSet<HardNegativeKind> =
            corpus.hard_negatives.iter().map(|n| n.kind).collect();
        assert!(kinds.contains(&HardNegativeKind::SameDccNearName));
        assert!(kinds.contains(&HardNegativeKind::CrossDccSameContent));
    }

    #[test]
    fn near_name_twins_share_a_prefix_with_their_target() {
        let corpus = Corpus::build(SCALE_300);
        for negative in &corpus.hard_negatives {
            if negative.kind != HardNegativeKind::SameDccNearName {
                continue;
            }
            let (short, long) = if negative.twin.len() <= negative.target.len() {
                (negative.twin.as_str(), negative.target.as_str())
            } else {
                (negative.target.as_str(), negative.twin.as_str())
            };
            assert!(
                long.starts_with(short),
                "`{}` is not a prefix relation with `{}`",
                short,
                long
            );
        }
    }

    #[test]
    fn cross_dcc_twins_are_content_identical_but_for_dcc() {
        let corpus = Corpus::build(SCALE_300);
        for negative in &corpus.hard_negatives {
            if negative.kind != HardNegativeKind::CrossDccSameContent {
                continue;
            }
            let target = corpus.get(&negative.target).expect("target present");
            let twin = corpus.get(&negative.twin).expect("twin present");
            assert_ne!(target.dcc, twin.dcc);
            assert_eq!(target.description, twin.description);
            assert_eq!(target.tags, twin.tags);
        }
    }

    #[test]
    fn dcc_filter_removes_the_cross_dcc_twin() {
        // The claim the split rests on: a cross-DCC twin carries the target's
        // content under another host prefix, so only the per-DCC shard can
        // exclude it. Without the filter the pair competes; with it, the twin
        // is not even a candidate.
        let corpus = Corpus::build(SCALE_300);
        let catalog = corpus.catalog();
        let negative = corpus
            .hard_negatives
            .iter()
            .find(|n| n.kind == HardNegativeKind::CrossDccSameContent)
            .expect("a cross-DCC pair exists");
        let target = corpus.get(&negative.target).unwrap();

        let query = target.description.clone();
        let filtered = catalog.search_skills(Some(&query), &[], Some(&target.dcc), None, Some(50));
        let names: Vec<&str> = filtered.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&negative.target.as_str()),
            "target missing from its own shard: {names:?}"
        );
        assert!(
            !names.contains(&negative.twin.as_str()),
            "twin leaked through the dcc shard: {names:?}"
        );
    }

    #[test]
    fn near_name_derives_both_directions() {
        assert_eq!(near_name("maya-mesh-ops").as_deref(), Some("maya-mesh"));
        assert_eq!(near_name("maya-rig").as_deref(), Some("maya-rig-lite"));
        assert_eq!(near_name("solo").as_deref(), None);
    }

    #[test]
    fn other_dcc_never_returns_the_input() {
        for dcc in DCCS.iter() {
            for ordinal in 0..8 {
                let other = other_dcc(dcc, ordinal).unwrap();
                assert_ne!(other, *dcc);
            }
        }
        assert_ne!(other_dcc("any", 0).unwrap(), "any");
    }

    #[test]
    fn targets_are_present_in_the_corpus() {
        let corpus = Corpus::build(SCALE_300);
        for target in corpus.targets() {
            assert!(corpus.get(target).is_some(), "missing target {target}");
        }
    }

    #[test]
    fn targets_split_into_hard_and_clean_halves() {
        // Both halves must be populated, otherwise the hard-vs-clean
        // comparison in the report is vacuous.
        let corpus = Corpus::build(SCALE_300);
        let hard = corpus.hard_negative_targets();
        let clean = corpus
            .targets()
            .iter()
            .filter(|name| !hard.contains(name.as_str()))
            .count();
        assert!(!hard.is_empty(), "no hard-negative targets");
        assert!(clean > 0, "no clean control targets");
    }
}
