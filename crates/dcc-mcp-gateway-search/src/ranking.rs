//! Pluggable ranking strategies (issues #659 / #765).
//!
//! Built-in scorers operate on [`crate::SearchRecord`] so the same ranking
//! logic can target any compact row type the gateway indexes.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use crate::query::{ScoreBreakdown, SearchMode};
use crate::record::SearchRecord;

/// A pluggable scoring strategy for the capability index.
pub trait Scorer {
    /// Return the score and match reasons of `record` against the pre-lowercased
    /// query `q`. A score of `0` means no match.
    fn explain(
        &mut self,
        record: &dyn SearchRecord,
        q: &str,
        scene_hint: Option<&str>,
    ) -> ScoreBreakdown;

    /// Return the score of `record` against the pre-lowercased query `q`.
    /// `0` means no match — search drops the row.
    fn score(&mut self, record: &dyn SearchRecord, q: &str, scene_hint: Option<&str>) -> u32 {
        self.explain(record, q, scene_hint).score
    }
}

fn add_reason(reasons: &mut Vec<String>, reason: &'static str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn add_component(breakdown: &mut ScoreBreakdown, amount: u32, reason: &'static str) {
    if amount == 0 {
        return;
    }
    breakdown.score = breakdown.score.saturating_add(amount);
    add_reason(&mut breakdown.match_reasons, reason);
}

/// Legacy substring scorer (pre-#659 table).
#[derive(Debug, Default, Clone, Copy)]
pub struct SubstringScorer;

impl Scorer for SubstringScorer {
    fn explain(
        &mut self,
        r: &dyn SearchRecord,
        q: &str,
        scene_hint: Option<&str>,
    ) -> ScoreBreakdown {
        let mut out = ScoreBreakdown::default();

        if !q.is_empty() {
            let tool_lower = r.backend_tool().to_ascii_lowercase();
            if tool_lower == q {
                add_component(&mut out, 10, "tool_exact");
            } else if tool_lower.contains(q) {
                add_component(&mut out, 6, "tool_substring");
            }
            if r.tags().iter().any(|t| t.to_ascii_lowercase() == *q) {
                add_component(&mut out, 5, "tag_exact");
            }
            if r.skill_name()
                .is_some_and(|s| s.to_ascii_lowercase().contains(q))
            {
                add_component(&mut out, 4, "skill_substring");
            }
            if r.summary().to_ascii_lowercase().contains(q) {
                add_component(&mut out, 2, "summary_substring");
            }
        }

        if let Some(hint) = scene_hint
            && (r.summary().to_ascii_lowercase().contains(hint)
                || r.tags().iter().any(|t| t.to_ascii_lowercase() == *hint))
        {
            add_component(&mut out, 2, "scene_hint");
        }

        out
    }
}

const FUZZY_FIELD_CAP: u32 = 10;
const PREFIX_BONUS: u32 = 4;
const EXACT_BONUS: u32 = 20;
const FUZZY_QUANTISE_DIVISOR: u32 = 32;
const AMBIGUOUS_SHORT_QUERY_LEN: usize = 3;

fn relaxed_multiword_haystack_score(r: &dyn SearchRecord, q: &str) -> u32 {
    let q = q.trim();
    if !q.chars().any(char::is_whitespace) {
        return 0;
    }
    if q.len() < 8 {
        return 0;
    }

    let mut hay = String::new();
    hay.push_str(&r.backend_tool().to_ascii_lowercase());
    hay.push(' ');
    hay.push_str(&r.summary().to_ascii_lowercase());
    if let Some(s) = r.skill_name() {
        hay.push(' ');
        hay.push_str(&s.to_ascii_lowercase());
    }
    for t in r.tags() {
        hay.push(' ');
        hay.push_str(&t.to_ascii_lowercase());
    }
    for t in r.search_tokens() {
        hay.push(' ');
        hay.push_str(&search_token_text(t).to_ascii_lowercase());
    }

    let words: Vec<&str> = q.split_whitespace().filter(|w| w.len() >= 2).collect();
    if words.len() < 2 {
        return 0;
    }

    let hay_tokens: Vec<String> = hay
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let mut matched: u32 = 0;
    for w in words {
        let wl = w.to_ascii_lowercase();
        if hay.contains(wl.as_str()) {
            matched = matched.saturating_add(1);
        } else if !hay_tokens.is_empty() {
            let hit_token = hay_tokens
                .iter()
                .any(|t| t.starts_with(wl.as_str()) || (wl.len() >= 3 && t.contains(wl.as_str())));
            if hit_token {
                matched = matched.saturating_add(1);
            }
        }
    }
    if matched == 0 {
        return 0;
    }
    (matched * 5).min(30)
}

fn search_token_text(token: &str) -> &str {
    token
        .strip_prefix("alias:")
        .or_else(|| token.strip_prefix("schema:"))
        .or_else(|| token.strip_prefix("required:"))
        .unwrap_or(token)
}

fn search_tokens_joined<'a>(tokens: impl Iterator<Item = &'a String>) -> String {
    let mut out = String::new();
    for token in tokens {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(search_token_text(token));
    }
    out
}

fn search_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if prev_lower && ch.is_ascii_uppercase() && !current.is_empty() {
                tokens.push(current.to_ascii_lowercase());
                current.clear();
            }
            current.push(ch);
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !current.is_empty() {
                tokens.push(current.to_ascii_lowercase());
                current.clear();
            }
            prev_lower = false;
        }
    }

    if !current.is_empty() {
        tokens.push(current.to_ascii_lowercase());
    }

    tokens
}

fn token_match_score(query: &str, candidate: &str, exact: u32, prefix: u32, substring: u32) -> u32 {
    if query.is_empty() || candidate.is_empty() {
        return 0;
    }

    let candidate_lower = candidate.to_ascii_lowercase();
    if candidate_lower == query {
        return exact;
    }
    if candidate_lower.contains(query) {
        return substring;
    }

    let query_tokens = search_tokens(query);
    let candidate_tokens = search_tokens(candidate);
    if query_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0;
    }

    let mut total = 0;
    for qtok in &query_tokens {
        let token_score = candidate_tokens
            .iter()
            .map(|ctok| {
                if ctok == qtok {
                    exact
                } else if ctok.starts_with(qtok) {
                    prefix
                } else if ctok.contains(qtok) {
                    substring
                } else {
                    0
                }
            })
            .max()
            .unwrap_or(0);
        if token_score == 0 {
            return 0;
        }
        total += token_score;
    }

    total
}

/// Fuzzy scorer built on `nucleo-matcher`.
pub struct FuzzyScorer {
    matcher: Matcher,
    haystack_buf: Vec<char>,
}

impl FuzzyScorer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            haystack_buf: Vec::with_capacity(64),
        }
    }

    fn compile_pattern(q: &str) -> Pattern {
        Pattern::new(
            q,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        )
    }

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
            bucket = 1;
        }
        if exact_override {
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
    fn explain(
        &mut self,
        r: &dyn SearchRecord,
        q: &str,
        scene_hint: Option<&str>,
    ) -> ScoreBreakdown {
        let mut out = ScoreBreakdown::default();

        if !q.is_empty() {
            let tool_lexical = token_match_score(q, r.backend_tool(), 18, 12, 8);
            let summary_lexical = token_match_score(q, r.summary(), 8, 5, 3);
            let alias_tokens =
                search_tokens_joined(r.search_tokens().iter().filter(|t| t.starts_with("alias:")));
            let schema_tokens = search_tokens_joined(
                r.search_tokens()
                    .iter()
                    .filter(|t| t.starts_with("schema:") || t.starts_with("required:")),
            );
            let generic_tokens = search_tokens_joined(r.search_tokens().iter().filter(|t| {
                !t.starts_with("alias:") && !t.starts_with("schema:") && !t.starts_with("required:")
            }));
            let alias_lexical = token_match_score(q, &alias_tokens, 14, 10, 6);
            let schema_lexical = token_match_score(q, &schema_tokens, 12, 8, 5);
            let search_token_lexical = token_match_score(q, &generic_tokens, 10, 7, 4);
            let mut deterministic = tool_lexical
                + summary_lexical
                + alias_lexical
                + schema_lexical
                + search_token_lexical;
            add_component(&mut out, tool_lexical, "tool_lexical");
            add_component(&mut out, summary_lexical, "summary_lexical");
            add_component(&mut out, alias_lexical, "alias_lexical");
            add_component(&mut out, schema_lexical, "schema_lexical");
            add_component(&mut out, search_token_lexical, "search_token_lexical");
            if deterministic == 0 {
                let multiword = relaxed_multiword_haystack_score(r, q);
                deterministic = deterministic.max(multiword);
                add_component(&mut out, multiword, "multi_token_lexical");
            }

            if q.len() == AMBIGUOUS_SHORT_QUERY_LEN && deterministic == 0 {
                return ScoreBreakdown::default();
            }

            let pattern = Self::compile_pattern(q);
            let tool_lower = r.backend_tool().to_ascii_lowercase();
            let exact_tool = tool_lower == q;
            let prefix_tool = !exact_tool && tool_lower.starts_with(q);

            let tool_fuzzy =
                self.score_field(&pattern, r.backend_tool(), FUZZY_FIELD_CAP, exact_tool);
            add_component(&mut out, tool_fuzzy, "tool_fuzzy");
            if exact_tool {
                add_component(&mut out, EXACT_BONUS, "tool_exact");
            } else if prefix_tool {
                add_component(&mut out, PREFIX_BONUS, "tool_prefix");
            }

            if let Some(skill) = r.skill_name() {
                let skill_score = self.score_field(&pattern, skill, 7, false);
                add_component(&mut out, skill_score, "skill_fuzzy");
            }

            let mut best_tag = 0;
            for tag in r.tags() {
                if tag.starts_with("schema:") {
                    continue;
                }
                let s = self.score_field(&pattern, tag, 6, false);
                if s > best_tag {
                    best_tag = s;
                }
            }
            add_component(&mut out, best_tag, "tag_fuzzy");

            let summary_fuzzy = self.score_field(&pattern, r.summary(), 5, false);
            add_component(&mut out, summary_fuzzy, "summary_fuzzy");

            let mut best_schema = 0;
            for tag in r.tags() {
                if let Some(stripped) = tag.strip_prefix("schema:") {
                    let s = self.score_field(&pattern, stripped, 4, false);
                    if s > best_schema {
                        best_schema = s;
                    }
                }
            }
            for token in r.search_tokens() {
                if token.starts_with("schema:") || token.starts_with("required:") {
                    let s = self.score_field(&pattern, search_token_text(token), 4, false);
                    if s > best_schema {
                        best_schema = s;
                    }
                }
            }
            add_component(&mut out, best_schema, "schema_fuzzy");

            let mut best_alias = 0;
            for token in r.search_tokens() {
                if let Some(alias) = token.strip_prefix("alias:") {
                    let s = self.score_field(&pattern, alias, 5, false);
                    if s > best_alias {
                        best_alias = s;
                    }
                }
            }
            add_component(&mut out, best_alias, "alias_fuzzy");

            // Issue #994: meta-tools are excluded from results unless the
            // query directly targets the tool by name. This prevents verbose
            // meta-tool descriptions from leaking into the top-N for domain
            // queries. A "direct target" means `token_match_score` on the
            // backend_tool name returned a non-zero value.
            if out.score > 0 && is_meta_tool(r.backend_tool()) {
                let tool_name_score = token_match_score(q, r.backend_tool(), 18, 12, 8);
                if tool_name_score == 0 {
                    // Query doesn't reference the meta-tool's name at all —
                    // all score came from description/tag token noise. Drop it.
                    return ScoreBreakdown::default();
                }
                // Query does reference the tool name — keep it but demoted.
                out.score /= META_TOOL_DIVISOR;
                if out.score == 0 {
                    out.score = 1;
                }
                add_reason(&mut out.match_reasons, "meta_tool_demoted");
            }

            // Issue #1325: tools tagged as escape-hatch scripting fallbacks
            // (`execute_python`, host script eval, MaxScript-style execution)
            // are kept in results but demoted so typed alternatives surface
            // first when both match the query. A tool that explicitly names
            // the escape-hatch backend (e.g. q = "execute_python") still
            // wins via the tool-name boost so debugging workflows are
            // unaffected.
            if out.score > 0
                && r.tool_role().is_some_and(|role| role == "escape_hatch")
                && !r.backend_tool().to_ascii_lowercase().contains(q)
            {
                out.score /= ESCAPE_HATCH_DIVISOR;
                if out.score == 0 {
                    out.score = 1;
                }
                add_reason(&mut out.match_reasons, "escape_hatch_demoted");
            }
        }

        if let Some(hint) = scene_hint
            && (r.summary().to_ascii_lowercase().contains(hint)
                || r.tags().iter().any(|t| t.to_ascii_lowercase() == *hint))
        {
            add_component(&mut out, 2, "scene_hint");
        }

        out
    }
}

/// Return `true` when the tool name matches a "meta-tool" pattern.
///
/// Meta-tools are gateway / infrastructure actions whose verbose
/// descriptions tend to out-rank domain-specific DCC tools in fuzzy
/// search. Issue #994: demote these so domain actions surface first.
///
/// Only targets **known gateway-level meta-tools** — not arbitrary
/// DCC actions that happen to share a prefix (e.g. `project_save` in
/// a maya-scene skill is NOT a meta-tool).
fn is_meta_tool(backend_tool: &str) -> bool {
    let lower = backend_tool.to_ascii_lowercase();
    // Specific known project meta-tools (NOT `project_save`, `project_open`, etc.)
    if lower == "project_resume" || lower == "project_checkpoint" {
        return true;
    }
    // recipes__* batch automation tools
    if lower.starts_with("recipes__") {
        return true;
    }
    // diagnostics__* diagnostic/screenshot tools
    if lower.starts_with("diagnostics__") {
        return true;
    }
    // dcc_capability_manifest infrastructure tool
    if lower == "dcc_capability_manifest" {
        return true;
    }
    false
}

/// Demotion divisor applied to meta-tool scores. A factor of 3 means
/// a meta-tool needs 3× the raw relevance of a domain tool to rank
/// above it.
const META_TOOL_DIVISOR: u32 = 3;

/// Demotion divisor applied to escape-hatch / generic-scripting tools
/// (issue #1325). A factor of 4 means a typed alternative with ¼ the
/// raw relevance still wins, so agents see the typed skill first when
/// it exists.
const ESCAPE_HATCH_DIVISOR: u32 = 4;

/// `SubstringScorer` alias from issue #765 acceptance text.
pub type ExactScorer = SubstringScorer;

/// Thread-safe single-field scorer seam (issue #765).
pub trait StrategyScorer: Send + Sync {
    fn score(&self, query: &str, candidate: &str) -> f32;
}

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

pub struct ScorerFactory;

impl ScorerFactory {
    #[must_use]
    pub fn from_mode(mode: SearchMode) -> Box<dyn StrategyScorer> {
        match mode {
            SearchMode::Fuzzy => Box::new(StrategyFuzzyScorer),
            SearchMode::Exact => Box::new(StrategyExactScorer),
        }
    }

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
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestRow {
        tool_slug: String,
        backend_tool: String,
        summary: String,
        skill_name: Option<String>,
        tags: Vec<String>,
        dcc_type: String,
        instance_id: Uuid,
        loaded: bool,
        #[serde(default)]
        tool_role: Option<String>,
    }

    impl SearchRecord for TestRow {
        fn tool_slug(&self) -> &str {
            &self.tool_slug
        }
        fn backend_tool(&self) -> &str {
            &self.backend_tool
        }
        fn summary(&self) -> &str {
            &self.summary
        }
        fn skill_name(&self) -> Option<&str> {
            self.skill_name.as_deref()
        }
        fn tags(&self) -> &[String] {
            &self.tags
        }
        fn dcc_type(&self) -> &str {
            &self.dcc_type
        }
        fn instance_id(&self) -> Uuid {
            self.instance_id
        }
        fn loaded(&self) -> bool {
            self.loaded
        }
        fn tool_role(&self) -> Option<&str> {
            self.tool_role.as_deref()
        }
    }

    fn rec(name: &str, summary: &str, skill: Option<&str>, tags: &[&str], loaded: bool) -> TestRow {
        let iid = Uuid::from_u128(0x1234_5678_0000_0000_0000_0000_0000_0001);
        TestRow {
            tool_slug: format!("maya.{:08x}.{name}", iid.as_u128() as u32),
            backend_tool: name.to_string(),
            summary: summary.to_string(),
            skill_name: skill.map(String::from),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
            dcc_type: "maya".to_string(),
            instance_id: iid,
            loaded,
            tool_role: None,
        }
    }

    fn rec_with_role(
        name: &str,
        summary: &str,
        skill: Option<&str>,
        tags: &[&str],
        loaded: bool,
        role: &str,
    ) -> TestRow {
        let mut r = rec(name, summary, skill, tags, loaded);
        r.tool_role = Some(role.to_string());
        r
    }

    #[test]
    fn substring_scorer_exact_tool_name() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"], true);
        assert_eq!(s.score(&r, "create_sphere", None), 10);
    }

    #[test]
    fn substring_scorer_substring_plus_summary() {
        let mut s = SubstringScorer;
        let r = rec("create_sphere", "make a sphere", None, &["geo"], true);
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
        let typo_score = s.score(&r, "creat_spher", None);
        assert!(
            typo_score > 0,
            "fuzzy must tolerate a typo; got {typo_score}"
        );
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
        let mut s = FuzzyScorer::new();
        let hit = s.score(&rec("create_sphere", "", None, &[], true), "cs", None);
        assert!(hit > 0, "subsequence match should score > 0");
    }

    #[test]
    fn fuzzy_scorer_rejects_totally_unrelated_query() {
        let mut s = FuzzyScorer::new();
        let hit = s.score(
            &rec("create_sphere", "geometry tool", None, &[], true),
            "xyzzy",
            None,
        );
        assert_eq!(hit, 0);
    }

    #[test]
    fn fuzzy_scorer_natural_language_multiword_hits_export_fbx() {
        let mut s = FuzzyScorer::new();
        let r = rec(
            "maya_geometry__export_fbx",
            "Export the current Maya scene or selection to FBX.",
            Some("maya-geometry"),
            &["interchange"],
            true,
        );
        let hit = s.score(&r, "create poly sphere export fbx", None);
        assert!(
            hit > 0,
            "bag-of-words relaxed path must surface export_fbx for prose queries; got {hit}",
        );
    }

    #[test]
    fn fuzzy_scorer_natural_language_multiword_hits_create_sphere() {
        let mut s = FuzzyScorer::new();
        let r = rec(
            "maya_primitives__create_sphere",
            "Create a polygon sphere.",
            Some("maya-primitives"),
            &["modeling"],
            true,
        );
        let hit = s.score(&r, "create poly sphere export fbx", None);
        assert!(
            hit > 0,
            "bag-of-words relaxed path must surface create_sphere; got {hit}",
        );
    }

    #[test]
    fn fuzzy_scorer_relaxed_multiword_prefix_token_matches_export() {
        let mut s = FuzzyScorer::new();
        let r = rec(
            "maya_geometry__export_fbx",
            "Batch write files for interchange pipelines.",
            None,
            &["interchange"],
            true,
        );
        let q = "exp scene interchg write interchange batch";
        let hit = s.score(&r, q, None);
        assert!(
            hit > 0,
            "prefix token match (exp→export) must contribute in relaxed path; got {hit}",
        );
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
    fn fuzzy_scorer_explains_weighted_match_reasons() {
        let mut s = FuzzyScorer::new();
        let row = rec(
            "maya_primitives__set_radius",
            "Set primitive radius.",
            Some("maya-primitives"),
            &["modeling", "schema:radius"],
            true,
        );
        let breakdown = s.explain(&row, "radius", None);
        assert!(breakdown.score > 0);
        assert!(
            breakdown
                .match_reasons
                .contains(&"tool_lexical".to_string())
        );
        assert!(
            breakdown
                .match_reasons
                .contains(&"schema_fuzzy".to_string())
        );
    }

    #[test]
    fn fuzzy_scorer_credits_tag_match() {
        let mut s = FuzzyScorer::new();
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

    #[test]
    fn scorer_factory_from_mode() {
        let f = ScorerFactory::from_mode(SearchMode::Fuzzy);
        assert!(f.score("sphere", "create_sphere") > 0.0);
    }

    // ── Issue #994: meta-tool demotion ──────────────────────────────────

    #[test]
    fn is_meta_tool_recognises_known_patterns() {
        assert!(super::is_meta_tool("project_resume"));
        assert!(super::is_meta_tool("project_checkpoint"));
        assert!(super::is_meta_tool("recipes__list"));
        assert!(super::is_meta_tool("diagnostics__screenshot"));
        assert!(super::is_meta_tool("dcc_capability_manifest"));

        // Domain tools must NOT be classified as meta.
        assert!(!super::is_meta_tool("maya_primitives__create_cube"));
        assert!(!super::is_meta_tool(
            "maya_light_rig__create_three_point_rig"
        ));
        assert!(!super::is_meta_tool("create_sphere"));
        // DCC actions that happen to start with "project_" are NOT meta.
        assert!(!super::is_meta_tool("project_save"));
        assert!(!super::is_meta_tool("project_open"));
    }

    #[test]
    fn fuzzy_scorer_demotes_meta_tools_below_domain_hits() {
        // Issue #994: `project_resume` must NOT out-rank a real lighting tool
        // for a query like "light rig three point".
        let mut s = FuzzyScorer::new();
        let domain_tool = rec(
            "maya_light_rig__create_three_point_rig",
            "Create a three-point lighting rig with key, fill, and rim lights.",
            Some("maya-lighting"),
            &["lighting", "rig"],
            true,
        );
        let meta_tool = rec(
            "project_resume",
            "Resume from last checkpoint: active_tool_groups, checkpoint_ids, three-point references, light rig state",
            Some("project"),
            &["meta"],
            true,
        );

        let domain_score = s.score(&domain_tool, "light rig three point", None);
        let meta_score = s.score(&meta_tool, "light rig three point", None);
        assert!(
            domain_score > meta_score,
            "domain tool ({domain_score}) must outrank meta-tool ({meta_score}) for 'light rig three point'"
        );
    }

    #[test]
    fn fuzzy_scorer_meta_tool_still_discoverable() {
        // Meta-tools should still be discoverable when queried by their exact name.
        let mut s = FuzzyScorer::new();
        let meta_tool = rec(
            "project_resume",
            "Resume a project from its last checkpoint.",
            Some("project"),
            &["meta"],
            true,
        );
        let score = s.score(&meta_tool, "project resume", None);
        assert!(
            score > 0,
            "meta-tool must still surface for exact-name query; got {score}"
        );
    }

    // ── Issue #1325: escape-hatch demotion ────────────────────────────────

    #[test]
    fn fuzzy_scorer_escape_hatch_demoted_below_typed_alternative() {
        let mut s = FuzzyScorer::new();
        let typed = rec(
            "usd_import",
            "Import a USD layer into the active scene.",
            Some("interchange"),
            &["usd", "import"],
            true,
        );
        let escape = rec_with_role(
            "execute_python",
            "Execute arbitrary Python in the host. Useful for USD import scripts.",
            Some("scripting"),
            &["scripting"],
            true,
            "escape_hatch",
        );

        let typed_score = s.score(&typed, "import usd", None);
        let escape_breakdown = s.explain(&escape, "import usd", None);

        assert!(typed_score > 0, "typed tool must still match");
        assert!(
            typed_score > escape_breakdown.score,
            "typed ({typed_score}) must outrank escape-hatch ({}) for typed-intent query",
            escape_breakdown.score
        );
        assert!(
            escape_breakdown
                .match_reasons
                .iter()
                .any(|r| r == "escape_hatch_demoted"),
            "escape_hatch demotion must surface in match_reasons; got {:?}",
            escape_breakdown.match_reasons
        );
    }

    #[test]
    fn fuzzy_scorer_escape_hatch_still_wins_for_explicit_query() {
        let mut s = FuzzyScorer::new();
        let escape = rec_with_role(
            "execute_python",
            "Execute arbitrary Python in the host.",
            Some("scripting"),
            &["scripting"],
            true,
            "escape_hatch",
        );

        let breakdown = s.explain(&escape, "execute_python", None);
        assert!(
            breakdown.score > 0,
            "explicit escape-hatch name must still surface; got {}",
            breakdown.score
        );
        assert!(
            !breakdown
                .match_reasons
                .iter()
                .any(|r| r == "escape_hatch_demoted"),
            "explicit-name query must not be demoted; got {:?}",
            breakdown.match_reasons
        );
    }
}
