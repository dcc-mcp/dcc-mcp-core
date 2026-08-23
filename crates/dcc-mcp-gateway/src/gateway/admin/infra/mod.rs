//! Infrastructure layer — SQLite reads, log reads, integration config, asset serving.
//!
//! Contains modules that perform I/O and data aggregation against external stores.

pub mod activity;
pub(crate) mod audit_sink;

#[cfg(feature = "admin")]
pub(crate) mod integrations;

pub use activity::*;
pub use audit_sink::AdminAuditSink;

#[cfg(feature = "admin")]
pub use integrations::*;
