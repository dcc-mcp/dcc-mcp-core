//! Pluggable ranking strategies for the capability search layer
//! (issues [#659] / [#765]).
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
//! ## Strategy seam (issue [#765])
//!
//! [`StrategyScorer`] is the **public extension point** for embedders
//! that want to plug in a custom ranking algorithm (BM25, embedding
//! vectors, studio lexicons, …) without touching the handler code.
//! [`ScorerFactory`] selects a concrete `StrategyScorer` box from a
//! [`super::search::SearchMode`] variant or a free-form string tag so
//! callers outside this crate can use the same dispatch logic.
//!
//! The two built-in implementations are:
//!
//! * [`FuzzyScorer`] / [`StrategyFuzzyScorer`] — wraps `nucleo-matcher`
//!   (the Helix editor's fuzzy engine) and adds prefix bonuses plus
//!   multi-field weighting per the #659 acceptance criteria.
//! * [`SubstringScorer`] / [`ExactScorer`] — the original exact/substring
//!   table, preserved byte-for-byte so regressions surface in dedicated
//!   unit tests rather than in integration tests that happen to
//!   exercise search.
//!
//! Both internal scorers return scores on the **same scale** (`0` = no
//! match, higher = better) so the zero-filter in the search loop
//! stays valid for either strategy.
//!
//! [#659]: https://github.com/loonghao/dcc-mcp-core/issues/659
//! [#765]: https://github.com/loonghao/dcc-mcp-core/issues/765

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use super::record::CapabilityRecord;
use super::search::SearchMode;

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
    #[must_use]
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

/// `SubstringScorer` re-exported under the name used in issue #765
/// acceptance criteria ("ExactScorer"). The implementation is identical
/// to [`SubstringScorer`]; the alias exists so external callers can
/// refer to the concept by a more descriptive name.
pub type ExactScorer = SubstringScorer;

// ============================================================================
// Public strategy seam (issue #765)
// ============================================================================

/// Simplified, `Send + Sync` scoring strategy intended for **embedders**
/// that want to plug a custom algorithm into the gateway without touching
/// the filter/pagination/sort pipeline.
///
/// The signature intentionally differs from the internal [`Scorer`] trait:
///
/// * `&self` — no mutable state required; thread-safe by contract.
/// * Plain `&str` inputs — the caller passes the pre-processed query and
///   a single candidate string (tool name, summary, …) rather than an
///   entire [`CapabilityRecord`].  This lets custom scorers be unit-tested
///   without constructing gateway-internal types.
/// * `f32` return — a normalised `[0.0, 1.0]` range is idiomatic for
///   pluggable ML/embedding scorers; the gateway quantises to `u32` before
///   writing a [`super::search::SearchHit`].
///
/// # Thread safety
///
/// Implementations **must** be `Send + Sync` so the same box can be
/// shared across the async Axum worker threads without wrapping in a
/// `Mutex`.
///
/// # Example
///
/// ```rust
/// use dcc_mcp_gateway_core::capability::search_ranking::StrategyScorer;
///
/// struct PrefixScorer;
///
/// impl StrategyScorer for PrefixScorer {
///     fn score(&self, query: &str, candidate: &str) -> f32 {
///         if candidate.starts_with(query) { 1.0 } else { 0.0 }
///     }
/// }
/// ```
pub trait StrategyScorer: Send + Sync {
    /// Return a relevance score for `candidate` relative to `query`.
    ///
    /// * `query` — lower-cased, trimmed free-text query from the caller.
    /// * `candidate` — one field extracted from a [`CapabilityRecord`]
    ///   (tool name, summary, tag, …); pre-processing is the caller's
    ///   responsibility.
    ///
    /// Return `0.0` to signal "no match"; any positive value to signal a
    /// match.  Values above `1.0` are accepted but the gateway clamps the
    /// quantised `u32` at `FUZZY_FIELD_CAP` / `10`, so saturating above
    /// that threshold has no effect on ranking.
    fn score(&self, query: &str, candidate: &str) -> f32;
}

/// [`StrategyScorer`] adapter backed by [`FuzzyScorer`] (nucleo-matcher).
///
/// Constructs a fresh internal [`FuzzyScorer`] per call so the adapter is
/// `Sync` without a `Mutex`. The allocation cost (~few KB) is negligible
/// compared with the nucleo scoring itself.
#[derive(Debug, Default, Clone, Copy)]
pub struct StrategyFuzzyScorer;

impl StrategyScorer for StrategyFuzzyScorer {
    fn score(&self, query: &str, candidate: &str) -> f32 {
        if query.is_empty() || candidate.is_empty() {
            return 0.0;
        }
        let pattern = Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf: Vec<char> = Vec::with_capacity(candidate.len());
        let hs = Utf32Str::new(candidate, &mut buf);
        let raw = pattern.score(hs, &mut matcher).unwrap_or(0);
        if raw == 0 {
            return 0.0;
        }
        let bucket = (raw / FUZZY_QUANTISE_DIVISOR).clamp(1, FUZZY_FIELD_CAP);
        bucket as f32 / FUZZY_FIELD_CAP as f32
    }
}

/// [`StrategyScorer`] adapter backed by [`SubstringScorer`] / [`ExactScorer`].
///
/// Mirrors the weight table in [`SubstringScorer`] but applied to a single
/// candidate string so it can satisfy the [`StrategyScorer`] contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct StrategyExactScorer;

impl StrategyScorer for StrategyExactScorer {
    fn score(&self, query: &str, candidate: &str) -> f32 {
        if query.is_empty() || candidate.is_empty() {
            return 0.0;
        }
        let cand_lower = candidate.to_ascii_lowercase();
        if cand_lower == query {
            1.0
        } else if cand_lower.contains(query) {
            0.6
        } else {
            0.0
        }
    }
}

/// Factory that constructs a boxed [`StrategyScorer`] from a
/// [`SearchMode`] variant or a free-form string tag.
///
/// This is the primary extension point for callers outside this crate:
/// they can match on [`SearchMode`] without knowing the concrete scorer
/// type, and add new strategies by extending the string-tag arm of
/// [`ScorerFactory::from_tag`].
///
/// # Example
///
/// ```rust
/// use dcc_mcp_gateway_core::capability::search::SearchMode;
/// use dcc_mcp_gateway_core::capability::search_ranking::ScorerFactory;
///
/// let scorer = ScorerFactory::from_mode(SearchMode::Fuzzy);
/// let s = scorer.score("sphere", "create_sphere");
/// assert!(s > 0.0);
/// ```
pub struct ScorerFactory;

impl ScorerFactory {
    /// Return a [`StrategyScorer`] box appropriate for `mode`.
    ///
    /// | `mode`              | scorer                   |
    /// |---------------------|--------------------------|
    /// | [`SearchMode::Fuzzy`] | [`StrategyFuzzyScorer`] |
    /// | [`SearchMode::Exact`] | [`StrategyExactScorer`] |
    #[must_use]
    pub fn from_mode(mode: SearchMode) -> Box<dyn StrategyScorer> {
        match mode {
            SearchMode::Fuzzy => Box::new(StrategyFuzzyScorer),
            SearchMode::Exact => Box::new(StrategyExactScorer),
        }
    }

    /// Return a [`StrategyScorer`] box identified by a free-form string tag.
    ///
    /// Recognised tags (case-insensitive):
    ///
    /// | tag        | scorer                   |
    /// |------------|--------------------------|
    /// | `"fuzzy"`  | [`StrategyFuzzyScorer`] |
    /// | `"exact"`  | [`StrategyExactScorer`] |
    ///
    /// Unknown tags fall back to [`StrategyFuzzyScorer`] so existing
    /// configurations that add a new tag do not silently break.
    #[must_use]
    pub fn from_tag(tag: &str) -> Box<dyn StrategyScorer> {
        match tag.to_ascii_lowercase().as_str() {
            "exact" | "substring" => Box::new(StrategyExactScorer),
            _ => Box::new(StrategyFuzzyScorer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::record::tool_slug;
    use uuid::Uuid;

    fn rec(
        name: &str,
        summary: &str,
        skill: Option<&str>,
        tags: &[&str],
        loaded: bool,
    ) -> CapabilityRecord {
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
            false, // has_schema
            loaded,
        )
    }

    #[test]
    fn substring_scorer_exact_tool_name() {
        let mut s = SubstringScorer;
        // "create_sphere" is also a substring of "create_sphere"; the
        // scorer takes the exact branch (10) and stops.
        let r = rec("create_sphere", "make a sphere", None, &["geo"], true);
        assert_eq!(s.score(&r, "create_sphere", None), 10);
    }

    #[test]
    fn substring_scorer_substring_plus_summary() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"], true);
        // Substring hit on the tool name (6) + summary contains
        // "sphere" (2) = 8.
        assert_eq!(s.score(&r, "sphere", None), 6 + 2);
    }

    #[test]
    fn substring_scorer_exact_tag() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "", None, &["geo"], true);
        assert_eq!(s.score(&r, "geo", None), 5);
    }

    #[test]
    fn substring_scorer_zero_on_miss() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"], true);
        assert_eq!(s.score(&r, "xylophone", None), 0);
    }

    #[test]
    fn fuzzy_scorer_tolerates_single_character_typo() {
        let mut s = FuzzyScorer::new();
        let r = rec("create_sphere", "", None, &[], true);
        // Missing final `e` in the needle — legacy substring matcher
        // would miss this entirely; fuzzy must produce a positive
        // score so the agent still sees the right tool.
        let typo_score = s.score(&r, "creat_spher", None);
        assert!(
            typo_score > 0,
            "fuzzy must tolerate a typo; got {typo_score}"
        );
        // An exact match still wins.
        let exact_score = s.score(
            &rec("create_sphere", "", None, &[], true),
            "create_sphere",
            None,
        );
        assert!(
            exact_score >= typo_score,
            "exact ({exact_score}) should outrank typo ({typo_score})",
        );
    }

    #[test]
    fn fuzzy_scorer_ranks_prefix_above_substring() {
        let mut s = FuzzyScorer::new();
        let prefix_hit = s.score(&rec("create_sphere", "", None, &[], true), "create", None);
        let substring_hit = s.score(&rec("recreate_plane", "", None, &[], true), "create", None);
        assert!(
            prefix_hit > substring_hit,
            "prefix match ({prefix_hit}) must outrank substring ({substring_hit})",
        );
    }

    #[test]
    fn fuzzy_scorer_matches_subsequence() {
        // Subsequence "cs" should match "create_sphere" (c…s).
        let mut s = FuzzyScorer::new();
        let hit = s.score(&rec("create_sphere", "", None, &[], true), "cs", None);
        assert!(hit > 0, "subsequence match should score > 0");
    }

    #[test]
    fn fuzzy_scorer_rejects_totally_unrelated_query() {
        let mut s = FuzzyScorer::new();
        // "xyzzy" shares nothing with "create_sphere" or "geometry"
        // so every field is a 0 fuzzy score and we fall through the
        // zero-filter.
        let hit = s.score(
            &rec("create_sphere", "geometry tool", None, &[], true),
            "xyzzy",
            None,
        );
        assert_eq!(hit, 0);
    }

    #[test]
    fn fuzzy_scorer_weights_tool_name_above_summary() {
        let mut s = FuzzyScorer::new();
        let via_tool = s.score(&rec("keyframe", "", None, &[], true), "keyframe", None);
        let via_summary = s.score(
            &rec("unrelated", "keyframe in summary", None, &[], true),
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
        let hit = s.score(&rec("misc", "", None, &["animation"], true), "anim", None);
        assert!(hit > 0, "tag fuzzy match must contribute; got {hit}");
    }

    #[test]
    fn fuzzy_scorer_credits_schema_field_tag() {
        let mut s = FuzzyScorer::new();
        let hit = s.score(
            &rec(
                "set_anim",
                "",
                None,
                &["schema:frame", "schema:value"],
                true,
            ),
            "frame",
            None,
        );
        assert!(hit > 0, "schema:<prop> tag must participate in ranking");
    }

    #[test]
    fn scene_hint_adds_boost_even_without_query() {
        let mut s = FuzzyScorer::new();
        let hit = s.score(
            &rec("open", "rig scene ready", None, &["scene"], true),
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
            true,
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
