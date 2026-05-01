//! Keyword + fuzzy search over the capability index.
//!
//! Starting with [#659], the scoring path delegates to a pluggable
//! [`Scorer`] instead of a hand-rolled substring table. The default
//! strategy is [`FuzzyScorer`] (backed by `nucleo-matcher`) which
//! adds typo tolerance, prefix bonuses, and schema-field awareness;
//! callers that want the pre-#659 behaviour can opt back in by
//! selecting [`SearchMode::Exact`] on the query.
//!
//! `SearchQuery` still serialises identically on the wire — the
//! `mode`, `instance_id`, `loaded_only`, and `offset` fields default
//! to variants that preserve the prior REST/MCP shape.
//!
//! Scoring contributions (higher = better) for the default fuzzy
//! mode are documented in detail on [`FuzzyScorer`]. The zero-filter
//! contract is shared across modes: records whose score is `0`
//! against a **non-empty** query are dropped so the agent's token
//! budget is never spent on irrelevant rows. Tie-breaking stays
//! alphabetical on `tool_slug` for deterministic ordering across
//! reruns.
//!
//! [#659]: https://github.com/loonghao/dcc-mcp-core/issues/659

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
/// callers that consume the list directly keep compiling against
/// [`search`]; the paginated path uses [`search_page`] instead.
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

/// Rank `snapshot` against `query` and return the top-N hits for
/// the first page (offset = 0). This keeps the pre-#659 call site
/// compiling unchanged; surfaces that need pagination should use
/// [`search_page`] instead.
///
/// The function is pure and synchronous so it can be called directly
/// by both REST and MCP handlers without any awaiting.
pub fn search(snapshot: &IndexSnapshot, query: &SearchQuery) -> Vec<SearchHit> {
    search_page(snapshot, query).hits
}

/// Paginated variant of [`search`]. Returns the ranked hits plus
/// the total match count and echoed offset/limit so callers can
/// paginate without re-issuing the query from scratch.
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
    //
    // The filter pass runs before scoring because (a) it is O(n)
    // with cheap string compares and (b) it lets us amortise the
    // fuzzy-matcher construction cost over a smaller record set.
    let candidates: Vec<&CapabilityRecord> = snapshot
        .records
        .iter()
        .filter(|r| dcc_filter.is_none_or(|f| r.dcc_type == f))
        .filter(|r| instance_filter.is_none_or(|iid| r.instance_id == iid))
        .filter(|r| loaded_filter != Some(true) || r.has_schema)
        .filter(|r| {
            tags_filter
                .iter()
                .all(|t| r.tags.iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .collect();

    // Phase 2: score. Construct exactly one scorer and reuse it
    // across records (its internal buffers are reused too).
    //
    // The trait dispatch is dynamic but only once per record — the
    // hot inner loop stays monomorphic inside each scorer's
    // `score()` body.
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
            // Tie-breaker: alphabetical slug so results stay stable
            // across reruns. Never rely on hash-map iteration order.
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
        // When the caller typed a query, filter out records that do
        // not contribute any match signal at all — keeping them
        // would poison the token budget with irrelevant rows ranked
        // purely by slug. When the query is empty, keep every record
        // so the caller can browse the full catalogue deterministically.
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
mod unit_tests {
    use super::super::index::{CapabilityIndex, InstanceFingerprint};
    use super::super::record::{CapabilityRecord, tool_slug};
    use super::*;
    use std::time::Instant;
    use uuid::Uuid;

    fn push_one(
        idx: &CapabilityIndex,
        dcc: &str,
        iid: Uuid,
        name: &str,
        summary: &str,
        tags: &[&str],
        has_schema: bool,
    ) {
        let rec = CapabilityRecord::new(
            tool_slug(dcc, &iid, name),
            name.to_string(),
            None,
            summary,
            tags.iter().map(|t| t.to_string()).collect(),
            dcc.to_string(),
            iid,
            has_schema,
        );
        // Overwrite the per-instance slice with a single record for
        // focused tests; real builders always ship sorted arrays.
        idx.upsert_instance(iid, vec![rec], InstanceFingerprint(1));
    }

    fn fresh_index() -> (CapabilityIndex, Uuid, Uuid) {
        let idx = CapabilityIndex::new();
        let a = Uuid::from_u128(0xaaaa_aaaa_0000_0000_0000_0000_0000_0001);
        let b = Uuid::from_u128(0xbbbb_bbbb_0000_0000_0000_0000_0000_0001);
        (idx, a, b)
    }

    // ========================================================================
    // Pre-#659 behaviours: preserved byte-for-byte on the default SearchQuery
    // ========================================================================

    #[test]
    fn empty_query_returns_all_records_within_limit() {
        let (idx, a, _) = fresh_index();
        push_one(
            &idx,
            "maya",
            a,
            "create_sphere",
            "make a sphere",
            &["geo"],
            true,
        );
        let snap = idx.snapshot();
        let hits = search(&snap, &SearchQuery::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
    }

    #[test]
    fn exact_mode_preserves_legacy_table() {
        let (idx, a, b) = fresh_index();
        idx.upsert_instance(
            a,
            vec![
                CapabilityRecord::new(
                    tool_slug("maya", &a, "sphere"),
                    "sphere".into(),
                    None,
                    "",
                    vec![],
                    "maya".into(),
                    a,
                    true,
                ),
                CapabilityRecord::new(
                    tool_slug("maya", &a, "create_sphere"),
                    "create_sphere".into(),
                    None,
                    "",
                    vec![],
                    "maya".into(),
                    a,
                    true,
                ),
            ],
            InstanceFingerprint(1),
        );
        idx.upsert_instance(
            b,
            vec![CapabilityRecord::new(
                tool_slug("maya", &b, "open"),
                "open".into(),
                None,
                "open a sphere scene",
                vec![],
                "maya".into(),
                b,
                false,
            )],
            InstanceFingerprint(1),
        );
        let snap = idx.snapshot();
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
    fn dcc_type_filter_drops_cross_dcc_matches() {
        let (idx, a, b) = fresh_index();
        push_one(&idx, "maya", a, "create_sphere", "", &[], true);
        push_one(&idx, "blender", b, "create_sphere", "", &[], true);
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "sphere".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.dcc_type, "maya");
    }

    #[test]
    fn tag_filter_requires_every_tag_to_be_present() {
        let (idx, a, b) = fresh_index();
        push_one(
            &idx,
            "maya",
            a,
            "read_scene",
            "",
            &["read-only", "scene"],
            true,
        );
        push_one(&idx, "maya", b, "export_fbx", "", &["destructive"], true);
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                tags: vec!["read-only".into(), "scene".into()],
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "read_scene");
        let hits = search(
            &snap,
            &SearchQuery {
                tags: vec!["nonexistent".into()],
                ..Default::default()
            },
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn limit_is_clamped_to_max() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..150)
            .map(|i| {
                CapabilityRecord::new(
                    tool_slug("maya", &iid, &format!("t{i:03}")),
                    format!("t{i:03}"),
                    None,
                    "",
                    vec![],
                    "maya".into(),
                    iid,
                    false,
                )
            })
            .collect();
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                limit: Some(9_999),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), MAX_LIMIT as usize);
    }

    #[test]
    fn zero_or_none_limit_falls_back_to_default() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..40)
            .map(|i| {
                CapabilityRecord::new(
                    tool_slug("maya", &iid, &format!("t{i:03}")),
                    format!("t{i:03}"),
                    None,
                    "",
                    vec![],
                    "maya".into(),
                    iid,
                    false,
                )
            })
            .collect();
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();
        let hits_default = search(&snap, &SearchQuery::default());
        assert_eq!(hits_default.len(), DEFAULT_LIMIT as usize);
        let hits_zero = search(
            &snap,
            &SearchQuery {
                limit: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(hits_zero.len(), DEFAULT_LIMIT as usize);
    }

    #[test]
    fn scene_hint_boosts_matching_records() {
        let (idx, a, b) = fresh_index();
        push_one(&idx, "maya", a, "export_fbx", "", &[], true);
        push_one(
            &idx,
            "maya",
            b,
            "open",
            "open a rig scene for character work",
            &["scene"],
            true,
        );
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "open".into(),
                scene_hint: Some("scene".into()),
                ..Default::default()
            },
        );
        assert_eq!(hits[0].record.backend_tool, "open");
    }

    // ========================================================================
    // New #659 behaviours
    // ========================================================================

    #[test]
    fn fuzzy_mode_tolerates_single_character_typo() {
        // Before #659 the substring scorer returned 0 hits for any
        // typo; agents relying on recall would see nothing. Fuzzy
        // mode (the new default) must surface the record.
        let (idx, a, _) = fresh_index();
        push_one(&idx, "maya", a, "create_sphere", "", &[], true);
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "creat_spher".into(), // missing two letters
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
    }

    #[test]
    fn fuzzy_mode_ranks_prefix_above_substring() {
        let (idx, a, b) = fresh_index();
        // `create_sphere` — query is a prefix.
        push_one(&idx, "maya", a, "create_sphere", "", &[], true);
        // `recreate_plane` — query is a mid-string substring.
        push_one(&idx, "maya", b, "recreate_plane", "", &[], true);
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "create".into(),
                ..Default::default()
            },
        );
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
        assert_eq!(hits[1].record.backend_tool, "recreate_plane");
    }

    #[test]
    fn instance_id_filter_drops_other_instances() {
        let (idx, a, b) = fresh_index();
        push_one(&idx, "maya", a, "create_sphere", "", &[], true);
        push_one(&idx, "maya", b, "create_sphere", "", &[], true);
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "sphere".into(),
                instance_id: Some(a),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.instance_id, a);
    }

    #[test]
    fn loaded_only_filter_drops_unloaded_records() {
        let (idx, a, b) = fresh_index();
        // Instance `a` carries the schema (loaded), `b` does not.
        push_one(&idx, "maya", a, "load_heavy", "", &[], true);
        push_one(&idx, "maya", b, "load_heavy", "", &[], false);
        let snap = idx.snapshot();
        let all = search(
            &snap,
            &SearchQuery {
                query: "load".into(),
                ..Default::default()
            },
        );
        assert_eq!(all.len(), 2);
        let loaded = search(
            &snap,
            &SearchQuery {
                query: "load".into(),
                loaded_only: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].record.has_schema);
        // `loaded_only: Some(false)` is a no-op — leave every record.
        let none = search(
            &snap,
            &SearchQuery {
                query: "load".into(),
                loaded_only: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(none.len(), 2);
    }

    #[test]
    fn pagination_returns_stable_slices() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..75)
            .map(|i| {
                CapabilityRecord::new(
                    tool_slug("maya", &iid, &format!("tool_{i:03}")),
                    format!("tool_{i:03}"),
                    None,
                    "",
                    vec![],
                    "maya".into(),
                    iid,
                    false,
                )
            })
            .collect();
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();

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
        let page_4 = search_page(
            &snap,
            &SearchQuery {
                limit: Some(20),
                offset: Some(60),
                ..Default::default()
            },
        );
        assert_eq!(page_1.total, 75);
        assert_eq!(page_2.total, 75);
        assert_eq!(page_4.total, 75);
        assert_eq!(page_1.hits.len(), 20);
        assert_eq!(page_2.hits.len(), 20);
        // Tail page holds the residual 75 - 60 = 15 records.
        assert_eq!(page_4.hits.len(), 15);
        // Pages must be disjoint.
        let seen: std::collections::HashSet<&str> = page_1
            .hits
            .iter()
            .chain(&page_2.hits)
            .chain(&page_4.hits)
            .map(|h| h.record.backend_tool.as_str())
            .collect();
        assert_eq!(seen.len(), 55); // 20 + 20 + 15
        // Offset past the total clamps to an empty page rather than
        // erroring — agents can walk off the end safely.
        let beyond = search_page(
            &snap,
            &SearchQuery {
                limit: Some(20),
                offset: Some(500),
                ..Default::default()
            },
        );
        assert!(beyond.hits.is_empty());
        assert_eq!(beyond.total, 75);
    }

    #[test]
    fn ranking_is_deterministic_across_reruns() {
        // Acceptance criterion "ranking is documented and
        // deterministic". Build a corpus with intentional score
        // ties so the tie-breaker path is exercised.
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records: Vec<CapabilityRecord> = (0..30)
            .map(|i| {
                CapabilityRecord::new(
                    tool_slug("maya", &iid, &format!("tool_{i:03}")),
                    format!("tool_{i:03}"),
                    None,
                    "shared summary text",
                    vec!["shared".into()],
                    "maya".into(),
                    iid,
                    true,
                )
            })
            .collect();
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();
        let slug_sequence = || -> Vec<String> {
            search(
                &snap,
                &SearchQuery {
                    query: "tool".into(),
                    limit: Some(30),
                    ..Default::default()
                },
            )
            .into_iter()
            .map(|h| h.record.tool_slug)
            .collect()
        };
        let first = slug_sequence();
        for _ in 0..5 {
            assert_eq!(slug_sequence(), first, "ranking must be deterministic");
        }
    }

    #[test]
    fn combined_filters_intersect() {
        // Guard against filter regressions: every filter must hold
        // simultaneously, with no silent OR fallthrough.
        //
        // NOTE: each `push_one` overwrites the per-instance slice,
        // so we need distinct instance ids even though the filter
        // under test is `dcc_type`, not `instance_id`.
        let idx = CapabilityIndex::new();
        let maya_a = Uuid::from_u128(0xaaaa_0000_0000_0000_0000_0000_0000_0001);
        let maya_b = Uuid::from_u128(0xaaaa_0000_0000_0000_0000_0000_0000_0002);
        let blender_c = Uuid::from_u128(0xbbbb_0000_0000_0000_0000_0000_0000_0001);
        push_one(&idx, "maya", maya_a, "read_scene", "", &["read-only"], true);
        push_one(
            &idx,
            "maya",
            maya_b,
            "write_scene",
            "",
            &["destructive"],
            true,
        );
        push_one(
            &idx,
            "blender",
            blender_c,
            "read_scene",
            "",
            &["read-only"],
            false,
        );
        let snap = idx.snapshot();
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

    // ========================================================================
    // Performance guard (issue #659 acceptance: "benchmarks or
    // targeted performance tests cover a realistic multi-DCC corpus")
    // ========================================================================

    #[test]
    fn fuzzy_search_stays_under_budget_on_5k_record_corpus() {
        // Build a 5k-record corpus spread over 5 DCC buckets and 50
        // instances. This is larger than any real deployment we
        // have observed (typical gateway = 2-5 backends * 20-100
        // tools each = 40-500 records) so a comfortable margin
        // under the budget here proves the index scales.
        let idx = CapabilityIndex::new();
        let dccs = ["maya", "blender", "houdini", "katana", "nuke"];
        let mut total_inserted = 0usize;
        for (d_idx, dcc) in dccs.iter().enumerate() {
            for inst in 0..10u128 {
                let iid =
                    Uuid::from_u128(((d_idx as u128) << 64) | ((inst + 1) << 32) | 0xdead_beef);
                let records: Vec<CapabilityRecord> = (0..100)
                    .map(|i| {
                        CapabilityRecord::new(
                            tool_slug(dcc, &iid, &format!("action_{i:03}")),
                            format!("action_{i:03}"),
                            Some(format!("{dcc}-skill-{}", i % 10)),
                            "a realistic summary blurb mentioning scene and object ops",
                            vec!["animation".into(), "schema:frame".into()],
                            (*dcc).into(),
                            iid,
                            i % 2 == 0,
                        )
                    })
                    .collect();
                total_inserted += records.len();
                idx.upsert_instance(iid, records, InstanceFingerprint(1 + inst as u64));
            }
        }
        assert_eq!(total_inserted, 5_000);
        let snap = idx.snapshot();

        // Warm up nucleo's scoring once so the first-call-only
        // pattern compilation does not skew the measurement.
        let _ = search(
            &snap,
            &SearchQuery {
                query: "warm".into(),
                limit: Some(10),
                ..Default::default()
            },
        );

        let start = Instant::now();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "anim".into(),
                limit: Some(25),
                ..Default::default()
            },
        );
        let elapsed = start.elapsed();
        assert!(!hits.is_empty(), "5k-record search must return hits");
        // Budget: fuzzy scoring 5k short strings on a cold path in a
        // CI runner (including Windows MSVC debug-built test
        // binaries) lands well under 1 s in local runs. We pick a
        // loose 2 s ceiling so this guard catches order-of-magnitude
        // regressions (an accidental O(n^2) scorer) without flapping
        // on slow shared runners.
        assert!(
            elapsed.as_millis() < 2_000,
            "fuzzy search on 5k records took {elapsed:?}, expected < 2s",
        );
    }
}
