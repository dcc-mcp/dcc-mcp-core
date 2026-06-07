//! Wire types for `search_tools` / `POST /v1/search`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which scoring strategy to use for a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Fuzzy matching with prefix/subsequence bonuses.
    #[default]
    Fuzzy,
    /// Substring-only matching (legacy deterministic table).
    Exact,
}

/// Parameters accepted by `search_tools` / `POST /v1/search`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchQuery {
    /// Free-text query matched against tool name, skill, summary, and tags.
    pub query: String,
    pub dcc_type: Option<String>,
    /// Additional DCC types matched in OR with `dcc_type`. Empty = no extra filter.
    #[serde(default)]
    pub dcc_types: Vec<String>,
    pub instance_id: Option<Uuid>,
    pub tags: Vec<String>,
    /// OR-tagged rows: a row carrying any of these tags passes the tag filter.
    /// `tags` is still AND. Empty = no extra filter.
    #[serde(default)]
    pub tags_any: Vec<String>,
    /// Case-insensitive exact tag match — rows carrying any of these tags are dropped.
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    pub loaded_only: Option<bool>,
    pub scene_hint: Option<String>,
    /// When set, hits with a final score strictly below this value are removed after ranking.
    pub min_score: Option<u32>,
    /// Soft score bonus when [`crate::SearchRecord::skill_name`] contains this substring (ASCII lowercased).
    pub skill_hint: Option<String>,
    /// Additional OR clauses: final score is the maximum across the primary [`query`](Self::query)
    /// (when non-empty) and each non-empty entry here.
    #[serde(default)]
    pub or_queries: Vec<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub mode: SearchMode,
}

/// Default page size for `search_tools`.
pub const DEFAULT_LIMIT: u32 = 25;
/// Upper bound on the number of results returned in a single page.
pub const MAX_LIMIT: u32 = 100;
/// Stable identifier for the current gateway ranking contract.
///
/// Bump this when score weights, match-reason vocabulary, or indexed fields
/// change in ways that can affect search telemetry dashboards.
pub const RANKER_VERSION: &str = "gateway-hybrid-v2";

/// Score plus bounded explanation metadata for one ranking decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScoreBreakdown {
    /// Final score used for ordering.
    pub score: u32,
    /// Stable, low-cardinality reasons explaining which fields matched.
    pub match_reasons: Vec<String>,
}

/// One ranked hit row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "R: serde::Serialize",
    deserialize = "R: serde::de::DeserializeOwned"
))]
pub struct SearchHit<R> {
    #[serde(flatten)]
    pub record: R,
    /// 1-based rank within the full filtered result set, after scoring.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rank: u32,
    pub score: u32,
    /// Stable, low-cardinality reasons explaining why the row matched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Paginated search response envelope (issue #659).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "R: serde::Serialize",
    deserialize = "R: serde::de::DeserializeOwned"
))]
pub struct SearchPage<R> {
    pub hits: Vec<SearchHit<R>>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}
