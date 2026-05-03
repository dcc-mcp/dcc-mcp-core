//! Backend SSE subscription + multiplexing (#320).
//!
//! This module lets the gateway multiplex notifications emitted by each
//! backend DCC server (`notifications/progress`, `$/dcc.jobUpdated`,
//! `$/dcc.workflowUpdated`) back to the *originating* client sessions.
//!
//! # Architecture
//!
//! ```text
//!   client_session_A ──────┐
//!                           \     ┌─────────── gateway ─────────────┐
//!   client_session_B ────────┼──>│ SubscriberManager               │
//!                           /    │   backends: url → BackendSub     │
//!   client_session_C ──────┘    │   job_routes: jobId → session     │
//!                               │   progress_routes: tok → session  │
//!                               │   inflight: url → {sessions}      │
//!                               │   client_sinks: session → tx       │
//!                               └─────┬───────────────┬────────────┘
//!                                     │ GET /mcp (backend)           │
//!                                     ▼                              ▼
//!                                backend-1 SSE               backend-2 SSE
//! ```
//!
//! ## Correlation
//!
//! * `notifications/progress` carries `params.progressToken` — resolve against
//!   `progress_token_routes` (set at outbound `tools/call` time).
//! * `$/dcc.jobUpdated` / `$/dcc.workflowUpdated` carries `params.job_id` —
//!   resolve against `job_routes` (set from `_meta.dcc.jobId` on the reply).
//!
//! If a notification arrives before either correlation is known it is
//! buffered for up to 30 s (or 256 events, whichever comes first) and
//! replayed once the mapping appears; otherwise dropped with a `warn!`.
//!
//! ## Reconnect
//!
//! Each [`BackendSubscriber`] owns an exponential-backoff retry loop
//! (start 100 ms → max 10 s, 25 % jitter). When a broken stream is
//! restored the subscriber emits a synthetic `$/dcc.gatewayReconnect`
//! notification to every client that had an in-flight job on that
//! backend (tracked in `backend_inflight`).

mod backend;
mod delivery;
mod helpers;
mod job_bus;
mod manager;
mod reconnect;
mod resource_subs;
mod route_gc;
#[cfg(test)]
mod tests;
mod types;

pub use manager::SubscriberManager;
pub(crate) use types::ResourceSubscriberRoute;
pub use types::{BackendId, BindJobError, ClientSessionId, JobRoute};

#[cfg(test)]
pub(crate) use backend::{BackendShared, BackendSubscriber};
#[cfg(test)]
pub(crate) use helpers::{backoff_delay, parse_sse_record, progress_token_key, resolve_target};
#[cfg(test)]
pub(crate) use manager::SubscriberManagerInner;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use dcc_mcp_jsonrpc::format_sse_event;

/// How long a notification with an unknown target may sit in the pending
/// buffer before being dropped.
pub(crate) const PENDING_BUFFER_TTL: Duration = Duration::from_secs(30);

/// Maximum number of notifications with unknown target buffered per backend.
pub(crate) const PENDING_BUFFER_CAP: usize = 256;

/// Initial reconnect delay after the backend SSE stream dies.
pub(crate) const RECONNECT_INITIAL: Duration = Duration::from_millis(100);

/// Ceiling on the reconnect delay.
pub(crate) const RECONNECT_MAX: Duration = Duration::from_secs(10);

/// Jitter multiplier applied to each reconnect delay (±25 %).
pub(crate) const RECONNECT_JITTER: f32 = 0.25;

/// Idle/read timeout applied to the established backend SSE stream.
///
/// This caps how long the subscriber waits between consecutive chunks
/// on the response body — **not** the total request duration. It must
/// be noticeably larger than the server-side SSE keep-alive interval
/// (axum's `KeepAlive::default()` emits a heartbeat every 15 s), so we
/// pick 60 s to tolerate GC pauses and transient network stalls while
/// still failing fast if the backend goes silent.
///
/// Do NOT pass this into `RequestBuilder::timeout()`; that would abort
/// the long-lived stream after this interval and trigger an endless
/// reconnect loop.
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Default ceiling on how long a non-terminal `JobRoute` may live in
/// the gateway's routing cache (`gateway_route_ttl`, issue #322).
pub(crate) const DEFAULT_ROUTE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

/// Default ceiling on concurrent live routes per client session
/// (`gateway_max_routes_per_session`, issue #322).
pub(crate) const DEFAULT_MAX_ROUTES_PER_SESSION: usize = 1_000;

/// Cadence of the background GC that evicts stale `JobRoute`s.
pub(crate) const ROUTE_GC_INTERVAL: Duration = Duration::from_secs(60);
