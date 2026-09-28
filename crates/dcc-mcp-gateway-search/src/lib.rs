//! Pure reusable DCC-MCP search: wire types, ranking, pagination, and the
//! explicit `dcc-cua` fallback route.
//!
//! This crate has **no** dependency on `dcc-mcp-gateway` or HTTP stacks — only
//! `serde`, `uuid`, and `nucleo-matcher`.  Implement [`SearchRecord`] on your
//! compact capability or catalog row type and call [`search_page`] or
//! [`rank_all`].
//!
//! Dependency direction:
//!
//! ```text
//! dcc-mcp-gateway-core / dcc-mcp-skills / dcc-mcp-skill-rest
//!     → dcc-mcp-gateway-search
//! ```

#![forbid(unsafe_code)]

mod engine;
mod fallback;
mod policy;
mod probe;
mod query;
mod ranking;
mod record;

pub use engine::{
    rank_all, resolve_fallback, resolve_fallback_among, resolve_fallback_with_policy, search,
    search_page, search_page_with_fallback,
};
pub use fallback::{
    CuaRuntimeProbe, CuaRuntimeState, FORBIDDEN_SUBSTITUTES, PREFLIGHT_ENSURE_CMD,
    PREFLIGHT_MANIFEST_CMD, PREFLIGHT_PING_CMD, PREFLIGHT_STATUS_CMD, SearchFallback,
    StaticCuaProbe, build_fallback, preflight_commands,
};
pub use policy::{
    FALLBACK_MIN_CONFIDENT_SCORE, FALLBACK_MIN_QUERY_LEN, FALLBACK_REASON_CUA_UNAVAILABLE,
    FALLBACK_REASON_LOW_CONFIDENCE, FALLBACK_REASON_NO_CANDIDATE,
    FALLBACK_REASON_NO_EXECUTABLE_INTERFACE, FALLBACK_SKILL, FallbackPolicy, FallbackTrigger,
    LAYER_DOMAIN, LAYER_EXAMPLE, LAYER_INFRASTRUCTURE, LAYER_THIN_HARNESS,
    PATH_SOURCE_ADMIN_CUSTOM, PATH_SOURCE_BUNDLED, PATH_SOURCE_ENV_VAR, PATH_SOURCE_EXPLICIT_ARG,
    PATH_SOURCE_LOCAL_DEV, PATH_SOURCE_PLATFORM, PATH_SOURCE_UNKNOWN, RankPolicy,
    apply_rank_policy, evaluate_fallback, is_fallback_target, layer_multiplier,
    path_source_multiplier,
};
pub use probe::{CLI_BIN, CUA_BIN, CliCuaProbe, PROBE_TIMEOUT, PathCuaProbe};
pub use query::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
};
pub use ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer, SubstringScorer,
};
pub use record::SearchRecord;

/// Fallback reason codes, re-exported as a namespace for callers that want to
/// match on them without importing each constant.
pub mod reason {
    pub use crate::policy::{
        FALLBACK_REASON_CUA_UNAVAILABLE, FALLBACK_REASON_LOW_CONFIDENCE,
        FALLBACK_REASON_NO_CANDIDATE, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE,
    };
}
