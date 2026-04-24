//! In-process async job tracker for MCP tool calls (issue #316).
//!
//! `JobManager` provides a lightweight, thread-safe registry for async
//! tool-call lifecycles.  It is used by the MCP HTTP server to expose job
//! status / progress / cancellation to clients without losing state when
//! `handle_tools_call` returns.
//!
//! This module is intentionally pure-Rust.  Python bindings are deferred to
//! issue #319 where a coherent user-facing API (`jobs.get_status`,
//! `jobs.cancel`, …) lands together.
//!
//! # Concurrency model
//!
//! - Jobs are stored in `DashMap<String, Arc<RwLock<Job>>>` — per-entry locks
//!   keep contention local to a single job.
//! - `parking_lot::RwLock` is used instead of `std::sync::RwLock` for
//!   performance and consistency with the rest of the workspace.
//! - `cancel_token` is a `tokio_util::sync::CancellationToken` so long-running
//!   async tool handlers can observe cancellation via `.cancelled().await`.
//!
//! # State machine
//!
//! ```text
//! Pending ──► Running ──► Completed
//!    │           │        ╰► Failed
//!    │           │        ╰► Cancelled
//!    │           ╰► Cancelled
//!    ╰► Cancelled
//! ```
//!
//! Invalid transitions (e.g. `Completed → Running`) are rejected: the mutator
//! returns `None` and logs at `debug` level.  The stored job state is left
//! unchanged.
//!
//! ## Maintainer layout
//!
//! This module is a **thin facade** that re-exports the public surface.
//! Implementation is split across sibling files:
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `job_types.rs`   | `JobStatus`, `JobProgress`, `Job`, `JobEvent`, `JobSubscriber` |
//! | `job_manager.rs` | `JobManager` — registry, transitions, persistence, subscribers, GC |
//! | `job_tests.rs`   | Unit tests (lifecycle, cancellation, invalid transitions, GC, serde) |

#[path = "job_types.rs"]
mod types;

#[path = "job_manager.rs"]
mod manager;

#[cfg(test)]
#[path = "job_tests.rs"]
mod tests;

pub use manager::JobManager;
pub use types::{Job, JobEvent, JobProgress, JobStatus, JobSubscriber};
