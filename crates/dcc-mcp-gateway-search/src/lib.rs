//! Pure gateway capability search: wire types, ranking, and pagination.
//!
//! This crate has **no** dependency on `dcc-mcp-gateway` or HTTP stacks — only
//! `serde`, `uuid`, and `nucleo-matcher`.  Implement [`SearchRecord`] on your
//! compact index row type (for example in `dcc-mcp-gateway-core`) and call
//! [`search_page`].
//!
//! Dependency direction:
//!
//! ```text
//! dcc-mcp-gateway / dcc-mcp-gateway-core  →  dcc-mcp-gateway-search
//! ```

#![forbid(unsafe_code)]

mod engine;
mod query;
mod ranking;
mod record;

pub use engine::{search, search_page};
pub use query::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
};
pub use ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer, SubstringScorer,
};
pub use record::SearchRecord;
