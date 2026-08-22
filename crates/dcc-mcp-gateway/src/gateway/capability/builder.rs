//! Compatibility facade for the pure capability builder.
//!
//! The implementation lives in `dcc-mcp-gateway-core`; this module keeps
//! historical gateway paths stable for downstream callers.

pub use dcc_mcp_gateway_core::capability::builder::{
    BuildInput, BuildOutcome, backend_job_status_tool, build_records_from_backend,
    is_backend_job_tool,
};
