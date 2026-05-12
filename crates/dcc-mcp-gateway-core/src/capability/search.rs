//! Wire types and pure ranking functions for capability search (`POST /v1/search`).
//!
//! This module carries both sides of the search contract:
//!
//! - the serialisable request / response shapes exchanged on REST and MCP
//!   surfaces, and
//! - the pure ranking loop that joins a [`SearchQuery`] against an immutable
//!   [`IndexSnapshot`].
//!
//! Runtime-owned mutable state remains in `dcc-mcp-gateway`: `CapabilityIndex`
//! owns locks and per-instance mutation, while this crate only sees snapshots.
//! Moving the ranking function here keeps Clean Architecture dependency
//! direction intact (gateway runtime → gateway-core domain) and guarantees that
//! every consumer ranks the same snapshot identically.
//!
//! # Wire contract
//!
//! Every public type here is `Serialize` + `Deserialize`. Field names,
//! defaults, and [`serde(rename_all)`] attributes are part of the
//! `POST /v1/search` REST contract — adjust with care and bump the contract
//! docs if you change the shape.
//!
//! # Pre-#659 compatibility
//!
//! [`SearchQuery`] fields default to values that preserve the pre-#659
//! behaviour, so existing clients that only set `query` keep working without
//! re-serialising.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::index::IndexSnapshot;
use super::record::CapabilityRecord;
use super::search_ranking::{FuzzyScorer, Scorer, SubstringScorer};

/// Which scoring strategy to use for a search.
///
/// Defaults to [`SearchMode::Fuzzy`] — the pre-#659 `Exact` strategy
/// stays addressable for callers (regression tests, deterministic
/// surfaces) that explicitly want substring-only matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Fuzzy matching with prefix/subsequence bonuses. Tolerates
    /// typos and partial tokens — the right default for agents.
    #[default]
    Fuzzy,
    /// Pre-#659 substring table. Exact or substring matches only,
    /// no typo tolerance. Mainly useful for deterministic golden
    /// tests and regression guards.
    Exact,
}

/// Parameters accepted by `search_tools` / `POST /v1/search`.
///
/// Every new field defaults to a value that preserves the pre-#659
/// behaviour, so existing clients that only set `query` keep working
/// without re-serialising.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchQuery {
    /// Free-text query matched against tool name, skill, summary,
    /// and tags. Empty string disables keyword ranking and returns
    /// the catalogue in deterministic order.
    pub query: String,
    /// Restrict results to a specific DCC bucket (`"maya"`, …).
    pub dcc_type: Option<String>,
    /// Restrict results to a single backend instance. Useful for
    /// follow-up calls where the agent has already picked the
    /// instance and wants instance-scoped autocomplete.
    pub instance_id: Option<Uuid>,
    /// Optional domain tags the caller wants to filter by — records
    /// that do not carry every listed tag are dropped.
    pub tags: Vec<String>,
    /// When `Some(true)`, drop records whose owning skill is not
    /// currently loaded (`has_schema == false`). The builder sets
    /// `has_schema` from the backend `tools/list` response, so this
    /// maps directly to "currently addressable" on the backend.
    pub loaded_only: Option<bool>,
    /// Optional scene / document hint; used as a soft boost rather
    /// than a filter because agents often pass inaccurate hints.
    pub scene_hint: Option<String>,
    /// Cap on the number of hits returned. `0` means "fall back to
    /// `DEFAULT_LIMIT`"; values > [`MAX_LIMIT`] are clamped.
    pub limit: Option<u32>,
    /// Number of hits to skip after ranking. Zero by default so
    /// existing clients see the same first page they did pre-#659.
    pub offset: Option<u32>,
    /// Which scoring strategy to apply — defaults to fuzzy.
    pub mode: SearchMode,
}

/// Default page size for `search_tools` — keeps the response token
/// cost modest even when the caller forgets to pass `limit`.
pub const DEFAULT_LIMIT: u32 = 25;
/// Upper bound on the number of results returned in a single page.
pub const MAX_LIMIT: u32 = 100;

/// One result row in the search response. Same wire shape on REST
/// and MCP surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// Capability being ranked.
    #[serde(flatten)]
    pub record: CapabilityRecord,
    /// Score used for ranking. Informational — clients should treat
    /// it as opaque and rely on the list order.
    pub score: u32,
}

/// Paginated search response envelope (issue #659).
///
/// The wrapper lets callers discover how many records matched
/// overall (for a progress bar, or to decide whether to ask for the
/// next page) without shipping every hit on every call. Kept as a
/// sibling type rather than replacing `Vec<SearchHit>` so existing
/// callers that consume the list directly keep compiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPage {
    /// Ranked, truncated hits for the requested page.
    pub hits: Vec<SearchHit>,
    /// Total number of records that matched the query after all
    /// filters were applied, before pagination truncation.
    pub total: u32,
    /// Offset the caller asked for (echoed back so clients can
    /// round-trip it into a "next page" request).
    pub offset: u32,
    /// Effective limit that was applied to produce `hits`.
    pub limit: u32,
}

/// Rank `snapshot` against `query` and return the top-N hits for the first page.
///
/// Surfaces that need pagination should use [`search_page`] instead.
#[must_use]
pub fn search(snapshot: &IndexSnapshot, query: &SearchQuery) -> Vec<SearchHit> {
    search_page(snapshot, query).hits
}

/// Paginated variant of [`search`].
///
/// Returns the ranked hits plus the total match count and echoed offset/limit
/// so callers can paginate without re-issuing the query from scratch.
#[must_use]
pub fn search_page(snapshot: &IndexSnapshot, query: &SearchQuery) -> SearchPage {
    let qnorm = query.query.trim().to_ascii_lowercase();
    let dcc_filter = query.dcc_type.as_deref();
    let instance_filter = query.instance_id;
    let loaded_filter = query.loaded_only;
    let tags_filter: Vec<String> = query
        .tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let scene = query.scene_hint.as_deref().map(|s| s.to_ascii_lowercase());

    // Phase 1: filter — these are strict drops, not score nudges.
    let candidates: Vec<&CapabilityRecord> = snapshot
        .records
        .iter()
        .filter(|r| dcc_filter.is_none_or(|f| r.dcc_type == f))
        .filter(|r| instance_filter.is_none_or(|iid| r.instance_id == iid))
        .filter(|r| loaded_filter != Some(true) || r.loaded)
        .filter(|r| {
            tags_filter
                .iter()
                .all(|t| r.tags.iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .collect();

    // Phase 2: score. Construct exactly one scorer and reuse it across records.
    let mut hits: Vec<SearchHit> = match query.mode {
        SearchMode::Fuzzy => {
            let mut scorer = FuzzyScorer::new();
            rank(&candidates, &mut scorer, &qnorm, scene.as_deref())
        }
        SearchMode::Exact => {
            let mut scorer = SubstringScorer;
            rank(&candidates, &mut scorer, &qnorm, scene.as_deref())
        }
    };

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            // Tie-breaker: alphabetical slug so results stay stable across reruns.
            .then_with(|| a.record.tool_slug.cmp(&b.record.tool_slug))
    });

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
    }
}

fn rank<S: Scorer>(
    candidates: &[&CapabilityRecord],
    scorer: &mut S,
    qnorm: &str,
    scene: Option<&str>,
) -> Vec<SearchHit> {
    candidates
        .iter()
        .map(|r| SearchHit {
            record: (*r).clone(),
            score: scorer.score(r, qnorm, scene),
        })
        // Empty queries browse the whole catalogue; non-empty queries drop
        // records with no match signal so token budget is not spent on noise.
        .filter(|hit| qnorm.is_empty() || hit.score > 0)
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
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::capability::record::tool_slug;

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
        assert!(q.loaded_only.is_none());
        assert!(q.scene_hint.is_none());
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
            ),
            score: 42,
        };
        let v: serde_json::Value = serde_json::to_value(&hit).unwrap();
        assert_eq!(v["tool_slug"], "maya.abcdef01.create_sphere");
        assert_eq!(v["score"], 42);
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
