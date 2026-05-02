//! Pluggable ranking strategies for the capability search layer
//! (issue [#659]).
//!
//! The original search ([`super::search`]) shipped a hand-rolled
//! substring scorer. That gave correct answers for exact queries
//! but silently returned zero hits for typos and partial tokens — a
//! poor fit for agent workflows where the model often names the
//! tool it wants from memory rather than from the catalogue.
//!
//! Rather than rewriting the scoring table in place (which would
//! leak fuzzy-matcher details into the [`super::search::SearchQuery`]
//! wire type and fight the existing deterministic tie-breaker), this
//! module factors scoring behind a [`Scorer`] trait so the index can
//! swap strategies without touching the filter / pagination / sort
//! pipeline. This is the
//! [Open/Closed Principle](https://en.wikipedia.org/wiki/Open%E2%80%93closed_principle)
//! applied to ranking — adding a future Tantivy- or FST-backed
//! scorer only requires implementing the trait.
//!
//! The two scorers shipped today are:
//!
//! * [`SubstringScorer`] — the original exact/substring table,
//!   preserved byte-for-byte so regressions surface in dedicated
//!   unit tests rather than in integration tests that happen to
//!   exercise search.
//! * [`FuzzyScorer`] — wraps `nucleo-matcher` (the Helix editor's
//!   fuzzy engine) and adds prefix bonuses plus multi-field
//!   weighting per the #659 acceptance criteria.
//!
//! Both scorers return scores on the **same scale** (`0` = no match,
//! higher = better) so the zero-filter in [`super::search::search`]
//! stays valid for either strategy.
//!
//! [#659]: https://github.com/loonghao/dcc-mcp-core/issues/659

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use super::record::CapabilityRecord;

/// A pluggable scoring strategy for the capability index.
///
/// Implementations are constructed once per search call — the
/// `nucleo-matcher` buffers allocated inside [`FuzzyScorer`] are
/// cheap (~few KB) and per-call construction avoids shared-mutability
/// concerns with the `&RwLock` the index is guarded by.
pub trait Scorer {
    /// Return the score of `record` against the pre-lowercased
    /// query `q`. `0` means no match — search drops the row. Higher
    /// wins.
    ///
    /// `scene_hint` is a **soft boost**: matches against summary/tags
    /// add to the score but do not keep a row alive on their own
    /// when the primary `q` does not match.
    fn score(&mut self, record: &CapabilityRecord, q: &str, scene_hint: Option<&str>) -> u32;
}

/// Legacy substring scorer — kept so callers that explicitly opt
/// out of fuzzy matching (deterministic golden tests, future
/// surfaces that want predictable scoring) can stay on the old
/// table.
///
/// The scoring weights match the pre-#659 behaviour byte-for-byte.
#[derive(Debug, Default, Clone, Copy)]
pub struct SubstringScorer;

impl Scorer for SubstringScorer {
    fn score(&mut self, r: &CapabilityRecord, q: &str, scene_hint: Option<&str>) -> u32 {
        let mut score: u32 = 0;

        if !q.is_empty() {
            let tool_lower = r.backend_tool.to_ascii_lowercase();
            if tool_lower == q {
                score += 10;
            } else if tool_lower.contains(q) {
                score += 6;
            }
            if r.tags.iter().any(|t| t.to_ascii_lowercase() == q) {
                score += 5;
            }
            if r.skill_name
                .as_deref()
                .is_some_and(|s| s.to_ascii_lowercase().contains(q))
            {
                score += 4;
            }
            if r.summary.to_ascii_lowercase().contains(q) {
                score += 2;
            }
        }

        if let Some(hint) = scene_hint
            && (r.summary.to_ascii_lowercase().contains(hint)
                || r.tags.iter().any(|t| t.to_ascii_lowercase() == hint))
        {
            score += 2;
        }

        score
    }
}

/// Max contribution from a single `nucleo-matcher` score, after
/// quantisation. Keeping each field's ceiling modest prevents a
/// freakishly long tool name from dominating the ranking.
const FUZZY_FIELD_CAP: u32 = 10;
/// Extra score when the query is a **prefix** of the backend tool
/// name — mirrors the interactive fuzzy UX from Helix / fzf where
/// prefix matches surface first.
const PREFIX_BONUS: u32 = 4;
/// Exact-match bonus (stacked on top of the fuzzy score) so the
/// ordering invariant `exact > prefix > fuzzy` holds.
const EXACT_BONUS: u32 = 20;
/// Divisor used to bin `nucleo-matcher`'s raw score into the
/// `0..FUZZY_FIELD_CAP` bucket. Nucleo typically returns scores in
/// the ~40–300 range for the short strings we feed it (tool names,
/// one-line summaries); a `/32` divisor puts those into 1–10
/// buckets which preserves the ordering while collapsing
/// fingerprint-level jitter.
const FUZZY_QUANTISE_DIVISOR: u32 = 32;

/// Fuzzy scorer built on top of `nucleo-matcher`.
///
/// Scoring contributions per field (bounded, additive):
///
/// | Signal                                | Max |
/// |---------------------------------------|-----|
/// | Exact match on `backend_tool`         | 20  |
/// | Prefix of `backend_tool`              |  4  |
/// | Fuzzy on `backend_tool`               | 10  |
/// | Fuzzy on `skill_name`                 |  7  |
/// | Fuzzy on best-matching `tag`          |  6  |
/// | Fuzzy on `summary`                    |  5  |
/// | Fuzzy on a `schema:<prop>` tag        |  4  |
/// | Scene/document hint match on summary  |  2  |
///
/// The exact/prefix bonuses stack *on top of* the fuzzy score, so
/// `create_sphere` vs the query `create` always beats
/// `sphere_creation_helper` even though both fuzzy-match.
pub struct FuzzyScorer {
    matcher: Matcher,
    /// Reused UTF-32 codepoint buffer — `Utf32Str::new` fills this
    /// when the haystack is not ASCII. Keeping it on the scorer
    /// means we allocate it once per search call, not once per
    /// record * field.
    haystack_buf: Vec<char>,
}

impl FuzzyScorer {
    /// Build a scorer with nucleo's default configuration
    /// (case-insensitive, standard ASCII/Unicode rules).
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            haystack_buf: Vec::with_capacity(64),
        }
    }

    /// Compile a needle once per `score()` call. `AtomKind::Fuzzy`
    /// matches characters in order with arbitrary gaps — exactly the
    /// behaviour agents expect from a "type part of the name" flow.
    fn compile_pattern(q: &str) -> Pattern {
        Pattern::new(
            q,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        )
    }

    /// Quantised nucleo score for one haystack field. Returns `0`
    /// when the needle does not match so callers can treat the
    /// result as zero-is-falsy.
    fn score_field(
        &mut self,
        pattern: &Pattern,
        haystack: &str,
        cap: u32,
        exact_override: bool,
    ) -> u32 {
        if haystack.is_empty() {
            return 0;
        }
        let hs = Utf32Str::new(haystack, &mut self.haystack_buf);
        let raw = pattern.score(hs, &mut self.matcher).unwrap_or(0);
        if raw == 0 {
            return 0;
        }
        let mut bucket = (raw / FUZZY_QUANTISE_DIVISOR).min(cap);
        if bucket == 0 {
            // Keep weak-but-positive matches distinguishable from
            // "no match" so the zero-filter in `search()` still
            // drops them only when every field fails.
            bucket = 1;
        }
        if exact_override {
            // Exact tool-name matches go to the top of the field's
            // bucket regardless of nucleo's internal ceiling.
            bucket = cap;
        }
        bucket
    }
}

impl Default for FuzzyScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl Scorer for FuzzyScorer {
    fn score(&mut self, r: &CapabilityRecord, q: &str, scene_hint: Option<&str>) -> u32 {
        let mut score: u32 = 0;

        if !q.is_empty() {
            let pattern = Self::compile_pattern(q);
            let tool_lower = r.backend_tool.to_ascii_lowercase();
            let exact_tool = tool_lower == q;
            let prefix_tool = !exact_tool && tool_lower.starts_with(q);

            score += self.score_field(&pattern, &r.backend_tool, FUZZY_FIELD_CAP, exact_tool);
            if exact_tool {
                score += EXACT_BONUS;
            } else if prefix_tool {
                score += PREFIX_BONUS;
            }

            if let Some(skill) = r.skill_name.as_deref() {
                score += self.score_field(&pattern, skill, 7, false);
            }

            // Credit only the best-matching free tag so a record
            // tagged with the same word three times does not unfairly
            // dominate. Walk tags once picking the max contribution.
            let mut best_tag = 0;
            for tag in &r.tags {
                if tag.starts_with("schema:") {
                    continue;
                }
                let s = self.score_field(&pattern, tag, 6, false);
                if s > best_tag {
                    best_tag = s;
                }
            }
            score += best_tag;

            score += self.score_field(&pattern, &r.summary, 5, false);

            // Schema property names: the builder encodes them as
            // `schema:<prop>` tags (see #659 acceptance criterion
            // "schema field names"). Credit the best schema-tag
            // match only so a multi-field action does not unfairly
            // collect dozens of points.
            let mut best_schema = 0;
            for tag in &r.tags {
                if let Some(stripped) = tag.strip_prefix("schema:") {
                    let s = self.score_field(&pattern, stripped, 4, false);
                    if s > best_schema {
                        best_schema = s;
                    }
                }
            }
            score += best_schema;
        }

        if let Some(hint) = scene_hint
            && (r.summary.to_ascii_lowercase().contains(hint)
                || r.tags.iter().any(|t| t.to_ascii_lowercase() == hint))
        {
            score += 2;
        }

        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::capability::record::tool_slug;
    use uuid::Uuid;

    fn rec(name: &str, summary: &str, skill: Option<&str>, tags: &[&str]) -> CapabilityRecord {
        let iid = Uuid::from_u128(0x1234_5678_0000_0000_0000_0000_0000_0001);
        CapabilityRecord::new(
            tool_slug("maya", &iid, name),
            name.to_string(),
            name.to_string(),
            skill.map(String::from),
            summary,
            tags.iter().map(|t| t.to_string()).collect(),
            "maya".to_string(),
            iid,
            false,
        )
    }

    #[test]
    fn substring_scorer_exact_tool_name() {
        let mut s = SubstringScorer;
        // "create_sphere" is also a substring of "create_sphere"; the
        // scorer takes the exact branch (10) and stops.
        let r = rec("create_sphere", "make a sphere", None, &["geo"]);
        assert_eq!(s.score(&r, "create_sphere", None), 10);
    }

    #[test]
    fn substring_scorer_substring_plus_summary() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"]);
        // Substring hit on the tool name (6) + summary contains
        // "sphere" (2) = 8.
        assert_eq!(s.score(&r, "sphere", None), 6 + 2);
    }

    #[test]
    fn substring_scorer_exact_tag() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "", None, &["geo"]);
        assert_eq!(s.score(&r, "geo", None), 5);
    }

    #[test]
    fn substring_scorer_zero_on_miss() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"]);
        assert_eq!(s.score(&r, "xylophone", None), 0);
    }

    #[test]
    fn fuzzy_scorer_tolerates_single_character_typo() {
        let mut s = FuzzyScorer::new();
        let r = rec("create_sphere", "", None, &[]);
        // Missing final `e` in the needle — legacy substring matcher
        // would miss this entirely; fuzzy must produce a positive
        // score so the agent still sees the right tool.
        let typo_score = s.score(&r, "creat_spher", None);
        assert!(
            typo_score > 0,
            "fuzzy must tolerate a typo; got {typo_score}"
        );
        // An exact match still wins.
        let exact_score = s.score(&rec("create_sphere", "", None, &[]), "create_sphere", None);
        assert!(
            exact_score >= typo_score,
            "exact ({exact_score}) should outrank typo ({typo_score})",
        );
    }

    #[test]
    fn fuzzy_scorer_ranks_prefix_above_substring() {
        let mut s = FuzzyScorer::new();
        let prefix_hit = s.score(&rec("create_sphere", "", None, &[]), "create", None);
        let substring_hit = s.score(&rec("recreate_plane", "", None, &[]), "create", None);
        assert!(
            prefix_hit > substring_hit,
            "prefix match ({prefix_hit}) must outrank substring ({substring_hit})",
        );
    }

    #[test]
    fn fuzzy_scorer_matches_subsequence() {
        // Subsequence "cs" should match "create_sphere" (c…s).
        let mut s = FuzzyScorer::new();
        let hit = s.score(&rec("create_sphere", "", None, &[]), "cs", None);
        assert!(hit > 0, "subsequence match should score > 0");
    }

    #[test]
    fn fuzzy_scorer_rejects_totally_unrelated_query() {
        let mut s = FuzzyScorer::new();
        // "xyzzy" shares nothing with "create_sphere" or "geometry"
        // so every field is a 0 fuzzy score and we fall through the
        // zero-filter.
        let hit = s.score(
            &rec("create_sphere", "geometry tool", None, &[]),
            "xyzzy",
            None,
        );
        assert_eq!(hit, 0);
    }

    #[test]
    fn fuzzy_scorer_weights_tool_name_above_summary() {
        let mut s = FuzzyScorer::new();
        let via_tool = s.score(&rec("keyframe", "", None, &[]), "keyframe", None);
        let via_summary = s.score(
            &rec("unrelated", "keyframe in summary", None, &[]),
            "keyframe",
            None,
        );
        assert!(
            via_tool > via_summary,
            "tool-name match ({via_tool}) should outweigh summary-only match ({via_summary})",
        );
    }

    #[test]
    fn fuzzy_scorer_credits_tag_match() {
        let mut s = FuzzyScorer::new();
        // `anim` is a fuzzy prefix of `animation` — tag scoring must
        // return > 0.
        let hit = s.score(&rec("misc", "", None, &["animation"]), "anim", None);
        assert!(hit > 0, "tag fuzzy match must contribute; got {hit}");
    }

    #[test]
    fn fuzzy_scorer_credits_schema_field_tag() {
        let mut s = FuzzyScorer::new();
        let hit = s.score(
            &rec("set_anim", "", None, &["schema:frame", "schema:value"]),
            "frame",
            None,
        );
        assert!(hit > 0, "schema:<prop> tag must participate in ranking");
    }

    #[test]
    fn scene_hint_adds_boost_even_without_query() {
        let mut s = FuzzyScorer::new();
        let hit = s.score(
            &rec("open", "rig scene ready", None, &["scene"]),
            "",
            Some("scene"),
        );
        assert_eq!(hit, 2);
    }

    #[test]
    fn fuzzy_scores_are_deterministic_across_runs() {
        // Determinism guard for #659 acceptance criterion
        // "ranking is documented and deterministic". Build a fresh
        // matcher each iteration to catch state-leakage bugs.
        let r = rec(
            "create_sphere",
            "makes a sphere",
            Some("maya-geo"),
            &["geo"],
        );
        let scores: Vec<u32> = (0..16)
            .map(|_| FuzzyScorer::new().score(&r, "sphere", None))
            .collect();
        assert!(
            scores.windows(2).all(|w| w[0] == w[1]),
            "fuzzy score fluctuated across runs: {scores:?}",
        );
    }
}
