//! Reproducible skills benchmark for `dcc-mcp-core` (PIP-3408).
//!
//! Three dimensions, each with its own contract:
//!
//! | dimension | module | contract |
//! |---|---|---|
//! | hit rate | [`run`] | **hard gate** — a ranking regression is a bug |
//! | query efficiency | [`run`] | trend signal only — CI machines are noisy |
//! | multi-DCC context growth | [`context`] | **hard cap** — paid from the user's context window |
//!
//! The benchmark runs the production scoring path
//! ([`dcc_mcp_skills::SkillCatalog::search_skills`] → `dcc-mcp-gateway-search`),
//! never a reimplementation of it, which is why it lives inside this workspace
//! rather than in a separate repository.
//!
//! # Running
//!
//! ```text
//! cargo run --release -p dcc-mcp-skills-bench --bin skills-bench -- report
//! cargo run --release -p dcc-mcp-skills-bench --bin skills-bench -- regenerate-seeds
//! ```
//!
//! # Reading the numbers
//!
//! Hit rate is always reported split by [`run::Filter`]. The `dcc`-filtered
//! group narrows to a per-DCC shard before scoring; the unfiltered group
//! scores the whole catalogue. They are different problems and the gap between
//! them is itself a finding, so never read one without the other.

#![forbid(unsafe_code)]

pub mod context;
pub mod corpus;
pub mod metrics;
pub mod queries;
pub mod report;
pub mod run;
pub mod seeds;
pub mod synthetic;
pub mod thresholds;

pub use corpus::Corpus;
pub use metrics::{HitRates, Latency};
pub use queries::{Query, QueryKind};
pub use run::{Evaluation, Filter};
