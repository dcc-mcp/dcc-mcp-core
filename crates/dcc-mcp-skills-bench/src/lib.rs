//! Reproducible skills benchmark for `dcc-mcp-core` (PIP-3408).
//!
//! Three dimensions, each with its own contract:
//!
//! | dimension | module | contract |
//! |---|---|---|
//! | hit rate | [`run`] | **hard gate** — a ranking regression is a bug |
//! | query efficiency | [`run`] + [`trend`] | no gate; compared against a rolling median, and reported |
//! | multi-DCC context growth | [`context`] | **hard cap** — paid from the user's context window |
//!
//! [`trend`] is what closes the gap latency used to leave open: the number was
//! recorded and nobody was told when it regressed. It is never a gate — see the
//! module docs for why, and for how a corpus change keeps the baseline honest.
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
//! cargo run --release -p dcc-mcp-skills-bench --bin skills-bench -- harvest-adapters
//! ```
//!
//! # Where the corpus comes from
//!
//! [`seeds`] harvests the skills this repository ships; [`adapters`] harvests
//! the pinned catalogues of the DCC adapter repositories. Both snapshots are
//! committed, so a run never depends on the working tree or on the network.
//! [`synthetic`] fills the rest, and [`recall`] measures how much structured
//! discovery metadata the real part carries.
//!
//! # Reading the numbers
//!
//! Hit rate is always reported split by [`run::Filter`]. The `dcc`-filtered
//! group narrows to a per-DCC shard before scoring; the unfiltered group
//! scores the whole catalogue. They are different problems and the gap between
//! them is itself a finding, so never read one without the other.

#![forbid(unsafe_code)]

pub mod adapters;
pub mod context;
pub mod corpus;
pub mod metrics;
pub mod queries;
pub mod recall;
pub mod report;
pub mod run;
pub mod seeds;
pub mod synthetic;
pub mod thresholds;
pub mod trend;

pub use corpus::Corpus;
pub use metrics::{HitRates, Latency};
pub use queries::{Query, QueryKind};
pub use recall::RecallCoverage;
pub use run::{Evaluation, Filter};
