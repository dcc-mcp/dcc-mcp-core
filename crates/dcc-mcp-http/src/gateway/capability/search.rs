//! Keyword search over the capability index.
//!
//! The scoring model is deliberately boring — dcc-mcp-core does not
//! run semantic embeddings in-process, and the #657 non-goals
//! explicitly forbid doing so before keyword/tag search lands.
//!
//! Score contributions (higher = better):
//!
//! | Signal | Weight |
//! |--------|--------|
//! | Exact match on `backend_tool`     | 10 |
//! | Substring match on `backend_tool` |  6 |
//! | Exact match on any `tag`          |  5 |
//! | Substring match on `skill_name`   |  4 |
//! | Substring match on `summary`      |  2 |
//! | Scene/document hint match         |  2 |
//!
//! Absent signals contribute 0, not a negative — so an empty query
//! still yields deterministic ordering by (dcc_type, slug). DCC-type
//! and tag filters act as **filters**, not score nudges: records in
//! the wrong bucket are dropped before scoring so the
//! "query matches but score is zero" rows cannot sneak into the
//! response and waste the caller's token budget.

use serde::{Deserialize, Serialize};

use super::index::IndexSnapshot;
use super::record::CapabilityRecord;

/// Parameters accepted by `search_tools` / `POST /v1/search`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchQuery {
    /// Free-text query matched against tool name, skill, summary,
    /// and tags. Empty string disables keyword ranking.
    pub query: String,
    /// Restrict results to a specific DCC bucket (`"maya"`, …).
    pub dcc_type: Option<String>,
    /// Optional domain tags the caller wants to filter by — records
    /// that do not carry every listed tag are dropped.
    pub tags: Vec<String>,
    /// Optional scene / document hint; used as a soft boost rather
    /// than a filter because agents often pass inaccurate hints.
    pub scene_hint: Option<String>,
    /// Cap on the number of hits returned. `0` means "no cap", but
    /// we still apply [`DEFAULT_LIMIT`] behind the scenes to keep
    /// tokens bounded.
    pub limit: Option<u32>,
}

/// Default page size for `search_tools` — keeps the response token
/// cost modest even when the caller forgets to pass `limit`.
pub const DEFAULT_LIMIT: u32 = 25;
/// Upper bound on the number of results returned in a single page.
pub const MAX_LIMIT: u32 = 100;

/// One result row in the search response. Same wire shape in REST
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

/// Rank `snapshot` against `query` and return the top-N hits.
///
/// The function is pure and synchronous so it can be called directly
/// by both REST and MCP handlers without any awaiting — every
/// snapshot clone is already detached from the index's read lock.
pub fn search(snapshot: &IndexSnapshot, query: &SearchQuery) -> Vec<SearchHit> {
    let qnorm = query.query.trim().to_ascii_lowercase();
    let dcc_filter = query.dcc_type.as_deref();
    let tags_filter: Vec<String> = query
        .tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let scene = query.scene_hint.as_deref().map(|s| s.to_ascii_lowercase());

    let mut hits: Vec<SearchHit> = snapshot
        .records
        .iter()
        .filter(|r| dcc_filter.is_none_or(|f| r.dcc_type == f))
        .filter(|r| {
            tags_filter
                .iter()
                .all(|t| r.tags.iter().any(|rt| rt.to_ascii_lowercase() == *t))
        })
        .map(|r| SearchHit {
            record: r.clone(),
            score: score_record(r, &qnorm, scene.as_deref()),
        })
        // When the caller typed a query, filter out records that do
        // not contribute any match signal at all — keeping them would
        // poison the token budget with irrelevant rows ranked purely
        // by slug. When the query is empty, keep every record so the
        // caller can browse the full catalogue deterministically.
        .filter(|hit| qnorm.is_empty() || hit.score > 0)
        .collect();

    hits.sort_by(|a, b| {
        // Primary sort: score descending.
        b.score
            .cmp(&a.score)
            // Tie-breaker: alphabetical slug so results stay stable
            // across reruns. Never rely on hash-map iteration order.
            .then_with(|| a.record.tool_slug.cmp(&b.record.tool_slug))
    });

    let effective_limit = effective_limit(query.limit);
    hits.truncate(effective_limit as usize);
    hits
}

fn effective_limit(limit: Option<u32>) -> u32 {
    match limit {
        None => DEFAULT_LIMIT,
        Some(0) => DEFAULT_LIMIT,
        Some(n) => n.min(MAX_LIMIT),
    }
}

fn score_record(r: &CapabilityRecord, q: &str, scene_hint: Option<&str>) -> u32 {
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

    if let Some(hint) = scene_hint {
        if r.summary.to_ascii_lowercase().contains(hint)
            || r.tags.iter().any(|t| t.to_ascii_lowercase() == hint)
        {
            score += 2;
        }
    }

    score
}

#[cfg(test)]
mod unit_tests {
    use super::super::index::{CapabilityIndex, InstanceFingerprint};
    use super::super::record::{CapabilityRecord, tool_slug};
    use super::*;
    use uuid::Uuid;

    fn push(idx: &CapabilityIndex, dcc: &str, iid: Uuid, name: &str, summary: &str, tags: &[&str]) {
        let rec = CapabilityRecord::new(
            tool_slug(dcc, &iid, name),
            name.to_string(),
            None,
            summary,
            tags.iter().map(|t| t.to_string()).collect(),
            dcc.to_string(),
            iid,
            true,
        );
        // Overwrite the per-instance slice to include only this record
        // for the test; real builders ship sorted arrays.
        idx.upsert_instance(iid, vec![rec], InstanceFingerprint(1));
    }

    fn fresh_index() -> (CapabilityIndex, Uuid, Uuid) {
        let idx = CapabilityIndex::new();
        let a = Uuid::from_u128(0xaaaa_aaaa_0000_0000_0000_0000_0000_0001);
        let b = Uuid::from_u128(0xbbbb_bbbb_0000_0000_0000_0000_0000_0001);
        (idx, a, b)
    }

    #[test]
    fn empty_query_returns_all_records_within_limit() {
        let (idx, a, _) = fresh_index();
        push(&idx, "maya", a, "create_sphere", "make a sphere", &["geo"]);
        let snap = idx.snapshot();
        let hits = search(&snap, &SearchQuery::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.backend_tool, "create_sphere");
    }

    #[test]
    fn exact_tool_name_beats_substring_beats_summary() {
        let (idx, a, b) = fresh_index();
        // Insert three records with decreasing specificity for the
        // query term `sphere`.
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
                ..Default::default()
            },
        );
        assert_eq!(hits[0].record.backend_tool, "sphere"); // exact
        assert_eq!(hits[1].record.backend_tool, "create_sphere"); // substring
        assert_eq!(hits[2].record.backend_tool, "open"); // summary-only
    }

    #[test]
    fn dcc_type_filter_drops_cross_dcc_matches() {
        let (idx, a, b) = fresh_index();
        push(&idx, "maya", a, "create_sphere", "", &[]);
        push(&idx, "blender", b, "create_sphere", "", &[]);
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
        push(&idx, "maya", a, "read_scene", "", &["read-only", "scene"]);
        push(&idx, "maya", b, "export_fbx", "", &["destructive"]);
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
        // Asking for a tag no record carries returns zero hits
        // rather than silently falling back to all tools.
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
        push(&idx, "maya", a, "export_fbx", "", &[]);
        push(
            &idx,
            "maya",
            b,
            "open",
            "open a rig scene for character work",
            &["scene"],
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
        // The scene hint and the scene tag both score — the `open`
        // record wins over `export_fbx`.
        assert_eq!(hits[0].record.backend_tool, "open");
    }
}
