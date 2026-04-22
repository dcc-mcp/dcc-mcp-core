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

use crate::protocol::format_sse_event;

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

/// Request timeout used when opening the backend SSE stream.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Identifier for a client-side MCP session.
pub type ClientSessionId = String;

/// Identifier for a backend DCC server. Conventionally the backend's
/// MCP URL (`http://host:port/mcp`) — stable for the life of the
/// instance and sufficient for cancel forwarding.
pub type BackendId = String;

/// Default ceiling on how long a non-terminal `JobRoute` may live in
/// the gateway's routing cache (`gateway_route_ttl`, issue #322).
pub(crate) const DEFAULT_ROUTE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

/// Default ceiling on concurrent live routes per client session
/// (`gateway_max_routes_per_session`, issue #322).
pub(crate) const DEFAULT_MAX_ROUTES_PER_SESSION: usize = 1_000;

/// Cadence of the background GC that evicts stale `JobRoute`s.
pub(crate) const ROUTE_GC_INTERVAL: Duration = Duration::from_secs(60);

/// Gateway-owned routing entry for a single async job (issue #322).
///
/// Populated when the gateway forwards a `tools/call` and the backend
/// replies with a `job_id`; consulted on `notifications/cancelled` so
/// the cancel can be propagated to the exact backend that owns the
/// job. The `parent_job_id` link lets the gateway fan a cancel out
/// across backends when a workflow parent is cancelled (#318 cascade).
#[derive(Debug, Clone)]
pub struct JobRoute {
    /// Owning client session — used to route backend SSE notifications
    /// back to the originator (the pre-#322 behaviour).
    pub client_session_id: ClientSessionId,
    /// Backend that runs this job (stable for the job's lifetime —
    /// routes are sticky, no multi-backend failover, per #322).
    pub backend_id: BackendId,
    /// Tool name reported on dispatch, kept for cancel-payload logs.
    pub tool: String,
    /// Wall-clock time the route was created — drives TTL GC.
    pub created_at: DateTime<Utc>,
    /// Parent job id when this job was dispatched under a workflow
    /// (`_meta.dcc.parentJobId`). A cancel on the parent cascades to
    /// every child route, even across backends.
    pub parent_job_id: Option<String>,
}

/// Error returned when a new route cannot be admitted to the gateway
/// routing cache (issue #322).
#[derive(Debug, Clone)]
pub enum BindJobError {
    /// The owning session already holds `cap` live routes. The gateway
    /// surfaces this as a JSON-RPC `-32005 too_many_in_flight_jobs`
    /// error so AI clients can back off or cancel in-flight jobs.
    TooManyInFlight {
        session_id: ClientSessionId,
        live: usize,
        cap: usize,
    },
}

impl std::fmt::Display for BindJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindJobError::TooManyInFlight {
                session_id,
                live,
                cap,
            } => write!(
                f,
                "too_many_in_flight_jobs: session {session_id} holds {live} live routes (cap {cap})"
            ),
        }
    }
}

impl std::error::Error for BindJobError {}

/// A notification buffered while its target mapping is still unknown.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    inserted_at: Instant,
    value: Value,
}

/// Per-backend subscription state.
///
/// Public fields are `pub(crate)` so the gateway's `start_gateway_tasks`
/// can spawn / abort the reconnect loop directly.
pub struct BackendSubscriber {
    /// Absolute URL of the backend MCP endpoint (`http://host:port/mcp`).
    #[allow(dead_code)]
    pub(crate) url: String,
    /// Reconnect loop JoinHandle. `None` when the subscriber was never
    /// started or has been aborted.
    pub(crate) task: Option<JoinHandle<()>>,
    /// Shared state with the reconnect task.
    pub(crate) shared: Arc<BackendShared>,
}

impl BackendSubscriber {
    /// Abort the reconnect loop. Idempotent.
    pub fn abort(&mut self) {
        if let Some(h) = self.task.take() {
            h.abort();
        }
    }
}

impl Drop for BackendSubscriber {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Shared state for a single backend's reconnect loop.
pub(crate) struct BackendShared {
    /// Backend URL for logging.
    pub(crate) url: String,
    /// Per-backend bounded buffer of notifications whose target session
    /// could not yet be resolved.
    pub(crate) pending: Mutex<VecDeque<Pending>>,
    /// Number of consecutive reconnect attempts (reset on a successful
    /// open of the SSE stream).
    pub(crate) reconnect_attempts: Mutex<u32>,
}

impl BackendShared {
    fn new(url: String) -> Self {
        Self {
            url,
            pending: Mutex::new(VecDeque::with_capacity(PENDING_BUFFER_CAP)),
            reconnect_attempts: Mutex::new(0),
        }
    }
}

/// Central multiplexer for backend SSE streams.
///
/// A single instance is owned by [`crate::gateway::state::GatewayState`] and
/// shared across every gateway handler. `ensure_subscribed` is the
/// single entry point callers need — the first request for a given
/// backend URL spawns a long-lived reconnect task; subsequent requests
/// are cheap DashMap lookups.
#[derive(Clone)]
pub struct SubscriberManager {
    inner: Arc<SubscriberManagerInner>,
}

struct SubscriberManagerInner {
    backends: DashMap<String, BackendSubscriber>,
    /// `job_id` → full [`JobRoute`] (issue #322).
    ///
    /// Before v0.15 this was `DashMap<String, ClientSessionId>`; callers
    /// that only need the session id now read `route.client_session_id`.
    job_routes: DashMap<String, JobRoute>,
    /// Reverse index: `client_session_id` → set of live `job_id`s. Used
    /// to enforce the per-session cap without walking the whole
    /// `job_routes` map on every insert.
    session_jobs: DashMap<ClientSessionId, DashSet<String>>,
    /// Reverse index: gateway JSON-RPC `requestId` (stringified) →
    /// dispatched `job_id`. Populated at dispatch time for async jobs
    /// so `notifications/cancelled { requestId }` can resolve to a
    /// `JobRoute` even after the original RPC has already returned
    /// (issue #322).
    request_to_job: DashMap<String, String>,
    /// `progressToken` (serialised JSON) → owning client session.
    progress_token_routes: DashMap<String, ClientSessionId>,
    /// Backend URL → set of client sessions with in-flight jobs on that
    /// backend. Used for `$/dcc.gatewayReconnect` fan-out.
    backend_inflight: DashMap<String, DashSet<ClientSessionId>>,
    /// Client session → broadcast::Sender used by the GET /mcp handler.
    client_sinks: DashMap<ClientSessionId, broadcast::Sender<String>>,
    /// Per-`job_id` broadcast of parsed `$/dcc.jobUpdated` / `workflowUpdated`
    /// JSON-RPC notifications (#321 wait-for-terminal passthrough).
    ///
    /// The bus is created lazily by [`SubscriberManager::job_event_channel`]
    /// — typically called from the gateway aggregator just before
    /// forwarding an async `tools/call` so the waiter cannot miss a
    /// terminal event that arrives while the POST reply is in flight.
    job_event_buses: DashMap<String, broadcast::Sender<Value>>,
    /// Shared HTTP client with connection pooling.
    http_client: reqwest::Client,
    /// TTL beyond which a non-terminal route is evicted by the GC task
    /// (issue #322).
    route_ttl: Duration,
    /// Per-session ceiling on concurrent live routes (issue #322). `0`
    /// disables the cap.
    max_routes_per_session: usize,
}

impl Default for SubscriberManager {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

impl SubscriberManager {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self::with_limits(
            http_client,
            DEFAULT_ROUTE_TTL,
            DEFAULT_MAX_ROUTES_PER_SESSION,
        )
    }

    /// Construct with explicit routing-cache limits (issue #322). A
    /// `max_routes_per_session` of `0` is treated as unlimited — caps
    /// are opt-in by configuration.
    pub fn with_limits(
        http_client: reqwest::Client,
        route_ttl: Duration,
        max_routes_per_session: usize,
    ) -> Self {
        Self {
            inner: Arc::new(SubscriberManagerInner {
                backends: DashMap::new(),
                job_routes: DashMap::new(),
                session_jobs: DashMap::new(),
                request_to_job: DashMap::new(),
                progress_token_routes: DashMap::new(),
                backend_inflight: DashMap::new(),
                client_sinks: DashMap::new(),
                job_event_buses: DashMap::new(),
                http_client,
                route_ttl,
                max_routes_per_session,
            }),
        }
    }

    // ── Client-side API ────────────────────────────────────────────────

    /// Register (or replace) the broadcast::Sender used to deliver SSE
    /// events to `session_id`. Returns a fresh receiver that the
    /// GET /mcp handler can forward onto its axum SSE stream.
    pub fn register_client(&self, session_id: &str) -> broadcast::Receiver<String> {
        let (tx, rx) = broadcast::channel::<String>(128);
        self.inner.client_sinks.insert(session_id.to_string(), tx);
        rx
    }

    /// Remove a client session and drop its sink. Any future
    /// notifications destined for this session are dropped silently.
    pub fn forget_client(&self, session_id: &str) {
        self.inner.client_sinks.remove(session_id);
        // Scrub the backend_inflight index so a later reconnect on some
        // backend does not try to notify this long-gone session.
        for entry in self.inner.backend_inflight.iter() {
            entry.value().remove(session_id);
        }
        // Scrub routing tables to avoid memory growth. We don't scan
        // `progress_token_routes` keys eagerly (tokens are short-lived)
        // but removing job_routes bound to this session is cheap.
        let mut dropped_jobs: Vec<String> = Vec::new();
        self.inner.job_routes.retain(|job_id, route| {
            let keep = route.client_session_id.as_str() != session_id;
            if !keep {
                dropped_jobs.push(job_id.clone());
            }
            keep
        });
        for jid in &dropped_jobs {
            self.inner.job_event_buses.remove(jid);
        }
        // The reverse index for this session is now redundant.
        self.inner.session_jobs.remove(session_id);
        // Orphaned request_to_job entries would be cheap to keep, but
        // may as well scrub them.
        self.inner
            .request_to_job
            .retain(|_, jid| !dropped_jobs.iter().any(|d| d == jid));
        self.inner
            .progress_token_routes
            .retain(|_, sid| sid.as_str() != session_id);
    }

    // ── Correlation updates ────────────────────────────────────────────

    /// Associate a `progressToken` seen on the outbound `tools/call`
    /// with the initiating client session.
    pub fn bind_progress_token(&self, token: &Value, session_id: &str) {
        let key = progress_token_key(token);
        self.inner
            .progress_token_routes
            .insert(key, session_id.to_string());
    }

    /// Associate a `job_id` extracted from a backend reply with its
    /// owning client session, backend, and tool name (issue #322). Also
    /// registers the session as having an in-flight job on `backend_url`
    /// so that a later reconnect on that backend can emit
    /// `$/dcc.gatewayReconnect`.
    ///
    /// Returns [`BindJobError::TooManyInFlight`] when the session has
    /// reached `gateway_max_routes_per_session`; the gateway translates
    /// this into a JSON-RPC `-32005` error so clients can back off or
    /// cancel in-flight jobs.
    pub fn bind_job_route(
        &self,
        job_id: &str,
        session_id: &str,
        backend_url: &str,
        tool: &str,
        parent_job_id: Option<&str>,
    ) -> Result<(), BindJobError> {
        if self.inner.max_routes_per_session > 0 {
            let live = self
                .inner
                .session_jobs
                .get(session_id)
                .map(|e| e.value().len())
                .unwrap_or(0);
            if live >= self.inner.max_routes_per_session {
                return Err(BindJobError::TooManyInFlight {
                    session_id: session_id.to_string(),
                    live,
                    cap: self.inner.max_routes_per_session,
                });
            }
        }
        let route = JobRoute {
            client_session_id: session_id.to_string(),
            backend_id: backend_url.to_string(),
            tool: tool.to_string(),
            created_at: Utc::now(),
            parent_job_id: parent_job_id.map(str::to_owned),
        };
        self.inner.job_routes.insert(job_id.to_string(), route);
        self.inner
            .session_jobs
            .entry(session_id.to_string())
            .or_default()
            .insert(job_id.to_string());
        self.inner
            .backend_inflight
            .entry(backend_url.to_string())
            .or_default()
            .insert(session_id.to_string());
        self.flush_pending_for_backend(backend_url);
        Ok(())
    }

    /// Back-compat shim for the pre-#322 signature. Still used by a few
    /// tests; new code should prefer [`bind_job_route`].
    #[cfg(test)]
    pub fn bind_job(&self, job_id: &str, session_id: &str, backend_url: &str) {
        let _ = self.bind_job_route(job_id, session_id, backend_url, "", None);
    }

    /// Associate a gateway `requestId` with the `job_id` produced for
    /// that async dispatch (issue #322). `notifications/cancelled`
    /// carries only the `requestId`, so this reverse index lets the
    /// gateway resolve back to a [`JobRoute`] even after the original
    /// RPC has returned.
    pub fn bind_request_to_job(&self, request_id: &str, job_id: &str) {
        self.inner
            .request_to_job
            .insert(request_id.to_string(), job_id.to_string());
    }

    /// Lookup helper for the cancel path.
    pub fn job_id_for_request(&self, request_id: &str) -> Option<String> {
        self.inner
            .request_to_job
            .get(request_id)
            .map(|e| e.value().clone())
    }

    /// Drop the `request_id → job_id` mapping (e.g. when the dispatch
    /// is resolved client-side and cannot be cancelled any more).
    #[allow(dead_code)]
    pub fn forget_request(&self, request_id: &str) {
        self.inner.request_to_job.remove(request_id);
    }

    /// Fetch a cloned [`JobRoute`] for `job_id`, if any.
    pub fn job_route(&self, job_id: &str) -> Option<JobRoute> {
        self.inner.job_routes.get(job_id).map(|e| e.value().clone())
    }

    /// Return every route whose `parent_job_id == parent` (issue #322
    /// cross-backend cascade).
    pub fn children_of(&self, parent: &str) -> Vec<(String, JobRoute)> {
        self.inner
            .job_routes
            .iter()
            .filter_map(|e| match &e.value().parent_job_id {
                Some(p) if p == parent => Some((e.key().clone(), e.value().clone())),
                _ => None,
            })
            .collect()
    }

    /// Total number of live routes (introspection for tests + metrics).
    pub fn route_count(&self) -> usize {
        self.inner.job_routes.len()
    }

    /// Forget a `job_id` once the gateway has observed a terminal event
    /// (the subscriber loop does this automatically on `$/dcc.jobUpdated`
    /// with a terminal status — callers typically don't need to invoke
    /// it themselves).
    pub fn forget_job(&self, job_id: &str) {
        let removed = self.inner.job_routes.remove(job_id);
        self.inner.job_event_buses.remove(job_id);
        if let Some((_, route)) = removed {
            if let Some(set) = self.inner.session_jobs.get(&route.client_session_id) {
                set.value().remove(job_id);
            }
        }
        self.inner
            .request_to_job
            .retain(|_, jid| jid.as_str() != job_id);
    }

    // ── Job event bus (#321 wait-for-terminal) ─────────────────────────

    /// Subscribe to parsed `$/dcc.jobUpdated` / `workflowUpdated`
    /// JSON-RPC notifications for `job_id`. Idempotent — repeated calls
    /// return independent receivers reading from the same broadcast.
    ///
    /// Callers should invoke this **before** forwarding the outbound
    /// `tools/call` so that a terminal event produced during the brief
    /// window between the backend reply and the waiter installing its
    /// subscription cannot be missed.
    pub fn job_event_channel(&self, job_id: &str) -> broadcast::Receiver<Value> {
        let entry = self
            .inner
            .job_event_buses
            .entry(job_id.to_string())
            .or_insert_with(|| broadcast::channel::<Value>(32).0);
        entry.value().subscribe()
    }

    /// Drop the per-job broadcast bus. Outstanding receivers will see
    /// `RecvError::Closed` on their next `recv().await`; call this after
    /// the waiter has observed a terminal event (or timed out) so the
    /// map does not grow unboundedly across many async jobs.
    pub fn forget_job_bus(&self, job_id: &str) {
        self.inner.job_event_buses.remove(job_id);
    }

    /// Publish a parsed notification to the per-job bus, if any waiter
    /// is listening. Silently noops when nobody subscribed.
    fn publish_job_event(&self, job_id: &str, value: &Value) {
        if let Some(entry) = self.inner.job_event_buses.get(job_id) {
            let _ = entry.value().send(value.clone());
        }
    }

    /// Testing-only: hand-feed a `$/dcc.jobUpdated` notification to the
    /// per-job bus. Lets integration tests exercise the wait-for-
    /// terminal path without spinning up a real backend SSE stream.
    #[doc(hidden)]
    pub fn test_publish_job_event(&self, job_id: &str, value: Value) {
        self.publish_job_event(job_id, &value);
    }

    /// Testing-only: report how many receivers are currently attached
    /// to the per-job broadcast bus. Returns zero when the bus does
    /// not yet exist. Used by integration tests to synchronise the
    /// publish against the gateway's own subscription so the test
    /// isn't racing the backend round-trip under CI instrumentation.
    #[doc(hidden)]
    pub fn job_bus_receiver_count(&self, job_id: &str) -> usize {
        self.inner
            .job_event_buses
            .get(job_id)
            .map(|entry| entry.value().receiver_count())
            .unwrap_or(0)
    }

    // ── Backend lifecycle ──────────────────────────────────────────────

    /// Ensure a reconnecting SSE subscriber exists for `backend_url`.
    /// Idempotent — a second call is a cheap DashMap lookup.
    pub fn ensure_subscribed(&self, backend_url: &str) {
        if self.inner.backends.contains_key(backend_url) {
            return;
        }
        let shared = Arc::new(BackendShared::new(backend_url.to_string()));
        let mgr = self.clone();
        let url = backend_url.to_string();
        let shared_clone = shared.clone();
        let task = tokio::spawn(async move {
            mgr.run_backend_loop(url, shared_clone).await;
        });
        self.inner.backends.insert(
            backend_url.to_string(),
            BackendSubscriber {
                url: backend_url.to_string(),
                task: Some(task),
                shared,
            },
        );
    }

    // ── Route GC (#322) ────────────────────────────────────────────────

    /// Spawn a background task that periodically evicts stale
    /// [`JobRoute`]s older than `route_ttl`. Returns the `JoinHandle`
    /// so the gateway supervisor can cancel it on shutdown.
    pub fn spawn_route_gc(&self) -> JoinHandle<()> {
        let mgr = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(ROUTE_GC_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                mgr.run_route_gc_once();
            }
        })
    }

    /// One GC pass — exposed separately so tests can drive it
    /// synchronously without waiting a real interval.
    pub fn run_route_gc_once(&self) -> usize {
        let ttl = self.inner.route_ttl;
        if ttl.is_zero() {
            return 0;
        }
        let cutoff = Utc::now() - chrono::Duration::from_std(ttl).unwrap_or_default();
        let stale: Vec<String> = self
            .inner
            .job_routes
            .iter()
            .filter(|e| e.value().created_at < cutoff)
            .map(|e| e.key().clone())
            .collect();
        for jid in &stale {
            self.forget_job(jid);
        }
        stale.len()
    }

    // ── Introspection helpers (for tests) ──────────────────────────────

    #[cfg(test)]
    pub(crate) fn route_for_job(&self, job_id: &str) -> Option<String> {
        self.inner
            .job_routes
            .get(job_id)
            .map(|e| e.value().client_session_id.clone())
    }

    #[cfg(test)]
    pub(crate) fn route_for_progress_token(&self, token: &Value) -> Option<String> {
        self.inner
            .progress_token_routes
            .get(&progress_token_key(token))
            .map(|e| e.value().clone())
    }

    #[cfg(test)]
    pub(crate) fn pending_count(&self, backend_url: &str) -> usize {
        self.inner
            .backends
            .get(backend_url)
            .map(|b| b.shared.pending.lock().len())
            .unwrap_or(0)
    }

    // ── Delivery ───────────────────────────────────────────────────────

    /// Deliver an MCP notification JSON to the right client session, or
    /// buffer it if we cannot resolve the target yet.
    fn deliver(&self, value: Value, backend_shared: &BackendShared) {
        // #321: fan out `$/dcc.jobUpdated` / `workflowUpdated` onto any
        // per-job wait-for-terminal bus before we worry about SSE
        // routing. Publishing is independent of whether a client SSE
        // sink exists — a wait-for-terminal POST client may not have
        // any GET /mcp stream open at all.
        if let Some(jid) = job_id_for_job_notification(&value) {
            self.publish_job_event(&jid, &value);
        }
        // #322: auto-evict the JobRoute once a terminal status arrives,
        // so the cache doesn't grow with completed jobs.
        if let Some(jid) = terminal_job_id(&value) {
            self.forget_job(&jid);
        }
        let session = resolve_target(&self.inner, &value);
        match session {
            Some(sid) => {
                if let Some(sender) = self.inner.client_sinks.get(&sid) {
                    let event = format_sse_event(&value, None);
                    // receiver_count() == 0 is fine: push_event in
                    // SessionManager has the same semantics.
                    let _ = sender.send(event);
                } else {
                    tracing::debug!(
                        session = %sid,
                        backend = %backend_shared.url,
                        "gateway SSE: target session has no live sink — dropping"
                    );
                }
            }
            None => self.buffer_pending(backend_shared, value),
        }
    }

    fn buffer_pending(&self, shared: &BackendShared, value: Value) {
        let mut buf = shared.pending.lock();
        // Expire stale entries first.
        let now = Instant::now();
        while buf
            .front()
            .map(|p| now.duration_since(p.inserted_at) >= PENDING_BUFFER_TTL)
            .unwrap_or(false)
        {
            buf.pop_front();
        }
        if buf.len() >= PENDING_BUFFER_CAP {
            let dropped = buf.pop_front();
            tracing::warn!(
                backend = %shared.url,
                buffered = buf.len() + 1,
                dropped_method = %dropped
                    .as_ref()
                    .and_then(|p| p.value.get("method"))
                    .and_then(|m| m.as_str())
                    .unwrap_or(""),
                "gateway SSE pending buffer full — dropping oldest"
            );
        }
        buf.push_back(Pending {
            inserted_at: now,
            value,
        });
    }

    /// Re-scan the pending buffer after a new routing mapping appeared.
    fn flush_pending_for_backend(&self, backend_url: &str) {
        let Some(backend) = self.inner.backends.get(backend_url) else {
            return;
        };
        let shared = backend.shared.clone();
        drop(backend); // release DashMap shard lock before taking inner lock

        let drained: Vec<Pending> = {
            let mut buf = shared.pending.lock();
            let now = Instant::now();
            buf.retain(|p| now.duration_since(p.inserted_at) < PENDING_BUFFER_TTL);
            std::mem::take(&mut *buf).into_iter().collect()
        };
        for p in drained {
            let session = resolve_target(&self.inner, &p.value);
            match session {
                Some(sid) => {
                    if let Some(sender) = self.inner.client_sinks.get(&sid) {
                        let event = format_sse_event(&p.value, None);
                        let _ = sender.send(event);
                    }
                }
                None => {
                    // Still unresolved — re-queue.
                    shared.pending.lock().push_back(p);
                }
            }
        }
    }

    /// Fan-out a synthetic `$/dcc.gatewayReconnect` notification to every
    /// client that had an in-flight job on `backend_url`.
    fn emit_gateway_reconnect(&self, backend_url: &str) {
        let Some(sessions) = self.inner.backend_inflight.get(backend_url) else {
            return;
        };
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.gatewayReconnect",
            "params": {
                "backend_url": backend_url,
            },
        });
        let event = format_sse_event(&notification, None);
        for sid in sessions.iter() {
            if let Some(sender) = self.inner.client_sinks.get(sid.key()) {
                let _ = sender.send(event.clone());
            }
        }
    }

    // ── Backend reconnect loop ─────────────────────────────────────────

    async fn run_backend_loop(self, url: String, shared: Arc<BackendShared>) {
        let mut attempt: u32 = 0;
        loop {
            match self.open_stream(&url).await {
                Ok(resp) => {
                    if attempt > 0 {
                        tracing::info!(
                            backend = %url,
                            attempts = attempt,
                            "gateway SSE: backend reconnected — emitting gatewayReconnect"
                        );
                        self.emit_gateway_reconnect(&url);
                    }
                    attempt = 0;
                    *shared.reconnect_attempts.lock() = 0;
                    // Pump until the stream closes / errors out.
                    self.pump_stream(resp, &shared).await;
                    tracing::info!(backend = %url, "gateway SSE: stream closed — reconnecting");
                }
                Err(e) => {
                    tracing::debug!(
                        backend = %url,
                        attempt,
                        error = %e,
                        "gateway SSE: connect failed"
                    );
                }
            }
            attempt = attempt.saturating_add(1);
            *shared.reconnect_attempts.lock() = attempt;
            let delay = backoff_delay(attempt);
            tokio::time::sleep(delay).await;
        }
    }

    async fn open_stream(&self, url: &str) -> reqwest::Result<reqwest::Response> {
        self.inner
            .http_client
            .get(url)
            .timeout(CONNECT_TIMEOUT)
            .header("accept", "text/event-stream")
            .send()
            .await
            .and_then(|r| r.error_for_status())
    }

    async fn pump_stream(&self, resp: reqwest::Response, shared: &BackendShared) {
        let mut stream = resp.bytes_stream();
        let mut scratch: Vec<u8> = Vec::with_capacity(4096);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!(backend = %shared.url, error = %e, "gateway SSE: stream error");
                    break;
                }
            };
            scratch.extend_from_slice(&bytes);
            // SSE records terminate with "\n\n"; drain complete records
            // from the head of the scratch buffer.
            while let Some(pos) = find_record_end(&scratch) {
                let record = scratch.drain(..pos).collect::<Vec<u8>>();
                // Discard the trailing delimiter.
                let _ = scratch.drain(..record_delim_len(&scratch));
                if let Some(value) = parse_sse_record(&record) {
                    self.deliver(value, shared);
                }
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Serialise a `progressToken` (may be number or string) into a stable
/// map key.
pub(crate) fn progress_token_key(token: &Value) -> String {
    match token {
        Value::String(s) => format!("s:{s}"),
        Value::Number(n) => format!("n:{n}"),
        other => format!("j:{other}"),
    }
}

/// Exponential backoff with ±25 % jitter.
pub(crate) fn backoff_delay(attempt: u32) -> Duration {
    let base = RECONNECT_INITIAL.as_millis() as u64;
    // doubling, capped.
    let shift = attempt.saturating_sub(1).min(12); // 2^12 headroom
    let mut delay_ms = base.saturating_mul(1u64 << shift);
    let cap = RECONNECT_MAX.as_millis() as u64;
    if delay_ms > cap {
        delay_ms = cap;
    }
    // Pseudo-random jitter derived from attempt & current nanos so
    // multiple backends don't synchronise retries.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let entropy = (nanos.rotate_left(attempt % 64)) % 1024;
    let jitter_span = (delay_ms as f32 * RECONNECT_JITTER) as i64;
    let jitter = if jitter_span > 0 {
        (entropy as i64 % (jitter_span * 2 + 1)) - jitter_span
    } else {
        0
    };
    let final_ms = (delay_ms as i64).saturating_add(jitter).max(0) as u64;
    Duration::from_millis(final_ms)
}

/// Return the byte offset of the end of the next complete SSE record
/// (double-newline) in `buf`, relative to the buffer start.
fn find_record_end(buf: &[u8]) -> Option<usize> {
    // Accept both "\n\n" and "\r\n\r\n" as record terminators.
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        if i + 3 < buf.len() && &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

/// Length of the terminator `find_record_end` matched. Called after a
/// successful `find_record_end` returns `Some`.
fn record_delim_len(buf: &[u8]) -> usize {
    if buf.starts_with(b"\r\n\r\n") {
        4
    } else {
        // Standard case: the drained record already took everything up
        // to the first delimiter byte. What's left in the buffer starts
        // with the terminator itself.
        if buf.starts_with(b"\n\n") { 2 } else { 1 }
    }
}

/// Parse a single SSE record (without trailing blank line) into a JSON
/// value, returning `None` if the record has no `data:` field or the
/// payload is not valid JSON.
fn parse_sse_record(record: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(record).ok()?;
    let mut data_lines: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            // MDN / WHATWG: the single leading space is optional.
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let joined = data_lines.join("\n");
    serde_json::from_str::<Value>(&joined).ok()
}

/// Extract the `job_id` from a `$/dcc.jobUpdated` / `workflowUpdated`
/// notification envelope. Used by the per-job broadcast bus (#321) so
/// wait-for-terminal POST handlers can block on terminal events without
/// needing their own SSE subscription.
fn job_id_for_job_notification(value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if !matches!(
        method,
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated"
    ) {
        return None;
    }
    value
        .get("params")
        .and_then(|p| p.get("job_id"))
        .and_then(|j| j.as_str())
        .map(str::to_owned)
}

/// Extract the `job_id` from a `$/dcc.jobUpdated` / `workflowUpdated`
/// notification only when it carries a terminal status (issue #322
/// auto-eviction). Terminal statuses follow #318: `completed`,
/// `failed`, `cancelled`, `interrupted`.
fn terminal_job_id(value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if !matches!(
        method,
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated"
    ) {
        return None;
    }
    let params = value.get("params")?;
    let status = params.get("status").and_then(|s| s.as_str())?;
    if !matches!(status, "completed" | "failed" | "cancelled" | "interrupted") {
        return None;
    }
    params
        .get("job_id")
        .and_then(|j| j.as_str())
        .map(str::to_owned)
}

/// Determine which client session should receive `value`.
fn resolve_target(inner: &SubscriberManagerInner, value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    let params = value.get("params");

    match method {
        "notifications/progress" => {
            let token = params.and_then(|p| p.get("progressToken"))?;
            inner
                .progress_token_routes
                .get(&progress_token_key(token))
                .map(|e| e.value().clone())
        }
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated" => {
            let job_id = params
                .and_then(|p| p.get("job_id"))
                .and_then(|j| j.as_str())?;
            inner
                .job_routes
                .get(job_id)
                .map(|e| e.value().client_session_id.clone())
        }
        _ => None,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_starts_near_initial() {
        let d = backoff_delay(1);
        // first attempt — base 100 ms ± 25 %
        assert!(d >= Duration::from_millis(75));
        assert!(d <= Duration::from_millis(125));
    }

    #[test]
    fn backoff_delay_grows_exponentially_and_caps() {
        // large attempt — must not exceed RECONNECT_MAX + 25 %
        let cap_with_jitter = (RECONNECT_MAX.as_millis() as f32 * 1.25) as u128;
        for attempt in 1..30u32 {
            let d = backoff_delay(attempt).as_millis();
            assert!(
                d <= cap_with_jitter,
                "attempt={attempt} delay={d}ms exceeds cap {cap_with_jitter}ms"
            );
        }
        // At attempt=20 we are definitely saturated near the cap.
        let d = backoff_delay(20).as_millis();
        let floor = (RECONNECT_MAX.as_millis() as f32 * 0.75) as u128;
        assert!(d >= floor, "saturated backoff={d}ms below floor {floor}ms");
    }

    #[test]
    fn progress_token_key_distinguishes_string_and_number_tokens() {
        let s = progress_token_key(&Value::String("abc".into()));
        let n = progress_token_key(&serde_json::json!(42));
        let n_str = progress_token_key(&Value::String("42".into()));
        assert_ne!(s, n);
        assert_ne!(n, n_str);
    }

    #[test]
    fn parse_sse_record_extracts_json_from_data_field() {
        let rec = b"data: {\"method\":\"notifications/progress\",\"params\":{\"progress\":1}}";
        let v = parse_sse_record(rec).expect("valid record");
        assert_eq!(v["method"], "notifications/progress");
    }

    #[test]
    fn parse_sse_record_handles_multiline_data_and_id_field() {
        // Two `data:` lines must be concatenated with '\n' per
        // WHATWG SSE spec. We check both that the parse does not
        // panic on a multi-line record and that non-data lines
        // (`id:`, `event:`) are skipped.
        let rec = b"id: 7\nevent: message\ndata: {\"a\":1,\ndata: \"b\":2}";
        let v = parse_sse_record(rec).expect("multi-line data: joins cleanly");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn parse_sse_record_returns_none_for_record_without_data_field() {
        let rec = b"event: endpoint\n";
        assert!(parse_sse_record(rec).is_none());
    }

    fn empty_inner() -> SubscriberManagerInner {
        SubscriberManagerInner {
            backends: DashMap::new(),
            job_routes: DashMap::new(),
            session_jobs: DashMap::new(),
            request_to_job: DashMap::new(),
            progress_token_routes: DashMap::new(),
            backend_inflight: DashMap::new(),
            client_sinks: DashMap::new(),
            job_event_buses: DashMap::new(),
            http_client: reqwest::Client::new(),
            route_ttl: DEFAULT_ROUTE_TTL,
            max_routes_per_session: DEFAULT_MAX_ROUTES_PER_SESSION,
        }
    }

    fn test_route(sid: &str) -> JobRoute {
        JobRoute {
            client_session_id: sid.to_string(),
            backend_id: "http://backend/mcp".into(),
            tool: "test_tool".into(),
            created_at: Utc::now(),
            parent_job_id: None,
        }
    }

    #[test]
    fn resolve_target_prefers_progress_token_for_progress_notifications() {
        let inner = empty_inner();
        inner.progress_token_routes.insert(
            progress_token_key(&Value::String("tok".into())),
            "sessA".into(),
        );
        let note = serde_json::json!({
            "method": "notifications/progress",
            "params": {"progressToken": "tok", "progress": 5, "total": 10}
        });
        assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessA"));
    }

    #[test]
    fn resolve_target_uses_job_id_for_job_updated() {
        let inner = empty_inner();
        inner
            .job_routes
            .insert("jid-42".into(), test_route("sessB"));
        let note = serde_json::json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "jid-42", "status": "running"}
        });
        assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessB"));
    }

    #[test]
    fn resolve_target_returns_none_when_unknown() {
        let inner = empty_inner();
        let note = serde_json::json!({
            "method": "notifications/progress",
            "params": {"progressToken": "no-such-token"}
        });
        assert!(resolve_target(&inner, &note).is_none());
    }

    // #321: per-job broadcast delivery — unit tests here, end-to-end
    // wiring is covered by `gateway/tests.rs`.

    #[tokio::test]
    async fn job_event_channel_receives_published_notifications() {
        let mgr = SubscriberManager::default();
        let mut rx = mgr.job_event_channel("job-1");
        let note = serde_json::json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "job-1", "status": "completed"}
        });
        mgr.publish_job_event("job-1", &note);
        let delivered = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("recv did not time out")
            .expect("bus delivered");
        assert_eq!(delivered["params"]["status"], "completed");
    }

    #[tokio::test]
    async fn job_event_channel_publishes_only_to_requested_job() {
        let mgr = SubscriberManager::default();
        let mut rx_a = mgr.job_event_channel("job-a");
        let mut rx_b = mgr.job_event_channel("job-b");
        let note = serde_json::json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "job-a", "status": "running"}
        });
        mgr.publish_job_event("job-a", &note);
        assert!(rx_a.try_recv().is_ok());
        assert!(rx_b.try_recv().is_err());
    }

    #[tokio::test]
    async fn deliver_publishes_to_job_event_bus_even_without_route() {
        // The waiter path does NOT require `bind_job` — it subscribes to
        // the per-job bus directly before the reply arrives. `deliver`
        // must therefore publish to the bus regardless of whether a
        // client-session route exists.
        let mgr = SubscriberManager::default();
        let mut rx = mgr.job_event_channel("job-x");
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        let note = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "job-x", "status": "completed"}
        });
        mgr.deliver(note, &shared);
        let delivered = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("recv did not time out")
            .expect("bus delivered");
        assert_eq!(delivered["params"]["status"], "completed");
    }

    #[test]
    fn forget_job_bus_removes_the_broadcast() {
        let mgr = SubscriberManager::default();
        let _rx = mgr.job_event_channel("job-1");
        assert!(mgr.inner.job_event_buses.contains_key("job-1"));
        mgr.forget_job_bus("job-1");
        assert!(!mgr.inner.job_event_buses.contains_key("job-1"));
    }

    #[tokio::test]
    async fn manager_buffers_then_flushes_after_job_binding() {
        // Stand up a manager, register a client, hand-feed a notification
        // whose job_id mapping is not yet known, then bind the mapping
        // and assert the buffered event is delivered.
        let mgr = SubscriberManager::default();
        let mut rx = mgr.register_client("sess1");
        let backend = "http://127.0.0.1:0/mcp".to_string();
        // Fake a backend entry so buffer operations resolve.
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );

        let note = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "job-1", "status": "running"}
        });
        mgr.deliver(note.clone(), &shared);
        assert_eq!(mgr.pending_count(&backend), 1, "buffered while unresolved");
        assert!(rx.try_recv().is_err(), "nothing delivered yet");

        mgr.bind_job("job-1", "sess1", &backend);
        // After bind, the flush is triggered synchronously.
        assert_eq!(mgr.pending_count(&backend), 0, "buffer drained");
        let event = rx
            .try_recv()
            .expect("event should have been flushed to sink");
        assert!(event.contains("notifications/$/dcc.jobUpdated"));
    }

    #[tokio::test]
    async fn manager_emits_gateway_reconnect_to_inflight_sessions() {
        let mgr = SubscriberManager::default();
        let mut rx = mgr.register_client("sess1");
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared,
            },
        );
        mgr.bind_job("job-x", "sess1", &backend);

        mgr.emit_gateway_reconnect(&backend);

        let event = rx.try_recv().expect("gatewayReconnect should be delivered");
        assert!(event.contains("notifications/$/dcc.gatewayReconnect"));
        assert!(event.contains(&backend));
    }

    #[tokio::test]
    async fn manager_drops_events_for_forgotten_client() {
        let mgr = SubscriberManager::default();
        let _rx = mgr.register_client("sess1");
        mgr.forget_client("sess1");

        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );
        mgr.bind_job("job-gone", "sess1", &backend);
        let note = serde_json::json!({
            "jsonrpc":"2.0",
            "method":"notifications/$/dcc.jobUpdated",
            "params":{"job_id":"job-gone","status":"running"}
        });
        // Must not panic; simply drops.
        mgr.deliver(note, &shared);
    }

    #[test]
    fn pending_buffer_evicts_oldest_when_full() {
        let mgr = SubscriberManager::default();
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );
        for i in 0..(PENDING_BUFFER_CAP + 5) {
            let note = serde_json::json!({
                "method":"notifications/$/dcc.jobUpdated",
                "params":{"job_id": format!("j{i}"), "status":"running"}
            });
            mgr.deliver(note, &shared);
        }
        assert_eq!(
            mgr.pending_count(&backend),
            PENDING_BUFFER_CAP,
            "buffer is bounded"
        );
    }

    // ── #322 JobRoute store ─────────────────────────────────────────────

    #[test]
    fn bind_job_route_populates_all_fields() {
        let mgr = SubscriberManager::default();
        mgr.bind_job_route("j1", "sessA", "http://back/mcp", "my_tool", Some("parent"))
            .unwrap();
        let route = mgr.job_route("j1").expect("route present");
        assert_eq!(route.client_session_id, "sessA");
        assert_eq!(route.backend_id, "http://back/mcp");
        assert_eq!(route.tool, "my_tool");
        assert_eq!(route.parent_job_id.as_deref(), Some("parent"));
    }

    #[test]
    fn bind_request_to_job_resolves_back_to_route() {
        let mgr = SubscriberManager::default();
        mgr.bind_job_route("j1", "sessA", "http://back/mcp", "t", None)
            .unwrap();
        mgr.bind_request_to_job("\"req-7\"", "j1");
        let jid = mgr.job_id_for_request("\"req-7\"").expect("mapping");
        assert_eq!(jid, "j1");
        let route = mgr.job_route(&jid).unwrap();
        assert_eq!(route.backend_id, "http://back/mcp");
    }

    #[test]
    fn children_of_returns_every_child_of_parent() {
        let mgr = SubscriberManager::default();
        mgr.bind_job_route("c1", "s", "http://a/mcp", "t", Some("P"))
            .unwrap();
        mgr.bind_job_route("c2", "s", "http://b/mcp", "t", Some("P"))
            .unwrap();
        mgr.bind_job_route("other", "s", "http://c/mcp", "t", Some("Q"))
            .unwrap();
        let mut kids: Vec<String> = mgr.children_of("P").into_iter().map(|(j, _)| j).collect();
        kids.sort();
        assert_eq!(kids, vec!["c1".to_string(), "c2".to_string()]);
    }

    #[test]
    fn per_session_cap_rejects_overflow() {
        let mgr =
            SubscriberManager::with_limits(reqwest::Client::new(), Duration::from_secs(60), 2);
        assert!(
            mgr.bind_job_route("j1", "sess", "http://b/mcp", "t", None)
                .is_ok()
        );
        assert!(
            mgr.bind_job_route("j2", "sess", "http://b/mcp", "t", None)
                .is_ok()
        );
        let err = mgr
            .bind_job_route("j3", "sess", "http://b/mcp", "t", None)
            .expect_err("cap should reject");
        matches!(err, BindJobError::TooManyInFlight { .. });
    }

    #[test]
    fn terminal_status_auto_evicts_route() {
        let mgr = SubscriberManager::default();
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.bind_job_route("jT", "sess", &backend, "t", None)
            .unwrap();
        assert!(mgr.job_route("jT").is_some());
        let note = serde_json::json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "jT", "status": "completed"}
        });
        mgr.deliver(note, &shared);
        assert!(
            mgr.job_route("jT").is_none(),
            "route should be auto-evicted on completion"
        );
    }

    #[test]
    fn run_route_gc_once_evicts_stale_routes() {
        // TTL=0 disables GC (per spec); use 1 ms so routes older than
        // 1 ms are stale.
        let mgr =
            SubscriberManager::with_limits(reqwest::Client::new(), Duration::from_millis(1), 0);
        mgr.bind_job_route("old", "s", "http://b/mcp", "t", None)
            .unwrap();
        // Force the created_at far into the past.
        if let Some(mut e) = mgr.inner.job_routes.get_mut("old") {
            e.value_mut().created_at = Utc::now() - chrono::Duration::seconds(10);
        }
        let evicted = mgr.run_route_gc_once();
        assert_eq!(evicted, 1);
        assert!(mgr.job_route("old").is_none());
    }

    #[test]
    fn forget_job_cleans_up_reverse_indexes() {
        let mgr = SubscriberManager::default();
        mgr.bind_job_route("j1", "sess", "http://b/mcp", "t", None)
            .unwrap();
        mgr.bind_request_to_job("\"rid\"", "j1");
        assert!(mgr.job_route("j1").is_some());
        mgr.forget_job("j1");
        assert!(mgr.job_route("j1").is_none());
        assert!(mgr.job_id_for_request("\"rid\"").is_none());
        assert!(
            mgr.inner
                .session_jobs
                .get("sess")
                .is_none_or(|s| !s.contains("j1"))
        );
    }
}
