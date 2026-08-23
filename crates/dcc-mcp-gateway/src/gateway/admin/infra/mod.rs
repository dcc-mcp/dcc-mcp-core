//! Infrastructure layer — SQLite reads, log reads, integration config, asset serving.
//!
//! Contains modules that perform I/O and data aggregation against external stores.

pub mod activity;
pub(crate) mod audit_sink;
pub mod stats;

#[cfg(feature = "admin")]
pub(crate) mod integrations;

#[cfg(feature = "admin")]
pub(crate) mod wecom_response;

#[cfg(feature = "admin")]
pub(crate) mod wecom_url;

pub use activity::*;
pub use audit_sink::AdminAuditSink;
pub use stats::{
    GatewayStats, LatencyStats, StatsAggregator, StatsFilter, StatsRange, StatsStatus, TopEntry,
};

#[cfg(feature = "admin")]
pub use integrations::*;
