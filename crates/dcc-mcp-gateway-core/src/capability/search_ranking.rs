//! Compatibility re-exports — ranking implementations live in
//! [`dcc_mcp_gateway_search`].

pub use dcc_mcp_gateway_search::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, SearchRecord, StrategyExactScorer,
    StrategyFuzzyScorer, StrategyScorer, SubstringScorer,
};
