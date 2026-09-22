//! Query generation and ground truth (PIP-3408).
//!
//! Every query is generated *from* a target skill, so the expected answer is
//! known without human labelling. Three classes, matching the three ways an
//! agent asks for a skill:
//!
//! * [`QueryKind::Literal`] — surface deformation of the skill name
//!   (`maya-mesh-ops` → `"maya mesh ops"`, `"MAYA-MESH-OPS"`, `"mesh ops maya"`).
//! * [`QueryKind::Paraphrase`] — the target's own descriptive vocabulary with
//!   function words dropped, words reordered, and known synonyms substituted.
//! * [`QueryKind::Intent`] — what the user wants, built only from the target's
//!   hint / tag / tool vocabulary and carrying **none** of the name tokens.
//!
//! ## Why queries are built from distinctive terms
//!
//! The synthetic corpus draws descriptions from a 30-word pool, so an
//! unfiltered description query is genuinely ambiguous: dozens of skills say
//! "create mesh". Measuring against those would measure the corpus, not the
//! ranker. [`Distinctiveness`] therefore scores every candidate term by
//! document frequency and query building prefers terms few skills share, so
//! each query has one defensible right answer.

use std::collections::HashMap;

use dcc_mcp_models::SkillMetadata;

use crate::corpus::Corpus;

/// How a query was derived from its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    /// Surface deformation of the skill name.
    Literal,
    /// The target's descriptive vocabulary, reworded.
    Paraphrase,
    /// Pure intent, carrying no token from the skill name.
    Intent,
}

impl QueryKind {
    /// Stable label used in reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Paraphrase => "paraphrase",
            Self::Intent => "intent",
        }
    }
}

/// All three classes, in report order.
pub const ALL_KINDS: [QueryKind; 3] =
    [QueryKind::Literal, QueryKind::Paraphrase, QueryKind::Intent];

/// One benchmark query with its known-correct answer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Query {
    /// Query text handed to `search_skills`.
    pub text: String,
    /// Skill the query was derived from — the expected top hit.
    pub expected: String,
    /// DCC of the expected skill, for the `dcc`-filtered split.
    pub dcc: String,
    /// How the query was derived.
    pub kind: QueryKind,
    /// Whether the target carries a hard-negative twin.
    pub has_hard_negative: bool,
}

/// Document-frequency table over the corpus, used to prefer distinctive terms.
pub struct Distinctiveness {
    /// term → number of corpus skills containing it.
    df: HashMap<String, usize>,
}

impl Distinctiveness {
    /// Build the table over `corpus`.
    #[must_use]
    pub fn build(corpus: &Corpus) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        for skill in &corpus.skills {
            for term in terms_of(skill) {
                *df.entry(term).or_insert(0) += 1;
            }
        }
        Self { df }
    }

    /// How many corpus skills carry `term`.
    #[must_use]
    pub fn df(&self, term: &str) -> usize {
        self.df.get(term).copied().unwrap_or(0)
    }

    /// Keep the `limit` most distinctive terms of `candidates`.
    ///
    /// "Distinctive" means low document frequency; ties break on term text so
    /// the choice is independent of iteration order.
    #[must_use]
    pub fn most_distinctive(&self, candidates: &[String], limit: usize) -> Vec<String> {
        let mut scored: Vec<(usize, String)> = candidates
            .iter()
            .map(|term| (self.df(term), term.clone()))
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, term)| term)
            .collect()
    }
}

/// Every searchable term of `skill`, lowercased and de-duplicated.
fn terms_of(skill: &SkillMetadata) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in tokenize(&skill.name)
        .into_iter()
        .chain(tokenize(&skill.description))
        .chain(tokenize(&skill.search_hint))
        .chain(skill.tags.iter().flat_map(|tag| tokenize(tag)))
        .chain(skill.tools.iter().flat_map(|tool| {
            tokenize(&tool.name)
                .into_iter()
                .chain(tokenize(&tool.description))
        }))
    {
        if !terms.contains(&raw) {
            terms.push(raw);
        }
    }
    terms
}

/// Split text into lowercase alphanumeric terms.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| part.len() > 1)
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

/// Terms of the skill name, used to keep intent queries name-free.
fn name_terms(skill: &SkillMetadata) -> Vec<String> {
    tokenize(&skill.name)
}

/// How many targets produced each query class.
///
/// Literal queries always exist; the descriptive classes exist only for
/// targets whose vocabulary is distinctive enough to have one right answer
/// (see [`MAX_ANSWERABLE_DF`]). Reading hit rates without this coverage is
/// misleading — a class measured on six targets is an anecdote.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Coverage {
    /// Targets the corpus generated queries for.
    pub targets: usize,
    /// Targets with at least one literal query.
    pub literal: usize,
    /// Targets with a paraphrase query.
    pub paraphrase: usize,
    /// Targets with an intent query.
    pub intent: usize,
}

/// Build every query for `corpus`: one of each class per target, plus the
/// [`Coverage`] describing which classes each target could produce.
#[must_use]
pub fn build_queries_with_coverage(corpus: &Corpus) -> (Vec<Query>, Coverage) {
    let distinct = Distinctiveness::build(corpus);
    let hard = corpus.hard_negative_targets();
    let mut queries = Vec::new();
    let mut coverage = Coverage {
        targets: corpus.targets().len(),
        ..Coverage::default()
    };

    for target_name in corpus.targets() {
        let Some(skill) = corpus.get(target_name) else {
            continue;
        };
        let has_hard_negative = hard.contains(target_name.as_str());
        let base = Query {
            text: String::new(),
            expected: target_name.clone(),
            dcc: skill.dcc.clone(),
            kind: QueryKind::Literal,
            has_hard_negative,
        };

        let literal = literal_queries(target_name, corpus.has_near_name_twin(target_name));
        if !literal.is_empty() {
            coverage.literal += 1;
        }
        for text in literal {
            queries.push(Query {
                text,
                kind: QueryKind::Literal,
                ..base.clone()
            });
        }

        if let Some(text) = paraphrase_query(skill, &distinct) {
            coverage.paraphrase += 1;
            queries.push(Query {
                text,
                kind: QueryKind::Paraphrase,
                ..base.clone()
            });
        }
        if let Some(text) = intent_query(skill, &distinct) {
            coverage.intent += 1;
            queries.push(Query {
                text,
                kind: QueryKind::Intent,
                ..base.clone()
            });
        }
    }

    (queries, coverage)
}

/// Build every query for `corpus`, discarding [`Coverage`].
#[must_use]
pub fn build_queries(corpus: &Corpus) -> Vec<Query> {
    build_queries_with_coverage(corpus).0
}

// ── Class 1: literal surface deformation ───────────────────────────────

/// Surface deformations of the skill name.
///
/// An agent types skill names from memory: separators, casing and word order
/// are all unreliable, and the tail of a long name is the first thing to go.
///
/// `skip_truncated_tail` suppresses the tail-dropped variant when the corpus
/// injected a prefix-neighbour twin: the truncated form *is* that twin's name,
/// so grading the target against it would measure a contradiction the corpus
/// created rather than a ranking decision.
fn literal_queries(target_name: &str, skip_truncated_tail: bool) -> Vec<String> {
    let segments: Vec<&str> = target_name
        .split('-')
        .filter(|part| !part.is_empty())
        .collect();
    let mut out = Vec::new();

    // Hyphens → spaces: the single most common deformation.
    if segments.len() > 1 {
        out.push(segments.join(" "));
        // Word order swap.
        let mut swapped = segments.clone();
        swapped.rotate_left(1);
        out.push(swapped.join(" "));
        // Truncated tail: the discriminating part of the name is missing.
        if !skip_truncated_tail {
            out.push(segments[..segments.len() - 1].join(" "));
        }
    }
    // Casing noise.
    out.push(target_name.to_uppercase());
    // Dropped word-internal character, i.e. the typo the fuzzy path exists for.
    if let Some(typo) = drop_one_character(target_name) {
        out.push(typo);
    }
    // Nothing but the name produced a query (single short segment): fall back
    // to the name itself so the target always has a literal query.
    if out.is_empty() {
        out.push(target_name.to_string());
    }
    out
}

/// Remove one interior character so the query is a near-miss, not a typo of a
/// separator.
///
/// Dropping a separator would merge two tokens into one that matches neither:
/// `maya-skill-00000` would become `maya-skill00000`, whose tokens are
/// `["maya", "skill00000"]`. That measures token merging rather than a
/// single-character miss, so scan forward from the two-thirds point for the
/// first alphanumeric character and drop that one instead.
fn drop_one_character(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() < 5 {
        return None;
    }
    // Two thirds in: past the DCC prefix, inside the discriminating tail.
    let start = chars.len() * 2 / 3;
    let index = (start..chars.len()).find(|index| chars[*index].is_alphanumeric())?;
    Some(chars[..index].iter().chain(&chars[index + 1..]).collect())
}

// ── Class 2: description paraphrase ────────────────────────────────────

/// Domain synonyms used to genuinely reword a description.
///
/// Only pairs that keep the query answerable are listed: both sides are words
/// the shipped skills and the synthetic pool actually use.
const SYNONYMS: [(&str, &str); 14] = [
    ("create", "build"),
    ("build", "create"),
    ("edit", "modify"),
    ("modify", "edit"),
    ("manage", "organise"),
    ("export", "write out"),
    ("import", "read in"),
    ("generate", "produce"),
    ("apply", "attach"),
    ("transform", "move"),
    ("render", "draw"),
    ("polygon", "mesh"),
    ("compute", "calculate"),
    ("simulate", "emulate"),
];

/// Words carrying no ranking signal, dropped before term selection.
const STOPWORDS: [&str; 12] = [
    "and", "the", "for", "with", "that", "this", "from", "into", "when", "your", "are", "not",
];

/// Highest document frequency a query term may have for the query to be
/// answerable at all.
///
/// The synthetic filler draws descriptions from a 30-word pool, so most of
/// its terms are shared by hundreds of skills. A query built only from those
/// has no single right answer; grading the ranker against it measures the
/// corpus, not the ranker. Descriptive queries are therefore emitted only for
/// targets that own at least one term this rare, and [`build_queries`] reports
/// how many did.
pub const MAX_ANSWERABLE_DF: usize = 5;

/// Whether `terms` contain at least `count` terms rare enough to identify one
/// skill.
fn is_answerable(distinct: &Distinctiveness, terms: &[String], count: usize) -> bool {
    terms
        .iter()
        .filter(|term| distinct.df(term) <= MAX_ANSWERABLE_DF)
        .count()
        >= count
}

/// Reword the target's description: drop stopwords, substitute synonyms, keep
/// only the distinctive terms, then reorder.
fn paraphrase_query(skill: &SkillMetadata, distinct: &Distinctiveness) -> Option<String> {
    let candidates = descriptive_terms(skill);
    if candidates.is_empty() {
        return None;
    }
    let picked = distinct.most_distinctive(&candidates, 5);
    if !is_answerable(distinct, &picked, 1) {
        return None;
    }

    let mut words: Vec<String> = picked
        .iter()
        .map(|term| synonym_of(term).to_string())
        .collect();
    // Deterministic reorder: rotate so the query is not the description's
    // own word order, which the scorer rewards as a phrase.
    words.rotate_left(1);
    Some(words.join(" "))
}

/// Description + hint + tag + tool-description vocabulary of `skill`.
fn descriptive_terms(skill: &SkillMetadata) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in tokenize(&skill.description)
        .into_iter()
        .chain(tokenize(&skill.search_hint))
        .chain(skill.tags.iter().flat_map(|tag| tokenize(tag)))
        .chain(
            skill
                .tools
                .iter()
                .flat_map(|tool| tokenize(&tool.description)),
        )
    {
        if STOPWORDS.contains(&raw.as_str()) {
            continue;
        }
        if !terms.contains(&raw) {
            terms.push(raw);
        }
    }
    terms
}

/// Synonym for `term`, or `term` itself when the map has no entry.
fn synonym_of(term: &str) -> &str {
    SYNONYMS
        .iter()
        .find(|(from, _)| *from == term)
        .map_or(term, |(_, to)| *to)
}

// ── Class 3: pure intent ───────────────────────────────────────────────

/// Build an intent query: the target's hint/tag/tool vocabulary with every
/// name token removed.
fn intent_query(skill: &SkillMetadata, distinct: &Distinctiveness) -> Option<String> {
    let banned = name_terms(skill);
    let candidates: Vec<String> = descriptive_terms(skill)
        .into_iter()
        .filter(|term| !banned.iter().any(|name| name == term))
        .collect();
    if candidates.len() < 2 {
        return None;
    }
    let picked = distinct.most_distinctive(&candidates, 4);
    if !is_answerable(distinct, &picked, 2) {
        return None;
    }
    // Phrase it as a request rather than a keyword list, and never name the
    // skill or its DCC — that is what makes it an intent query.
    Some(format!("i need to {}", picked.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Corpus, SCALE_300};

    #[test]
    fn intent_queries_never_contain_the_skill_name() {
        let corpus = Corpus::build(SCALE_300);
        for query in build_queries(&corpus) {
            if query.kind != QueryKind::Intent {
                continue;
            }
            let skill = corpus.get(&query.expected).unwrap();
            for term in name_terms(skill) {
                assert!(
                    !tokenize(&query.text).iter().any(|t| t == &term),
                    "intent query {:?} leaks name term {term:?}",
                    query.text
                );
            }
        }
    }

    #[test]
    fn coverage_reports_which_classes_each_target_could_produce() {
        let corpus = Corpus::build(SCALE_300);
        let (queries, coverage) = build_queries_with_coverage(&corpus);
        assert_eq!(coverage.targets, corpus.targets().len());
        assert_eq!(coverage.literal, coverage.targets);
        // The synthetic filler shares its vocabulary, so not every target has
        // an answerable descriptive query — that is expected and reported.
        assert!(coverage.paraphrase <= coverage.targets);
        assert!(coverage.intent <= coverage.targets);
        assert!(coverage.paraphrase > 0, "no paraphrase queries at all");
        let expected = coverage.literal + coverage.paraphrase + coverage.intent;
        // Literal targets contribute more than one variant each, so the query
        // count is at least the sum of the per-class target counts.
        assert!(queries.len() >= expected);
    }

    #[test]
    fn all_three_kinds_are_generated() {
        let corpus = Corpus::build(SCALE_300);
        let queries = build_queries(&corpus);
        for kind in ALL_KINDS {
            assert!(
                queries.iter().any(|q| q.kind == kind),
                "no queries of kind {kind:?}"
            );
        }
    }

    #[test]
    fn trailing_tail_is_suppressed_for_prefix_twins() {
        assert!(literal_queries("maya-mesh-ops", false).contains(&"maya mesh".to_string()));
        assert!(!literal_queries("maya-mesh-ops", true).contains(&"maya mesh".to_string()));
        // The other deformations survive either way.
        assert!(literal_queries("maya-mesh-ops", true).contains(&"maya mesh ops".to_string()));
    }

    #[test]
    fn literal_queries_deform_the_name() {
        let corpus = Corpus::build(SCALE_300);
        let queries = build_queries(&corpus);
        let literal: Vec<&Query> = queries
            .iter()
            .filter(|q| q.kind == QueryKind::Literal)
            .collect();
        assert!(!literal.is_empty());
        for query in literal {
            assert!(!query.text.trim().is_empty());
        }
    }

    #[test]
    fn queries_cover_the_dcc_filtered_and_unfiltered_splits() {
        let corpus = Corpus::build(SCALE_300);
        let queries = build_queries(&corpus);
        assert!(queries.iter().all(|q| !q.dcc.is_empty()));
        assert!(queries.iter().any(|q| q.has_hard_negative));
    }

    #[test]
    fn distinctiveness_prefers_rare_terms() {
        let corpus = Corpus::build(SCALE_300);
        let distinct = Distinctiveness::build(&corpus);
        let candidates = vec!["mesh".to_string(), "maya".to_string()];
        let picked = distinct.most_distinctive(&candidates, 1);
        assert_eq!(picked.len(), 1);
        // The rarer of the two wins, whichever it is.
        let winner = &picked[0];
        let loser = if winner == "mesh" { "maya" } else { "mesh" };
        assert!(distinct.df(winner) <= distinct.df(loser));
    }

    #[test]
    fn drop_one_character_keeps_the_name_recognisable() {
        let typo = drop_one_character("maya-mesh-ops").unwrap();
        assert_ne!(typo, "maya-mesh-ops");
        assert_eq!(typo.len(), "maya-mesh-ops".len() - 1);
        assert_eq!(drop_one_character("abc"), None);
    }

    #[test]
    fn drop_one_character_never_removes_a_separator() {
        // The bug this guards: deleting a separator merges two tokens into one
        // that matches neither, so the query stops being a near-miss.
        for name in [
            "maya-skill-00000",
            "unreal-skill-00000",
            "blender-skill-00000",
            "max-skill-00000",
            "houdini-skill-00000",
            "maya-skill-00000-advanced",
        ] {
            let typo = drop_one_character(name).unwrap_or_else(|| panic!("no typo for {name}"));
            assert_eq!(
                typo.len(),
                name.len() - 1,
                "{name} -> {typo} did not drop exactly one character"
            );
            let name_chars: Vec<char> = name.chars().collect();
            let typo_chars: Vec<char> = typo.chars().collect();
            // Where the two first differ is where the character went. When they
            // never differ the removed character was the last one, because zip
            // only walks the overlapping prefix.
            let removed_index = name_chars
                .iter()
                .zip(typo_chars.iter())
                .position(|(before, after)| before != after)
                .unwrap_or(name_chars.len() - 1);
            let removed = name_chars[removed_index];
            assert!(
                removed.is_alphanumeric(),
                "{name} -> {typo} removed the non-alphanumeric {removed:?}; \
                 separators must survive or the query changes tokenisation"
            );
            // The token count must be unchanged: that is what "near-miss" means.
            assert_eq!(
                tokenize(name).len(),
                tokenize(&typo).len(),
                "{name} -> {typo} changed the token count"
            );
        }
    }
}
