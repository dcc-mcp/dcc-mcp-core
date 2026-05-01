//! In-process async job tracker + pluggable persistence (issues #316, #328).
//!
//! Extracted from `dcc-mcp-http` so that:
//! 1. The optional SQLite persistence backend (`job-persist-sqlite`)
//!    no longer pollutes the HTTP server's feature matrix.
//! 2. Alternative servers (and tests) can depend on the job machinery
//!    without pulling in axum/tokio/reqwest.
//!
//! Re-exported under `dcc_mcp_http::{job, job_storage}` for backwards
//! compatibility — see the alias in `dcc-mcp-http/src/lib.rs`.

pub mod job;
pub mod job_storage;
