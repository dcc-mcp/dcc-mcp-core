//! Multi-DCC MCP gateway — extracted from `dcc-mcp-http`.
//!
//! The gateway aggregates multiple per-DCC MCP servers behind a single
//! HTTP endpoint, performs first-wins port election, and offers the
//! REST/MCP "dynamic capability" surface (#653 / #654 / #655).
//!
//! It is published as its own crate so that:
//! 1. Touching gateway code does not trigger a full recompile of the
//!    embedded MCP HTTP server (and vice versa) — gateway is the
//!    biggest module by far (~11k LoC across 53 files), so the
//!    incremental-build win is substantial.
//! 2. Downstream binaries that *only* need an embedded server (e.g.
//!    DCC adapters that never participate in gateway election) no
//!    longer have to compile the gateway code path.
//!
//! For backwards compatibility the entire surface is re-exported from
//! `dcc_mcp_http` under the historical `dcc_mcp_http::gateway` path —
//! every existing import keeps working without code changes.

// Keep the internal layout (`src/gateway/<sub>.rs`) intact so all
// existing `crate::gateway::*` references inside the moved files
// continue to compile unchanged. The lib root simply re-exports the
// full public surface.
pub mod gateway;

pub use gateway::*;
