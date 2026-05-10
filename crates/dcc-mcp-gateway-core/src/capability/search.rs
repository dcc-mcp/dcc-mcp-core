//! Wire types for capability search (`POST /v1/search`).
//!
//! This module carries the **shape** of every search request /
//! response exchanged on the REST and MCP surfaces, nothing else.
//! The actual ranking loop that joins a [`SearchQuery`] against a
//! live capability index lives in `dcc-mcp-gateway` because it
//! needs the gateway-side `IndexSnapshot` and the pluggable
//! [`Scorer`] trait — both of which depend on runtime state the
//! domain layer has no business knowing about.
//!
//! # Wire contract
//!
//! Every type here is `Serialize` + `Deserialize`. Field names,
//! defaults, and [`serde(rename_all)`] attributes are part of the
//! `POST /v1/search` REST contract — adjust with care and bump the
//! contract docs if you change the shape.
//!
//! # Pre-#659 compatibility
//!
//! [`SearchQuery`] fields default to values that preserve the
//! pre-#659 behaviour, so existing clients that only set `query`
//! keep working without re-serialising.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::record::CapabilityRecord;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_mode_defaults_to_fuzzy() {
        assert_eq!(SearchMode::default(), SearchMode::Fuzzy);
    }

    #[test]
    fn search_mode_wire_is_snake_case() {
        // Preserves the pre-#659 wire contract: the JSON form of the
        // enum uses lowercase variant names. Regression guard so a
        // future derive tweak cannot silently break downstream REST
        // clients.
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
        // Existing clients that only set `query` must keep deserialising
        // without re-shaping their JSON; this is the contract #659
        // guaranteed and the types mirror it.
        let q: SearchQuery = serde_json::from_str(r#"{"query": "create sphere"}"#).unwrap();
        assert_eq!(q.query, "create sphere");
        assert_eq!(q.mode, SearchMode::Fuzzy);
    }

    #[test]
    fn search_hit_flattens_record_into_row() {
        // `#[serde(flatten)]` is part of the wire contract — the row
        // must carry CapabilityRecord fields at the top level, not
        // nested under a `record` key, or agents parsing the pre-#659
        // shape would break.
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
        // A nested `record` object would be a wire break.
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
        // These values are part of the REST contract; tightening or
        // loosening them silently would change client-visible
        // behaviour. Pin them here so any change forces a conscious
        // update of the contract docs.
        assert_eq!(DEFAULT_LIMIT, 25);
        assert_eq!(MAX_LIMIT, 100);
    }
}
