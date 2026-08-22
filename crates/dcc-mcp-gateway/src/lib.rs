//! Multi-DCC MCP gateway — extracted from `dcc-mcp-http`.
//!
//! The gateway aggregates multiple per-DCC MCP servers behind a single
//! HTTP endpoint, performs first-wins port election, and offers the
//! REST/MCP "dynamic capability" surface (#653 / #654 / #655).
//!
//! It is published as its own crate so that:
//! 1. Touching gateway code does not trigger a full recompile of the
//!    embedded MCP HTTP server (and vice versa).
//! 2. Downstream binaries that *only* need an embedded server (e.g.
//!    DCC adapters that never participate in gateway election) no
//!    longer have to compile the gateway code path.
//!
//! The admin audit/trace domain, compact response, link, issue-report, statistics, and analytics
//! projections, optional dashboard frontend, and Node/Vite build lifecycle are
//! isolated in `dcc-mcp-gateway-admin`. This crate contains only the gateway
//! application and its Rust-side admin adapters; builds without the `admin`
//! feature do not compile or run the frontend build crate.
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
