//! Pure reusable DCC-MCP search: wire types, ranking, and pagination.
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
mod policy;
mod query;
mod ranking;
mod record;

pub use engine::{rank_all, search, search_page};
pub use policy::{
    LAYER_DOMAIN, LAYER_EXAMPLE, LAYER_INFRASTRUCTURE, LAYER_THIN_HARNESS,
    PATH_SOURCE_ADMIN_CUSTOM, PATH_SOURCE_BUNDLED, PATH_SOURCE_ENV_VAR, PATH_SOURCE_EXPLICIT_ARG,
    PATH_SOURCE_LOCAL_DEV, PATH_SOURCE_PLATFORM, PATH_SOURCE_UNKNOWN, RankPolicy,
    apply_rank_policy, layer_multiplier, path_source_multiplier,
};
pub use query::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
};
pub use ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer, SubstringScorer,
};
pub use record::SearchRecord;
