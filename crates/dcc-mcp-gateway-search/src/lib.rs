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
//! dcc-mcp-gateway-core / dcc-mcp-catalog  →  dcc-mcp-gateway-search
//! ```

#![forbid(unsafe_code)]

mod engine;
mod query;
mod ranking;
mod record;

pub use engine::{rank_all, search, search_page};
pub use query::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
};
pub use ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer, SubstringScorer,
};
pub use record::SearchRecord;
