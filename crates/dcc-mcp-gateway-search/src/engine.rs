//! Pure search pipeline: filter → score → sort → paginate.

use std::borrow::Borrow;

use crate::fallback::{CuaRuntimeProbe, SearchFallback, build_fallback};
use crate::policy::{FallbackPolicy, FallbackTrigger, evaluate_fallback, is_fallback_target};
use crate::query::{DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchMode, SearchPage, SearchQuery};
use crate::ranking::{FuzzyScorer, Scorer, SubstringScorer};
use crate::record::SearchRecord;
use crate::{LAYER_DOMAIN, LAYER_EXAMPLE, LAYER_INFRASTRUCTURE, LAYER_THIN_HARNESS};
use crate::{RankPolicy, apply_rank_policy};

/// Rank `records` against `query` and return the first page of hits.
#[must_use]
pub fn search<R: SearchRecord + Clone>(records: &[R], query: &SearchQuery) -> Vec<SearchHit<R>> {
    search_page(records, query).hits
}

/// Paginated variant of [`search`].
///
/// The returned page never carries fallback advice; use
/// [`search_page_with_fallback`] on surfaces that can offer the `dcc-cua`
/// route (PIP-3702).
#[must_use]
pub fn search_page<R: SearchRecord + Clone>(records: &[R], query: &SearchQuery) -> SearchPage<R> {
    let hits = rank_all(records, query);
    paginate(hits, query, None)
}

/// Paginated search that attaches explicit `dcc-cua` fallback advice when the
/// result set cannot serve the request (PIP-3702).
///
/// `probe` is only consulted when the fallback criteria fire, so a search that
/// is answered normally never pays for a runtime check.
#[must_use]
pub fn search_page_with_fallback<R: SearchRecord + Clone>(
    records: &[R],
    query: &SearchQuery,
    probe: &dyn CuaRuntimeProbe,
) -> SearchPage<R> {
    let (hits, candidates) = rank_all_counted(records, query);
    // `candidates` is the post-filter count, so neither an empty index nor a
    // query-side filter that excluded every row is mistaken for "no skill can
    // do this".
    let fallback = resolve_fallback_among(&hits, &fallback_query_text(query), candidates, probe);
    paginate(hits, query, fallback)
}

/// Decide whether `hits` should be routed to the `dcc-cua` fallback.
///
/// Kept separate from pagination so callers that own their own paging contract
/// (package catalogs, the skill catalog) can reuse the same judgement.
///
/// The criteria and thresholds live in [`crate::policy`]; this function only
/// adapts a ranked page to them and, when they fire, asks `probe` whether the
/// route is actually usable.
///
/// # Empty pages
///
/// This convenience overload derives the candidate count from `hits.len()`, so
/// an empty page reads as "nothing was eligible" and never fires. It therefore
/// cannot produce [`crate::policy::FALLBACK_REASON_NO_CANDIDATE`]. Callers that
/// can see the candidate set — which is the only way to distinguish "nothing
/// matched" from "nothing was eligible" — must use
/// [`resolve_fallback_among`].
#[must_use]
pub fn resolve_fallback<R: SearchRecord, H: Borrow<SearchHit<R>>>(
    hits: &[H],
    query: &str,
    probe: &dyn CuaRuntimeProbe,
) -> Option<SearchFallback> {
    resolve_fallback_with_policy(hits, query, probe, FallbackPolicy::default())
}

/// [`resolve_fallback`] with an explicit policy.
#[must_use]
pub fn resolve_fallback_with_policy<R: SearchRecord, H: Borrow<SearchHit<R>>>(
    hits: &[H],
    query: &str,
    probe: &dyn CuaRuntimeProbe,
    policy: FallbackPolicy,
) -> Option<SearchFallback> {
    let trigger = classify(hits, query, candidates_considered(hits), policy)?;
    Some(build_fallback(trigger, probe.probe()))
}

/// How many rows were eligible to be ranked.
///
/// Derived from the hits themselves because `resolve_fallback` only sees a
/// ranked page. A non-empty page proves at least that many candidates were
/// considered; an empty page means "nothing matched", which is only a routing
/// decision when the caller separately confirms rows existed. Callers that know
/// the true candidate count pass it explicitly via
/// [`classify_with_candidates`].
fn candidates_considered<R: SearchRecord, H: Borrow<SearchHit<R>>>(hits: &[H]) -> usize {
    hits.len()
}

/// [`resolve_fallback`] for callers that know how many rows were eligible.
///
/// Use this when the candidate set is filtered before ranking, so an
/// everything-excluded filter is not mistaken for "no skill can do this".
#[must_use]
pub fn resolve_fallback_among<R: SearchRecord, H: Borrow<SearchHit<R>>>(
    hits: &[H],
    query: &str,
    candidates_considered: usize,
    probe: &dyn CuaRuntimeProbe,
) -> Option<SearchFallback> {
    let trigger = classify(hits, query, candidates_considered, policy_default())?;
    Some(build_fallback(trigger, probe.probe()))
}

fn policy_default() -> FallbackPolicy {
    FallbackPolicy::default()
}

/// Apply the central fallback policy to a ranked page.
///
/// Generic over `Borrow<SearchHit<R>>` so a caller can pass either an owned
/// slice or a slice of references produced by post-ranking filtering, without
/// cloning rows to satisfy the signature.
///
/// Returns the trigger without probing the runtime, so the judgement can be
/// unit-tested on its own.
///
/// `candidates_considered` is the number of rows that were eligible to be
/// ranked, before scoring. Zero means nothing was ever in the running — an
/// empty catalog, or a caller-side filter that excluded everything — which is a
/// discovery problem, not evidence that no skill can do the job. Sending that
/// to the CUA route would mask a rescan or a bad `dcc=` filter behind a
/// plausible-sounding suggestion.
fn classify<R: SearchRecord, H: Borrow<SearchHit<R>>>(
    hits: &[H],
    query: &str,
    candidates_considered: usize,
    policy: FallbackPolicy,
) -> Option<FallbackTrigger> {
    if candidates_considered == 0 {
        return None;
    }
    let len = query.trim().chars().count();
    match hits.first().map(Borrow::borrow) {
        Some(top) => {
            // A request the fallback target already answered is not an
            // unanswered request. Routing it would tell the caller to use the
            // thing it just found.
            if is_fallback_target(top.record.skill_name())
                || is_fallback_target(Some(top.record.tool_slug()))
                || is_fallback_target(Some(top.record.backend_tool()))
            {
                return None;
            }
            // `executable_interface_count() == Some(0)` is the only proof of a
            // missing interface; `None` (unmodelled) counts as having one.
            let has_interface = top.record.executable_interface_count() != Some(0);
            evaluate_fallback(len, Some(top.score), has_interface, policy)
        }
        // No hits at all is the strongest signal, but it is still the policy's
        // call: a discovery request must stay silent.
        None => evaluate_fallback(len, None, true, policy),
    }
}

/// Query text the fallback judgement should measure.
///
/// [`SearchQuery::or_queries`] makes a search answerable even when
/// [`SearchQuery::query`] is empty, so the OR clauses count as query text.
/// When both are empty the request is discovery, which never falls back.
fn fallback_query_text(query: &SearchQuery) -> String {
    let trimmed = query.query.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    query
        .or_queries
        .iter()
        .map(|clause| clause.trim())
        .filter(|clause| !clause.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn paginate<R: SearchRecord + Clone>(
    hits: Vec<SearchHit<R>>,
    query: &SearchQuery,
    fallback: Option<SearchFallback>,
) -> SearchPage<R> {
    let total = hits.len() as u32;
    let effective_limit = effective_limit(query.limit);
    let offset = query.offset.unwrap_or(0).min(total);
    let end = offset.saturating_add(effective_limit).min(total);
    let page = if offset < total {
        hits[offset as usize..end as usize].to_vec()
    } else {
        Vec::new()
    };

    SearchPage {
        hits: page,
        total,
        offset,
        limit: effective_limit,
        fallback,
    }
}

/// Rank every matching record, also reporting how many rows survived filtering.
///
/// Scoring and ordering are identical to [`rank_all`]. The extra value is the
/// number of rows eligible to be scored, after the query's own `dcc_type` /
/// `dcc_types` / `instance_id` / `loaded_only` / `tags` / `exclude_tags`
/// filters but before scoring.
///
/// The fallback route needs it: a filter that excluded every row is not
/// evidence that nothing can serve the request, and this count is the only way
/// to tell the two apart. See [`resolve_fallback_among`].
#[must_use]
pub fn rank_all_counted<R: SearchRecord + Clone>(
    records: &[R],
    query: &SearchQuery,
) -> (Vec<SearchHit<R>>, usize) {
    (rank_all(records, query), count_candidates(records, query))
}

/// Number of rows eligible to be scored, after the query's scope filters.
///
/// Covers the filters that make a row *ineligible*: `dcc_type` / `dcc_types` /
/// `instance_id` / `loaded_only` / `tags` / `tags_any` / `exclude_tags`.
///
/// `min_score` is deliberately excluded: it is applied after scoring and only
/// decides whether a hit is good enough to return, so it does not shrink the
/// eligible set. A bar nothing clears means "nothing here was good enough",
/// which is still worth routing on.
///
/// Mirrors the candidate filter in [`rank_all`]. Kept as a second pass so the
/// ranking path keeps its current shape; the extra linear scan is paid only by
/// callers that ask for the count.
fn count_candidates<R: SearchRecord>(records: &[R], query: &SearchQuery) -> usize {
    let dcc_filter = query
        .dcc_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let dcc_types: Vec<String> = query
        .dcc_types
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let tags_filter: Vec<String> = query
        .tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let tags_any: Vec<String> = query
        .tags_any
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let exclude_tags: Vec<String> = query
        .exclude_tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    records
        .iter()
        .filter(|r| {
            dcc_filter.is_none() && dcc_types.is_empty()
                || dcc_filter
                    .as_deref()
                    .is_some_and(|f| r.dcc_type().eq_ignore_ascii_case(f))
                || dcc_types
                    .iter()
                    .any(|d| r.dcc_type().eq_ignore_ascii_case(d))
        })
        .filter(|r| query.instance_id.is_none_or(|iid| r.instance_id() == iid))
        .filter(|r| query.loaded_only != Some(true) || r.loaded())
        .filter(|r| {
            tags_filter
                .iter()
                .all(|t| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .filter(|r| {
            tags_any.is_empty()
                || tags_any
                    .iter()
                    .any(|t| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .filter(|r| {
            !exclude_tags
                .iter()
                .any(|ex| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *ex))
        })
        .count()
}

/// Rank every matching record without applying pagination limits or offsets.
///
/// Filtering, scoring, and ordering are identical to [`search_page`]. This is
/// useful for callers that own their own pagination contract, such as package
/// catalogs that must not inherit the gateway's [`MAX_LIMIT`] page cap.
#[must_use]
pub fn rank_all<R: SearchRecord + Clone>(records: &[R], query: &SearchQuery) -> Vec<SearchHit<R>> {
    let qnorm = query.query.trim().to_ascii_lowercase();
    let dcc_filter = query
        .dcc_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let dcc_types: Vec<String> = query
        .dcc_types
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let instance_filter = query.instance_id;
    let loaded_filter = query.loaded_only;
    let tags_filter: Vec<String> = query
        .tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let tags_any: Vec<String> = query
        .tags_any
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let exclude_tags: Vec<String> = query
        .exclude_tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let scene = query.scene_hint.as_deref().map(|s| s.to_ascii_lowercase());

    let mut clauses: Vec<String> = Vec::new();
    if !qnorm.is_empty() {
        clauses.push(qnorm.clone());
    }
    for o in &query.or_queries {
        let t = o.trim().to_ascii_lowercase();
        if !t.is_empty() && !clauses.contains(&t) {
            clauses.push(t);
        }
    }
    let has_clauses = !clauses.is_empty();
    let explicit_layer = query.tags.iter().any(|tag| {
        matches!(
            tag.trim().to_ascii_lowercase().as_str(),
            LAYER_DOMAIN | LAYER_THIN_HARNESS | LAYER_INFRASTRUCTURE | LAYER_EXAMPLE
        )
    });

    let candidates: Vec<&R> = records
        .iter()
        .filter(|r| {
            dcc_filter.is_none() && dcc_types.is_empty()
                || dcc_filter
                    .as_deref()
                    .is_some_and(|f| r.dcc_type().eq_ignore_ascii_case(f))
                || dcc_types
                    .iter()
                    .any(|d| r.dcc_type().eq_ignore_ascii_case(d))
        })
        .filter(|r| instance_filter.is_none_or(|iid| r.instance_id() == iid))
        .filter(|r| loaded_filter != Some(true) || r.loaded())
        .filter(|r| {
            tags_filter
                .iter()
                .all(|t| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .filter(|r| {
            tags_any.is_empty()
                || tags_any
                    .iter()
                    .any(|t| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .filter(|r| {
            !exclude_tags
                .iter()
                .any(|ex| r.tags().iter().any(|rt| rt.to_ascii_lowercase() == *ex))
        })
        .collect();

    let mut hits: Vec<SearchHit<R>> = match query.mode {
        SearchMode::Fuzzy | SearchMode::Hybrid => {
            let mut scorer = FuzzyScorer::new();
            rank_multi(
                &candidates,
                &mut scorer,
                &clauses,
                has_clauses,
                scene.as_deref(),
                explicit_layer,
            )
        }
        SearchMode::Exact => {
            let mut scorer = SubstringScorer;
            rank_multi(
                &candidates,
                &mut scorer,
                &clauses,
                has_clauses,
                scene.as_deref(),
                explicit_layer,
            )
        }
    };

    for hit in &mut hits {
        apply_skill_hint_boost(hit, query.skill_hint.as_deref());
    }

    if let Some(min) = query.min_score
        && has_clauses
    {
        hits.retain(|h| h.score >= min);
    }

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.record.rank_scope().cmp(&a.record.rank_scope()))
            .then_with(|| a.record.tool_slug().cmp(b.record.tool_slug()))
    });

    for (idx, hit) in hits.iter_mut().enumerate() {
        hit.rank = idx as u32 + 1;
    }

    hits
}

fn apply_skill_hint_boost<R: SearchRecord>(hit: &mut SearchHit<R>, hint: Option<&str>) {
    let Some(h) = hint.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let h = h.to_ascii_lowercase();
    if h.len() < 2 {
        return;
    }
    if hit
        .record
        .skill_name()
        .is_some_and(|s| s.to_ascii_lowercase().contains(h.as_str()))
    {
        hit.score = hit.score.saturating_add(8);
        if !hit
            .match_reasons
            .iter()
            .any(|reason| reason == "skill_hint")
        {
            hit.match_reasons.push("skill_hint".to_string());
        }
    }
}

fn rank_multi<R: SearchRecord + Clone, S: Scorer>(
    candidates: &[&R],
    scorer: &mut S,
    clauses: &[String],
    has_clauses: bool,
    scene: Option<&str>,
    explicit_layer: bool,
) -> Vec<SearchHit<R>> {
    candidates
        .iter()
        .filter_map(|r| {
            let breakdown = if has_clauses {
                clauses
                    .iter()
                    .map(|c| scorer.explain(*r as &dyn SearchRecord, c, scene))
                    .max_by(|a, b| a.score.cmp(&b.score))
                    .unwrap_or_default()
            } else {
                Default::default()
            };
            let exact_name = clauses.iter().any(|clause| {
                r.backend_tool().eq_ignore_ascii_case(clause)
                    || r.skill_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case(clause))
            });
            let score = apply_rank_policy(
                breakdown.score,
                r.rank_layer(),
                r.rank_path_source(),
                RankPolicy {
                    exact_name,
                    explicit_layer,
                },
            );
            score.map(|score| SearchHit {
                record: (*r).clone(),
                rank: 0,
                score: if exact_name { u32::MAX } else { score },
                match_reasons: breakdown.match_reasons,
            })
        })
        .filter(|hit| !has_clauses || hit.score > 0)
        .collect()
}

fn effective_limit(limit: Option<u32>) -> u32 {
    match limit {
        None => DEFAULT_LIMIT,
        Some(0) => DEFAULT_LIMIT,
        Some(n) => n.min(MAX_LIMIT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct Row {
        tool_slug: String,
        backend_tool: String,
        summary: String,
        skill_name: Option<String>,
        tags: Vec<String>,
        dcc_type: String,
        instance_id: Uuid,
        loaded: bool,
    }

    impl SearchRecord for Row {
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
    }

    fn mk(slug: &str, name: &str, summary: &str, tags: &[&str], loaded: bool) -> Row {
        let iid = Uuid::from_u128(1);
        Row {
            tool_slug: slug.to_string(),
            backend_tool: name.to_string(),
            summary: summary.to_string(),
            skill_name: None,
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
            dcc_type: "maya".to_string(),
            instance_id: iid,
            loaded,
        }
    }

    fn mk_skill(
        slug: &str,
        name: &str,
        summary: &str,
        skill: &str,
        tags: &[&str],
        loaded: bool,
    ) -> Row {
        let mut r = mk(slug, name, summary, tags, loaded);
        r.skill_name = Some(skill.to_string());
        r
    }

    #[test]
    fn or_queries_union_without_primary_query() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export the current Maya scene to FBX.",
                &["interchange"],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: String::new(),
                or_queries: vec!["create sphere".into(), "export fbx".into()],
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert!(
            hits.len() >= 2,
            "expected OR to surface both tools; got {hits:?}"
        );
        let tools: Vec<&str> = hits.iter().map(|h| h.record.backend_tool()).collect();
        assert!(tools.contains(&"maya_primitives__create_sphere"));
        assert!(tools.contains(&"maya_geometry__export_fbx"));
    }

    #[test]
    fn exclude_tags_filters_rows() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export to FBX.",
                &["interchange"],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: "sphere".into(),
                exclude_tags: vec!["modeling".into()],
                ..Default::default()
            },
        );
        assert!(
            hits.iter()
                .all(|h| h.record.backend_tool() != "maya_primitives__create_sphere"),
            "modeling-tagged row should be excluded: {hits:?}"
        );
    }

    #[test]
    fn instance_id_filter_limits_rows_before_scoring() {
        let target = Uuid::from_u128(2);
        let mut other = mk(
            "m.1.sphere",
            "maya_primitives__create_sphere",
            "Create a sphere in another instance.",
            &[],
            true,
        );
        other.instance_id = Uuid::from_u128(1);
        let mut selected = mk(
            "m.2.session",
            "maya_scene__get_session_info",
            "Read scene session info.",
            &[],
            true,
        );
        selected.instance_id = target;

        let hits = search(
            &[other, selected],
            &SearchQuery {
                query: "scene".into(),
                instance_id: Some(target),
                ..Default::default()
            },
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.instance_id, target);
        assert_eq!(hits[0].record.backend_tool, "maya_scene__get_session_info");
    }

    #[test]
    fn skill_hint_boost_prefers_matching_skill() {
        let records = vec![
            mk(
                "m.1.geo",
                "maya_geometry__export_fbx",
                "Export the scene to disk.",
                &[],
                true,
            ),
            mk_skill(
                "m.1.sel",
                "maya_scene__export_selection",
                "Export the current selection.",
                "maya-geometry",
                &[],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: "export".into(),
                skill_hint: Some("maya-geometry".into()),
                ..Default::default()
            },
        );
        assert!(!hits.is_empty());
        assert_eq!(
            hits[0].record.backend_tool(),
            "maya_scene__export_selection"
        );
        assert!(hits[0].match_reasons.contains(&"skill_hint".to_string()));
    }

    #[test]
    fn fuzzy_hits_carry_match_reasons() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export the current Maya scene to FBX.",
                &["interchange"],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: "create sphere".into(),
                ..Default::default()
            },
        );
        assert!(!hits.is_empty());
        assert!(
            hits[0]
                .match_reasons
                .iter()
                .any(|reason| reason == "tool_lexical" || reason == "summary_lexical"),
            "expected bounded explanation reasons on top hit: {:?}",
            hits[0].match_reasons
        );
    }

    #[test]
    fn min_score_drops_weak_hits_when_clauses_present() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export the scene to FBX interchange.",
                &["interchange"],
                true,
            ),
        ];

        let loose = search(
            &records,
            &SearchQuery {
                query: "sphere export".into(),
                ..Default::default()
            },
        );
        assert!(loose.len() >= 2);

        let tight = search(
            &records,
            &SearchQuery {
                query: "sphere export".into(),
                min_score: Some(500),
                ..Default::default()
            },
        );
        assert!(
            tight.is_empty(),
            "unrealistic min_score should clear hits: {tight:?}"
        );
    }

    #[test]
    fn search_mode_and_pagination_echo() {
        let q = SearchQuery::default();
        assert_eq!(q.mode, SearchMode::Fuzzy);

        let page = SearchPage::<Row> {
            hits: vec![],
            total: 300,
            offset: 25,
            limit: 25,
            fallback: None,
        };
        let s = serde_json::to_string(&page).unwrap();
        let back: SearchPage<Row> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.total, 300);
    }

    #[test]
    fn rank_all_is_not_capped_by_page_limit() {
        let records: Vec<Row> = (0..125)
            .map(|index| {
                mk(
                    &format!("m.1.tool-{index:03}"),
                    &format!("maya_tools__tool_{index:03}"),
                    "A matching catalog tool.",
                    &[],
                    true,
                )
            })
            .collect();
        let query = SearchQuery {
            query: "tool".into(),
            limit: Some(125),
            ..Default::default()
        };

        let ranked = rank_all(&records, &query);
        let page = search_page(&records, &query);

        assert_eq!(ranked.len(), 125);
        assert_eq!(ranked.last().map(|hit| hit.rank), Some(125));
        assert_eq!(page.total, 125);
        assert_eq!(page.limit, MAX_LIMIT);
        assert_eq!(page.hits.len(), MAX_LIMIT as usize);
    }

    #[test]
    fn hybrid_mode_acts_like_fuzzy() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export the current Maya scene to FBX.",
                &["interchange"],
                true,
            ),
        ];

        let fuzzy_hits = search(
            &records,
            &SearchQuery {
                query: "sphere".into(),
                mode: SearchMode::Fuzzy,
                ..Default::default()
            },
        );
        let hybrid_hits = search(
            &records,
            &SearchQuery {
                query: "sphere".into(),
                mode: SearchMode::Hybrid,
                ..Default::default()
            },
        );

        // Hybrid should produce same results as Fuzzy in Phase 1
        assert_eq!(fuzzy_hits.len(), hybrid_hits.len());
        assert_eq!(fuzzy_hits[0].score, hybrid_hits[0].score);
        assert_eq!(
            fuzzy_hits[0].record.backend_tool(),
            hybrid_hits[0].record.backend_tool()
        );
    }

    #[test]
    fn fuzzy_mode_natural_language_prose_query() {
        let records = vec![
            mk(
                "m.1.sphere",
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "maya_geometry__export_fbx",
                "Export the current Maya scene or selection to FBX.",
                &["interchange"],
                true,
            ),
            mk(
                "m.1.find",
                "maya_scene__find_by_pattern",
                "Find objects by name pattern",
                &[],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: "create poly sphere export fbx".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert!(hits.len() >= 2, "expected sphere + fbx; got {hits:?}");
        let tools: Vec<&str> = hits.iter().map(|h| h.record.backend_tool()).collect();
        assert!(tools.contains(&"maya_primitives__create_sphere"));
        assert!(tools.contains(&"maya_geometry__export_fbx"));
    }

    #[test]
    fn dcc_types_or_filter_excludes_non_matching() {
        let mut maya1 = mk(
            "m.1.sphere",
            "maya_primitives__create_sphere",
            "Create sphere.",
            &[],
            true,
        );
        maya1.dcc_type = "maya".to_string();
        let mut blender1 = mk(
            "b.1.cube",
            "blender_mesh__create_cube",
            "Create cube.",
            &[],
            true,
        );
        blender1.dcc_type = "blender".to_string();
        let mut houdini1 = mk("h.1.grid", "houdini_sop__grid", "Create grid.", &[], true);
        houdini1.dcc_type = "houdini".to_string();

        let hits = search(
            &[maya1, blender1, houdini1],
            &SearchQuery {
                query: "create".into(),
                dcc_types: vec!["maya".into(), "blender".into()],
                ..Default::default()
            },
        );
        let tools: Vec<&str> = hits.iter().map(|h| h.record.backend_tool()).collect();
        assert!(tools.contains(&"maya_primitives__create_sphere"));
        assert!(tools.contains(&"blender_mesh__create_cube"));
        assert!(!tools.contains(&"houdini_sop__grid"));
    }

    #[test]
    fn single_dcc_type_filter_is_trimmed_and_case_insensitive() {
        let records = vec![mk(
            "m.1.sphere",
            "create_sphere",
            "Create a sphere.",
            &[],
            true,
        )];

        let hits = search(
            &records,
            &SearchQuery {
                query: "sphere".into(),
                dcc_type: Some("  MAYA  ".into()),
                ..Default::default()
            },
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool(), "create_sphere");
    }

    #[test]
    fn dcc_types_combined_with_dcc_type_or() {
        let mut maya1 = mk("m.1.sphere", "create_sphere", "Create.", &[], true);
        maya1.dcc_type = "maya".to_string();
        let mut blender1 = mk("b.1.cube", "create_cube", "Create.", &[], true);
        blender1.dcc_type = "blender".to_string();

        let hits = search(
            &[maya1, blender1],
            &SearchQuery {
                query: "create".into(),
                dcc_type: Some("maya".into()),
                dcc_types: vec!["blender".into()],
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn tags_any_or_filter_matches_any_tag() {
        let records = vec![
            mk(
                "m.1.sphere",
                "create_sphere",
                "Create.",
                &["modeling"],
                true,
            ),
            mk(
                "m.1.fbx",
                "export_fbx",
                "Export to FBX.",
                &["interchange"],
                true,
            ),
            mk(
                "m.1.anim",
                "animate_curve",
                "Animate curve.",
                &["animation"],
                true,
            ),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                tags_any: vec!["modeling".into(), "interchange".into()],
                ..Default::default()
            },
        );
        let tools: Vec<&str> = hits.iter().map(|h| h.record.backend_tool()).collect();
        assert!(tools.contains(&"create_sphere"));
        assert!(tools.contains(&"export_fbx"));
        assert!(!tools.contains(&"animate_curve"));
    }

    #[test]
    fn tags_and_tags_any_combined() {
        let records = vec![
            mk(
                "m.1.sphere",
                "create_sphere",
                "Create.",
                &["modeling", "primitives"],
                true,
            ),
            mk(
                "m.1.fbx",
                "export_fbx",
                "Export.",
                &["interchange", "primitives"],
                true,
            ),
            mk(
                "m.1.anim",
                "animate_curve",
                "Animate.",
                &["modeling", "animation"],
                true,
            ),
        ];

        // tags AND = requires "modeling" tag → rows 1 and 3 pass
        // tags_any OR = any of the OR tags → "primitives" or "animation"
        // Row 1: has modelings+primitives → passes AND + OR ✓
        // Row 2: has interchange+primitives → no "modeling", fails AND ✗
        // Row 3: has modeling+animation → passes AND + OR ✓
        let hits = search(
            &records,
            &SearchQuery {
                query: String::new(),
                tags: vec!["modeling".into()],
                tags_any: vec!["primitives".into(), "animation".into()],
                ..Default::default()
            },
        );
        let tools: Vec<&str> = hits.iter().map(|h| h.record.backend_tool()).collect();
        assert!(tools.contains(&"create_sphere"));
        assert!(tools.contains(&"animate_curve"));
        assert!(!tools.contains(&"export_fbx"));
    }

    #[test]
    fn empty_dcc_types_and_tags_any_no_filter() {
        let records = vec![
            mk("m.1.sphere", "create_sphere", "Create.", &[], true),
            mk("m.1.fbx", "export_fbx", "Export.", &[], true),
        ];

        let hits = search(
            &records,
            &SearchQuery {
                query: "create".into(),
                dcc_types: vec![],
                tags_any: vec![],
                ..Default::default()
            },
        );
        assert!(!hits.is_empty());
    }

    #[test]
    fn dcc_types_or_without_dcc_type_still_filters() {
        let mut maya1 = mk("m.1.sphere", "create_sphere", "Create.", &[], true);
        maya1.dcc_type = "maya".to_string();
        let mut blender1 = mk("b.1.cube", "create_cube", "Create.", &[], true);
        blender1.dcc_type = "blender".to_string();

        let hits = search(
            &[maya1.clone(), blender1],
            &SearchQuery {
                query: "create".into(),
                dcc_types: vec!["maya".into()],
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool(), "create_sphere");
    }
}
