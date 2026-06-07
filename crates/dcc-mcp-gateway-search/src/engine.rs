//! Pure search pipeline: filter → score → sort → paginate.

use crate::query::{DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchMode, SearchPage, SearchQuery};
use crate::ranking::{FuzzyScorer, Scorer, SubstringScorer};
use crate::record::SearchRecord;

/// Rank `records` against `query` and return the first page of hits.
#[must_use]
pub fn search<R: SearchRecord + Clone>(records: &[R], query: &SearchQuery) -> Vec<SearchHit<R>> {
    search_page(records, query).hits
}

/// Paginated variant of [`search`].
#[must_use]
pub fn search_page<R: SearchRecord + Clone>(records: &[R], query: &SearchQuery) -> SearchPage<R> {
    let qnorm = query.query.trim().to_ascii_lowercase();
    let dcc_filter = query.dcc_type.as_deref();
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

    let candidates: Vec<&R> = records
        .iter()
        .filter(|r| {
            dcc_filter.is_none() && dcc_types.is_empty()
                || dcc_filter.is_some_and(|f| r.dcc_type() == f)
                || dcc_types.iter().any(|d| r.dcc_type() == d)
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
        SearchMode::Fuzzy => {
            let mut scorer = FuzzyScorer::new();
            rank_multi(
                &candidates,
                &mut scorer,
                &clauses,
                has_clauses,
                scene.as_deref(),
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
            .then_with(|| a.record.tool_slug().cmp(b.record.tool_slug()))
    });

    let total = hits.len() as u32;
    let effective_limit = effective_limit(query.limit);
    let offset = query.offset.unwrap_or(0).min(total);
    let end = offset.saturating_add(effective_limit).min(total);
    let mut page = if offset < total {
        hits[offset as usize..end as usize].to_vec()
    } else {
        Vec::new()
    };
    for (idx, hit) in page.iter_mut().enumerate() {
        hit.rank = offset + idx as u32 + 1;
    }

    SearchPage {
        hits: page,
        total,
        offset,
        limit: effective_limit,
    }
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
) -> Vec<SearchHit<R>> {
    candidates
        .iter()
        .map(|r| {
            let breakdown = if has_clauses {
                clauses
                    .iter()
                    .map(|c| scorer.explain(*r as &dyn SearchRecord, c, scene))
                    .max_by(|a, b| a.score.cmp(&b.score))
                    .unwrap_or_default()
            } else {
                Default::default()
            };
            SearchHit {
                record: (*r).clone(),
                rank: 0,
                score: breakdown.score,
                match_reasons: breakdown.match_reasons,
            }
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
        };
        let s = serde_json::to_string(&page).unwrap();
        let back: SearchPage<Row> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.total, 300);
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
