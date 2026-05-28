//! Configuration value types exposed on the HTTP server wire surface.
//!
//! These are the enums and small value types that the Python binding,
//! CLI flags, and environment-variable plumbing branch on. They live
//! here (rather than in `dcc-mcp-http::config`) so external Rust
//! tooling — CLI drivers, config validators, adapter orchestrators —
//! can depend on just the enumeration contract without dragging in
//! `axum` / `tokio` / `reqwest` / `pyo3`.
//!
//! The full `McpHttpConfig` aggregate lives in the sibling [`aggregate`]
//! module and is re-exported here alongside the cohesive sub-config
//! structs it composes.

mod aggregate;
mod feature_flags;
mod gateway;
mod instance;
mod job;
mod queue;
mod server;
mod session;
mod telemetry;
mod workflow;

pub use aggregate::McpHttpConfig;
pub use feature_flags::FeatureFlags;
pub use gateway::{GatewayConfig, RelaySourceConfig};
pub use instance::InstanceConfig;
pub use job::{JobConfig, JobRecoveryPolicy};
pub use queue::QueueConfig;
pub use server::{ServerConfig, ServerSpawnMode};
pub use session::SessionConfig;
pub use telemetry::TelemetryConfig;
pub use workflow::WorkflowConfig;

#[cfg(test)]
mod tests;
