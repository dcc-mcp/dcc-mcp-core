//! Capability search adapter (issue #845 / split crate).
//!
//! Wire types and the pure ranking loop live in [`dcc_mcp_gateway_search`].
//! This module wires [`IndexSnapshot`] + [`CapabilityRecord`] and preserves
//! the historical `dcc_mcp_gateway_core::capability::search::*` import paths.

use super::index::IndexSnapshot;
use super::record::CapabilityRecord;

pub use dcc_mcp_gateway_search::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchMode, SearchQuery,
};

/// One result row (flattened [`CapabilityRecord`] + score).
pub type SearchHit = dcc_mcp_gateway_search::SearchHit<CapabilityRecord>;

/// Paginated search response.
pub type SearchPage = dcc_mcp_gateway_search::SearchPage<CapabilityRecord>;

/// Rank `snapshot` against `query` and return the top-N hits for the first page.
#[must_use]
pub fn search(snapshot: &IndexSnapshot, query: &SearchQuery) -> Vec<SearchHit> {
    dcc_mcp_gateway_search::search(snapshot.records.as_ref(), query)
}

/// Paginated variant of [`search`].
#[must_use]
pub fn search_page(snapshot: &IndexSnapshot, query: &SearchQuery) -> SearchPage {
    dcc_mcp_gateway_search::search_page(snapshot.records.as_ref(), query)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::capability::record::tool_slug;
    use uuid::Uuid;

    fn record(
        dcc: &str,
        iid: Uuid,
        name: &str,
        summary: &str,
        tags: &[&str],
        has_schema: bool,
        loaded: bool,
    ) -> CapabilityRecord {
        CapabilityRecord::new(
            tool_slug(dcc, &iid, name),
            name.to_owned(),
            name.to_owned(),
            None,
            summary,
            tags.iter().map(|t| (*t).to_owned()).collect(),
            dcc.to_owned(),
            iid,
            has_schema,
            loaded,
            None,
        )
    }

    fn snapshot(records: Vec<CapabilityRecord>) -> IndexSnapshot {
        IndexSnapshot {
            records: Arc::from(records),
            fingerprints: HashMap::new(),
        }
    }

    #[test]
    fn search_mode_defaults_to_fuzzy() {
        assert_eq!(SearchMode::default(), SearchMode::Fuzzy);
    }

    #[test]
    fn search_mode_wire_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&SearchMode::Fuzzy).unwrap(),
            "\"fuzzy\""
        );
        assert_eq!(
            serde_json::to_string(&SearchMode::Exact).unwrap(),
            "\"exact\""
        );

        let back: SearchMode = serde_json::from_str("\"fuzzy\"").unwrap();
        assert_eq!(back, SearchMode::Fuzzy);
    }

    #[test]
    fn search_query_defaults_preserve_pre_659_shape() {
        let q = SearchQuery::default();
        assert!(q.query.is_empty());
        assert!(q.dcc_type.is_none());
        assert!(q.instance_id.is_none());
        assert!(q.tags.is_empty());
        assert!(q.exclude_tags.is_empty());
        assert!(q.loaded_only.is_none());
        assert!(q.scene_hint.is_none());
        assert!(q.min_score.is_none());
        assert!(q.skill_hint.is_none());
        assert!(q.or_queries.is_empty());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
        assert_eq!(q.mode, SearchMode::Fuzzy);
    }

    #[test]
    fn search_query_accepts_query_only_body() {
        let q: SearchQuery = serde_json::from_str(r#"{"query": "create sphere"}"#).unwrap();
        assert_eq!(q.query, "create sphere");
        assert_eq!(q.mode, SearchMode::Fuzzy);
    }

    #[test]
    fn search_query_deserializes_extended_search_fields() {
        let q: SearchQuery = serde_json::from_str(
            r#"{"query":"x","exclude_tags":["legacy"],"min_score":3,"skill_hint":"maya-geo","or_queries":["a","b"]}"#,
        )
        .unwrap();
        assert_eq!(q.query, "x");
        assert_eq!(q.exclude_tags, vec!["legacy"]);
        assert_eq!(q.min_score, Some(3));
        assert_eq!(q.skill_hint.as_deref(), Some("maya-geo"));
        assert_eq!(q.or_queries, vec!["a", "b"]);
    }

    #[test]
    fn search_hit_flattens_record_into_row() {
        let hit = SearchHit {
            record: CapabilityRecord::new(
                "maya.abcdef01.create_sphere".into(),
                "create_sphere".into(),
                "create_sphere".into(),
                None,
                "Create a polygonal sphere.",
                vec![],
                "maya".into(),
                Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap(),
                false,
                true,
                None,
            ),
            rank: 1,
            score: 42,
            match_reasons: vec!["tool_exact".to_string()],
        };
        let v: serde_json::Value = serde_json::to_value(&hit).unwrap();
        assert_eq!(v["tool_slug"], "maya.abcdef01.create_sphere");
        assert_eq!(v["rank"], 1);
        assert_eq!(v["score"], 42);
        assert_eq!(v["match_reasons"], serde_json::json!(["tool_exact"]));
        assert!(v.get("record").is_none());
    }

    #[test]
    fn search_page_carries_pagination_echo() {
        let page = SearchPage {
            hits: vec![],
            total: 300,
            offset: 25,
            limit: 25,
        };
        let s = serde_json::to_string(&page).unwrap();
        let back: SearchPage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.total, 300);
        assert_eq!(back.offset, 25);
        assert_eq!(back.limit, 25);
    }

    #[test]
    fn default_limit_and_max_limit_are_stable() {
        assert_eq!(DEFAULT_LIMIT, 25);
        assert_eq!(MAX_LIMIT, 100);
    }

    #[test]
    fn empty_query_returns_all_records_within_limit() {
        let iid = Uuid::from_u128(1);
        let snap = snapshot(vec![record(
            "maya",
            iid,
            "create_sphere",
            "make a sphere",
            &["geo"],
            true,
            true,
        )]);

        let hits = search(&snap, &SearchQuery::default());

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
    }

    #[test]
    fn search_matches_alias_and_schema_tokens_with_reasons() {
        let iid = Uuid::from_u128(11);
        let snap = snapshot(vec![
            record(
                "maya",
                iid,
                "create_sphere",
                "Create polygon mesh",
                &[],
                true,
                true,
            )
            .with_search_tokens(vec!["alias:primitive ball".into(), "schema:radius".into()]),
            record(
                "photoshop",
                iid,
                "resize_canvas",
                "Resize document",
                &[],
                true,
                true,
            )
            .with_search_tokens(vec![
                "alias:document bounds".into(),
                "required:height_pixels".into(),
            ]),
        ]);

        let alias_hits = search(
            &snap,
            &SearchQuery {
                query: "primitive ball".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert_eq!(alias_hits.len(), 1);
        assert_eq!(alias_hits[0].record.backend_tool, "create_sphere");
        assert!(
            alias_hits[0]
                .match_reasons
                .contains(&"alias_lexical".to_string())
        );

        let schema_hits = search(
            &snap,
            &SearchQuery {
                query: "height_pixels".into(),
                dcc_type: Some("photoshop".into()),
                ..Default::default()
            },
        );
        assert_eq!(schema_hits.len(), 1);
        assert_eq!(schema_hits[0].record.backend_tool, "resize_canvas");
        assert!(
            schema_hits[0]
                .match_reasons
                .contains(&"schema_lexical".to_string())
        );
    }

    #[test]
    fn exact_mode_preserves_legacy_table() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let snap = snapshot(vec![
            record("maya", a, "sphere", "", &[], true, true),
            record("maya", a, "create_sphere", "", &[], true, true),
            record("maya", b, "open", "open a sphere scene", &[], false, false),
        ]);

        let hits = search(
            &snap,
            &SearchQuery {
                query: "sphere".into(),
                mode: SearchMode::Exact,
                ..Default::default()
            },
        );

        assert_eq!(hits[0].record.backend_tool, "sphere");
        assert_eq!(hits[1].record.backend_tool, "create_sphere");
        assert_eq!(hits[2].record.backend_tool, "open");
    }

    #[test]
    fn filters_intersect_before_scoring() {
        let maya_a = Uuid::from_u128(1);
        let maya_b = Uuid::from_u128(2);
        let blender_c = Uuid::from_u128(3);
        let snap = snapshot(vec![
            record("maya", maya_a, "read_scene", "", &["read-only"], true, true),
            record(
                "maya",
                maya_b,
                "write_scene",
                "",
                &["destructive"],
                true,
                true,
            ),
            record(
                "blender",
                blender_c,
                "read_scene",
                "",
                &["read-only"],
                false,
                false,
            ),
        ]);

        let hits = search(
            &snap,
            &SearchQuery {
                query: "scene".into(),
                dcc_type: Some("maya".into()),
                tags: vec!["read-only".into()],
                loaded_only: Some(true),
                ..Default::default()
            },
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "read_scene");
        assert_eq!(hits[0].record.dcc_type, "maya");
    }

    #[test]
    fn fuzzy_mode_tolerates_single_character_typo() {
        let iid = Uuid::from_u128(1);
        let snap = snapshot(vec![record(
            "maya",
            iid,
            "create_sphere",
            "",
            &[],
            true,
            true,
        )]);

        let hits = search(
            &snap,
            &SearchQuery {
                query: "creat_spher".into(),
                ..Default::default()
            },
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
    }

    #[test]
    fn fuzzy_mode_finds_obvious_short_keywords() {
        let iid = Uuid::from_u128(1);
        let snap = snapshot(vec![
            record(
                "maya",
                iid,
                "maya_primitives__create_sphere",
                "Create a polygon sphere",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_scene__execute_python",
                "Execute Python in Maya",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_scene__group_objects",
                "Group a list of objects under a new transform node",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_anim__bake_simulation",
                "Bake animation simulation",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_io__import_scene",
                "Import a scene file",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_geometry__export_fbx",
                "Export selected geometry as FBX",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_viewport__playblast",
                "Create a viewport playblast",
                &[],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_scene__find_by_pattern",
                "Find objects by name pattern",
                &[],
                true,
                true,
            ),
        ]);

        for query in [
            "sphere",
            "create_sphere",
            "execute_python",
            "group",
            "bake",
            "import",
            "export",
            "playblast",
        ] {
            let hits = search(
                &snap,
                &SearchQuery {
                    query: query.into(),
                    ..Default::default()
                },
            );
            assert!(!hits.is_empty(), "expected at least one hit for {query:?}");
        }

        let fbx_hits = search(
            &snap,
            &SearchQuery {
                query: "fbx".into(),
                ..Default::default()
            },
        );
        assert!(
            fbx_hits
                .iter()
                .any(|hit| hit.record.backend_tool == "maya_geometry__export_fbx")
        );
        assert!(
            fbx_hits
                .iter()
                .all(|hit| hit.record.backend_tool != "maya_scene__find_by_pattern"),
            "fbx must not match unrelated find_by_pattern rows: {fbx_hits:?}"
        );
    }

    #[test]
    fn fuzzy_mode_natural_language_prose_query_surfaces_sphere_and_fbx() {
        let iid = Uuid::from_u128(1);
        let snap = snapshot(vec![
            record(
                "maya",
                iid,
                "maya_primitives__create_sphere",
                "Create a polygon sphere.",
                &["modeling"],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_geometry__export_fbx",
                "Export the current Maya scene or selection to FBX.",
                &["interchange"],
                true,
                true,
            ),
            record(
                "maya",
                iid,
                "maya_scene__find_by_pattern",
                "Find objects by name pattern",
                &[],
                true,
                true,
            ),
        ]);

        let hits = search(
            &snap,
            &SearchQuery {
                query: "create poly sphere export fbx".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert!(
            hits.len() >= 2,
            "expected sphere + fbx tools from prose query; got {hits:?}"
        );
        let tools: Vec<&str> = hits
            .iter()
            .map(|h| h.record.backend_tool.as_str())
            .collect();
        assert!(
            tools.contains(&"maya_primitives__create_sphere"),
            "missing create_sphere in {tools:?}"
        );
        assert!(
            tools.contains(&"maya_geometry__export_fbx"),
            "missing export_fbx in {tools:?}"
        );
    }

    #[test]
    fn pagination_returns_stable_slices() {
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..75)
            .map(|i| record("maya", iid, &format!("tool_{i:03}"), "", &[], false, false))
            .collect();
        let snap = snapshot(records);

        let page_1 = search_page(
            &snap,
            &SearchQuery {
                limit: Some(20),
                offset: Some(0),
                ..Default::default()
            },
        );
        let page_2 = search_page(
            &snap,
            &SearchQuery {
                limit: Some(20),
                offset: Some(20),
                ..Default::default()
            },
        );
        let beyond = search_page(
            &snap,
            &SearchQuery {
                limit: Some(20),
                offset: Some(500),
                ..Default::default()
            },
        );

        assert_eq!(page_1.total, 75);
        assert_eq!(page_2.total, 75);
        assert_eq!(page_1.hits.len(), 20);
        assert_eq!(page_2.hits.len(), 20);
        assert!(beyond.hits.is_empty());
        assert_eq!(beyond.total, 75);
    }

    #[test]
    fn limit_is_clamped_to_max() {
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..150)
            .map(|i| record("maya", iid, &format!("t{i:03}"), "", &[], false, false))
            .collect();
        let snap = snapshot(records);

        let hits = search(
            &snap,
            &SearchQuery {
                limit: Some(9_999),
                ..Default::default()
            },
        );

        assert_eq!(hits.len(), MAX_LIMIT as usize);
    }
}
