//! Server configuration compatibility facade.
//!
//! The value-type surface, including `McpHttpConfig`, lives in
//! `dcc-mcp-http-types::config` (issue #852) so Rust tooling can validate and
//! round-trip HTTP configuration without depending on axum / tokio / reqwest /
//! pyo3. This module preserves the historical `dcc_mcp_http::config::*` path.

pub use dcc_mcp_http_types::config::{
    FeatureFlags, GatewayConfig, InstanceConfig, JobConfig, JobRecoveryPolicy, McpHttpConfig,
    QueueConfig, ServerConfig, ServerSpawnMode, SessionConfig, TelemetryConfig, WorkflowConfig,
};
