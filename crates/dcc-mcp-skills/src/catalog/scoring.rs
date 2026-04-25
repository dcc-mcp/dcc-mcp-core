//! BM25-lite relevance scoring for `search_skills`.
//!
//! Replaces the previous naive substring matcher with a deterministic,
//! tokenised BM25-style ranker that weights different skill fields.
//! See issue #343 for the rationale.
//!
//! # Fields and weights
//!
//! | Field                                    | Weight |
//! |------------------------------------------|--------|
//! | `name`                                   | 5.0    |
//! | `tags`                                   | 3.0    |
//! | `search_hint`                            | 3.0    |
//! | `description`                            | 2.0    |
//! | sibling tool names (from `tools.yaml`)   | 2.0    |
//! | sibling tool descriptions                | 1.0    |
//! | `dcc` (exact token match only)           | 4.0    |
//!
//! Tokenisation: lowercase, split on `[\s_\-.,;:/]+`, drop a small stopword
//! list. No stemming, no fuzzy match.
//!
//! Scoring: BM25 with `k1=1.2`, `b=0.75`, per-token contribution summed
//! across fields and query tokens.
//!
//! Ties broken by (1) exact-name-substring presence, (2) scope precedence
//! (Admin > System > User > Repo), (3) alphabetical name.
//!
//! Exact-match fast-path: if the query equals the skill name
//! (case-insensitive, after trimming) that skill sorts first unconditionally.

use dcc_mcp_models::{SkillMetadata, SkillScope};

/// BM25 parameter: term-frequency saturation curve.
const K1: f64 = 1.2;

/// BM25 parameter: length normalisation.
const B: f64 = 0.75;

/// Field weights. See module docs.
pub const W_NAME: f64 = 5.0;
pub const W_TAGS: f64 = 3.0;
pub const W_HINT: f64 = 3.0;
pub const W_DESCRIPTION: f64 = 2.0;
pub const W_TOOL_NAME: f64 = 2.0;
pub const W_TOOL_DESCRIPTION: f64 = 1.0;
pub const W_DCC: f64 = 4.0;

/// A tiny English stopword list. Intentionally small — this library is used
/// with short, technical queries ("polygon bevel", "render maya"); we don't
/// want to filter more than the most generic connectives.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "and", "or", "to", "for", "with", "from",
];

/// Tokenise a string: lowercase, split on `[\s_\-.,;:/]+`, drop stopwords.
///
/// Deterministic — same input always yields the same token order.
pub fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '.' | ',' | ';' | ':' | '/'))
        .filter(|t| !t.is_empty())
        .filter(|t| !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// Field token buckets for a single skill, used during scoring.
#[derive(Debug, Clone, Default)]
pub struct FieldTokens {
    pub name: Vec<String>,
    pub tags: Vec<String>,
    pub hint: Vec<String>,
    pub description: Vec<String>,
    pub tool_names: Vec<String>,
    pub tool_descriptions: Vec<String>,
    pub dcc: Vec<String>,
}

impl FieldTokens {
    /// Total token count (used as BM25 document length).
    pub fn doc_len(&self) -> usize {
        self.name.len()
            + self.tags.len()
            + self.hint.len()
            + self.description.len()
            + self.tool_names.len()
            + self.tool_descriptions.len()
            + self.dcc.len()
    }

    /// Build from a `SkillMetadata`. Includes sibling `tools.yaml` content
    /// (available via `metadata.tools`) — see #356.
    pub fn from_metadata(meta: &SkillMetadata) -> Self {
        let hint_source = if meta.search_hint.is_empty() {
            meta.description.as_str()
        } else {
            meta.search_hint.as_str()
        };

        let mut tool_names = Vec::new();
        let mut tool_descriptions = Vec::new();
        for t in &meta.tools {
            tool_names.extend(tokenize(&t.name));
            tool_descriptions.extend(tokenize(&t.description));
        }

        let mut tag_tokens = Vec::new();
        for tag in &meta.tags {
            tag_tokens.extend(tokenize(tag));
        }

        Self {
            name: tokenize(&meta.name),
            tags: tag_tokens,
            hint: tokenize(hint_source),
            description: tokenize(&meta.description),
            tool_names,
            tool_descriptions,
            dcc: tokenize(&meta.dcc),
        }
    }
}

fn count_occurrences(field: &[String], token: &str) -> usize {
    field.iter().filter(|t| t.as_str() == token).count()
}

/// Inverse document frequency (BM25 form, clamped at 0).
fn idf(total_docs: usize, df: usize) -> f64 {
    let n = total_docs as f64;
    let df_f = df as f64;
    let v = ((n - df_f + 0.5) / (df_f + 0.5) + 1.0).ln();
    if v < 0.0 { 0.0 } else { v }
}

fn field_bm25(f: usize, dl: usize, avgdl: f64) -> f64 {
    if f == 0 {
        return 0.0;
    }
    let f_f = f as f64;
    let dl_f = dl as f64;
    let denom = f_f + K1 * (1.0 - B + B * (dl_f / avgdl.max(1.0)));
    (f_f * (K1 + 1.0)) / denom
}

/// Scored skill reference: index into the input slice + score + tie-break
/// auxiliaries.
#[derive(Debug, Clone)]
pub struct Scored {
    pub index: usize,
    pub score: f64,
    /// True if `name` contains the (raw, lowercased) query as a substring —
    /// primary tie-break after score equality.
    pub name_substring_hit: bool,
    /// Scope (for tie-break 2).
    pub scope: SkillScope,
    /// Skill name (for alphabetical tie-break 3).
    pub name: String,
    /// True if the query equals the skill name case-insensitively — forces
    /// this result to the top regardless of BM25 output.
    pub exact_name: bool,
}

/// Score a collection of skills against a query string.
///
/// Returns a `Vec<Scored>` sorted by relevance descending, with the full
/// tie-break chain applied. Skills with score `0.0` are dropped (unless they
/// are an exact-name match, which short-circuits to the top).
///
/// The `scopes` slice must be the same length as `skills` and provide the
/// scope for each skill (catalog holds scopes on `SkillEntry`, not on
/// `SkillMetadata`).
pub fn score_skills(query: &str, skills: &[&SkillMetadata], scopes: &[SkillScope]) -> Vec<Scored> {
    assert_eq!(
        skills.len(),
        scopes.len(),
        "skills and scopes slices must be the same length"
    );

    let tokens = tokenize(query);
    let q_trim_lower = query.trim().to_lowercase();
    let q_raw_lower = query.to_lowercase();

    // Pre-compute field tokens and document lengths.
    let fields: Vec<FieldTokens> = skills
        .iter()
        .map(|m| FieldTokens::from_metadata(m))
        .collect();
    let doc_lens: Vec<usize> = fields.iter().map(|f| f.doc_len()).collect();

    let total_docs = skills.len();
    let avgdl: f64 = if total_docs == 0 {
        1.0
    } else {
        doc_lens.iter().sum::<usize>() as f64 / total_docs as f64
    };

    // For each query token, count document frequency (how many docs contain
    // the token anywhere across weighted fields). Used for IDF.
    let df_per_token: Vec<usize> = tokens
        .iter()
        .map(|q| {
            fields
                .iter()
                .filter(|f| {
                    count_occurrences(&f.name, q) > 0
                        || count_occurrences(&f.tags, q) > 0
                        || count_occurrences(&f.hint, q) > 0
                        || count_occurrences(&f.description, q) > 0
                        || count_occurrences(&f.tool_names, q) > 0
                        || count_occurrences(&f.tool_descriptions, q) > 0
                        || count_occurrences(&f.dcc, q) > 0
                })
                .count()
        })
        .collect();

    let mut scored: Vec<Scored> = Vec::with_capacity(skills.len());
    for (i, (meta, fields_i)) in skills.iter().zip(fields.iter()).enumerate() {
        let dl = doc_lens[i];
        let mut total = 0.0_f64;

        for (q_idx, q) in tokens.iter().enumerate() {
            let idf_v = idf(total_docs, df_per_token[q_idx]);
            if idf_v == 0.0 {
                continue;
            }
            let contrib = W_NAME * field_bm25(count_occurrences(&fields_i.name, q), dl, avgdl)
                + W_TAGS * field_bm25(count_occurrences(&fields_i.tags, q), dl, avgdl)
                + W_HINT * field_bm25(count_occurrences(&fields_i.hint, q), dl, avgdl)
                + W_DESCRIPTION
                    * field_bm25(count_occurrences(&fields_i.description, q), dl, avgdl)
                + W_TOOL_NAME * field_bm25(count_occurrences(&fields_i.tool_names, q), dl, avgdl)
                + W_TOOL_DESCRIPTION
                    * field_bm25(count_occurrences(&fields_i.tool_descriptions, q), dl, avgdl)
                + W_DCC * field_bm25(count_occurrences(&fields_i.dcc, q), dl, avgdl);
            total += idf_v * contrib;
        }

        let name_lower = meta.name.to_lowercase();
        let exact_name = name_lower == q_trim_lower && !q_trim_lower.is_empty();
        let name_substring_hit = !q_raw_lower.is_empty() && name_lower.contains(&q_raw_lower);

        if total == 0.0 && !exact_name {
            continue;
        }

        scored.push(Scored {
            index: i,
            score: total,
            name_substring_hit,
            scope: scopes[i],
            name: meta.name.clone(),
            exact_name,
        });
    }

    // Deterministic sort with full tie-break chain.
    scored.sort_by(|a, b| {
        // 0. Exact-name fast-path.
        match (a.exact_name, b.exact_name) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        // 1. Higher score first.
        match b
            .score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
        {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        // 2. Exact name-substring hit wins.
        match (a.name_substring_hit, b.name_substring_hit) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        // 3. Scope precedence: Admin > System > User > Repo.
        match b.scope.cmp(&a.scope) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        // 4. Alphabetical name.
        a.name.cmp(&b.name)
    });

    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_models::ToolDeclaration;

    fn mk(name: &str) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: String::new(),
            dcc: String::new(),
            version: "1.0.0".to_string(),
            ..Default::default()
        }
    }

    fn mk_full(
        name: &str,
        desc: &str,
        hint: &str,
        tags: &[&str],
        dcc: &str,
        tools: &[(&str, &str)],
    ) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: desc.to_string(),
            search_hint: hint.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            dcc: dcc.to_string(),
            version: "1.0.0".to_string(),
            tools: tools
                .iter()
                .map(|(n, d)| ToolDeclaration {
                    name: n.to_string(),
                    description: d.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    // ── tokeniser ──

    #[test]
    fn test_tokenize_basic() {
        assert_eq!(
            tokenize("Polygon Bevel"),
            vec!["polygon".to_string(), "bevel".to_string()]
        );
    }

    #[test]
    fn test_tokenize_punct_and_separators() {
        assert_eq!(
            tokenize("maya-geometry.create_sphere/tool"),
            vec![
                "maya".to_string(),
                "geometry".to_string(),
                "create".to_string(),
                "sphere".to_string(),
                "tool".to_string(),
            ]
        );
    }

    #[test]
    fn test_tokenize_stopwords_dropped() {
        assert_eq!(
            tokenize("a list of the tools for maya"),
            vec!["list".to_string(), "tools".to_string(), "maya".to_string()]
        );
    }

    #[test]
    fn test_tokenize_empty_and_stopword_only() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("the of and or").is_empty());
    }

    // ── scorer ──

    #[test]
    fn test_exact_name_fast_path() {
        let a = mk_full(
            "maya-geometry",
            "Tons of sphere bevel keywords sphere bevel sphere bevel",
            "sphere bevel sphere bevel",
            &["sphere", "bevel"],
            "maya",
            &[("sphere", "create a sphere"), ("bevel", "bevel edges")],
        );
        let b = mk("sphere");
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("sphere", &skills, &scopes);
        assert!(!out.is_empty());
        assert_eq!(out[0].name, "sphere", "exact-name match must rank first");
    }

    #[test]
    fn test_prefix_only_no_match() {
        // BM25 is token-based — "poly" should NOT match "polygon" without stemming.
        let a = mk_full("polygon-tools", "polygon modelling", "", &[], "maya", &[]);
        let skills = vec![&a];
        let scopes = vec![SkillScope::Repo];
        let out = score_skills("poly", &skills, &scopes);
        assert!(
            out.is_empty(),
            "prefix-only queries must not match without stemming"
        );
    }

    #[test]
    fn test_tag_hit() {
        let a = mk_full("alpha", "", "", &["rigging"], "maya", &[]);
        let b = mk_full("beta", "something else entirely", "", &[], "maya", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("rigging", &skills, &scopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "alpha");
    }

    #[test]
    fn test_description_hit() {
        let a = mk_full("alpha", "handles character animation", "", &[], "maya", &[]);
        let b = mk_full("beta", "handles rendering", "", &[], "maya", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("animation", &skills, &scopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "alpha");
    }

    #[test]
    fn test_sibling_tool_hit() {
        // Skill whose skill-level fields do NOT mention "turntable" but whose
        // sibling tools.yaml tool does — must still rank via sibling expansion.
        let a = mk_full(
            "scene-utils",
            "scene helpers",
            "",
            &[],
            "maya",
            &[("turntable", "create a turntable camera")],
        );
        let b = mk_full("other", "unrelated stuff", "", &[], "maya", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("turntable", &skills, &scopes);
        assert_eq!(out.len(), 1, "sibling tool name must be scorable");
        assert_eq!(out[0].name, "scene-utils");
    }

    #[test]
    fn test_stopword_only_query() {
        let a = mk_full("alpha", "stuff", "", &[], "maya", &[]);
        let skills = vec![&a];
        let scopes = vec![SkillScope::Repo];
        let out = score_skills("the of and", &skills, &scopes);
        assert!(out.is_empty(), "stopword-only queries must yield no hits");
    }

    #[test]
    fn test_empty_query() {
        let a = mk("alpha");
        let skills = vec![&a];
        let scopes = vec![SkillScope::Repo];
        let out = score_skills("", &skills, &scopes);
        assert!(out.is_empty());
    }

    #[test]
    fn test_multi_token_query_ordering() {
        // "polygon bevel" — skill with both tokens should beat skill with one.
        let both = mk_full(
            "polygon-bevel",
            "bevels polygon edges",
            "polygon bevel",
            &[],
            "maya",
            &[],
        );
        let one = mk_full(
            "polygon-only",
            "polygon modelling helpers",
            "",
            &[],
            "maya",
            &[],
        );
        let skills = vec![&one, &both];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("polygon bevel", &skills, &scopes);
        assert_eq!(out[0].name, "polygon-bevel");
    }

    #[test]
    fn test_scope_tiebreak() {
        // Two skills with identical content — higher scope wins.
        let a = mk_full("alpha", "renderer thing", "", &[], "maya", &[]);
        let b = mk_full("beta", "renderer thing", "", &[], "maya", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Admin];
        let out = score_skills("renderer", &skills, &scopes);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].name, "beta",
            "Admin scope must outrank Repo scope at equal score"
        );
    }

    #[test]
    fn test_dcc_exact_token() {
        // "maya" appears in dcc field of one skill only.
        let a = mk_full("alpha", "renderer thing", "", &[], "maya", &[]);
        let b = mk_full("beta", "renderer thing", "", &[], "blender", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("maya", &skills, &scopes);
        assert_eq!(out[0].name, "alpha", "dcc token boost must apply");
    }

    #[test]
    fn test_deterministic_ordering() {
        // Two fully-equivalent skills — fall through to alphabetical name.
        let a = mk_full("zzz", "renderer thing", "", &[], "maya", &[]);
        let b = mk_full("aaa", "renderer thing", "", &[], "maya", &[]);
        let skills = vec![&a, &b];
        let scopes = vec![SkillScope::Repo, SkillScope::Repo];
        let out = score_skills("renderer", &skills, &scopes);
        assert_eq!(out[0].name, "aaa");
        assert_eq!(out[1].name, "zzz");
    }
}
