use super::backend::{BackendShared, BackendSubscriber};
use super::helpers::progress_token_key;
use super::*;

#[derive(Clone)]
pub struct SubscriberManager {
    pub(crate) inner: Arc<SubscriberManagerInner>,
}

pub(crate) struct SubscriberManagerInner {
    // Backend subscription store.
    pub(crate) backends: DashMap<String, BackendSubscriber>,

    // Job and progress routing stores.
    /// `job_id` → full [`JobRoute`] (issue #322).
    ///
    /// Before v0.15 this was `DashMap<String, ClientSessionId>`; callers
    /// that only need the session id now read `route.client_session_id`.
    pub(crate) job_routes: DashMap<String, JobRoute>,
    /// Reverse index: `client_session_id` → set of live `job_id`s. Used
    /// to enforce the per-session cap without walking the whole
    /// `job_routes` map on every insert.
    pub(crate) session_jobs: DashMap<ClientSessionId, DashSet<String>>,
    /// Reverse index: gateway JSON-RPC `requestId` (stringified) →
    /// dispatched `job_id`. Populated at dispatch time for async jobs
    /// so `notifications/cancelled { requestId }` can resolve to a
    /// `JobRoute` even after the original RPC has already returned
    /// (issue #322).
    pub(crate) request_to_job: DashMap<String, String>,
    /// `progressToken` (serialised JSON) → owning client session.
    pub(crate) progress_token_routes: DashMap<String, ClientSessionId>,
    /// Backend URL → set of client sessions with in-flight jobs on that
    /// backend. Used for `$/dcc.gatewayReconnect` fan-out.
    pub(crate) backend_inflight: DashMap<String, DashSet<ClientSessionId>>,

    // Client delivery stores.
    /// Client session → broadcast::Sender used by the GET /mcp handler.
    pub(crate) client_sinks: DashMap<ClientSessionId, broadcast::Sender<String>>,
    /// Per-`job_id` broadcast of parsed `$/dcc.jobUpdated` / `workflowUpdated`
    /// JSON-RPC notifications (#321 wait-for-terminal passthrough).
    ///
    /// The bus is created lazily by [`SubscriberManager::job_event_channel`]
    /// — typically called from the gateway aggregator just before
    /// forwarding an async `tools/call` so the waiter cannot miss a
    /// terminal event that arrives while the POST reply is in flight.
    pub(crate) job_event_buses: DashMap<String, broadcast::Sender<Value>>,

    // Resource subscription store.
    /// Resource-subscription routes (issue #732).
    ///
    /// Key: `(backend_url, backend_uri)` — uniquely identifies a
    /// backend resource. Value: set of `ResourceSubscriberRoute`s
    /// that must receive every `notifications/resources/updated`
    /// frame for that resource, each carrying the client-visible
    /// prefixed URI so the gateway can rewrite the notification's
    /// `params.uri` before forwarding.
    pub(crate) resource_subscriptions: DashMap<(String, String), DashSet<ResourceSubscriberRoute>>,
    /// Shared HTTP client with connection pooling.
    pub(crate) http_client: reqwest::Client,
    /// TTL beyond which a non-terminal route is evicted by the GC task
    /// (issue #322).
    pub(crate) route_ttl: Duration,
    /// Per-session ceiling on concurrent live routes (issue #322). `0`
    /// disables the cap.
    pub(crate) max_routes_per_session: usize,
}

impl SubscriberManagerInner {
    fn forget_client_job_routes(&self, session_id: &str) -> Vec<String> {
        let mut dropped_jobs: Vec<String> = Vec::new();
        self.job_routes.retain(|job_id, route| {
            let keep = route.client_session_id.as_str() != session_id;
            if !keep {
                dropped_jobs.push(job_id.clone());
            }
            keep
        });
        for jid in &dropped_jobs {
            self.job_event_buses.remove(jid);
        }
        self.session_jobs.remove(session_id);
        dropped_jobs
    }

    fn forget_client_reverse_indexes(&self, session_id: &str, dropped_jobs: &[String]) {
        self.request_to_job
            .retain(|_, jid| !dropped_jobs.iter().any(|d| d == jid));
        self.progress_token_routes
            .retain(|_, sid| sid.as_str() != session_id);
    }

    fn forget_client_backend_inflight(&self, session_id: &str) {
        for entry in self.backend_inflight.iter() {
            entry.value().remove(session_id);
        }
    }
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
                resource_subscriptions: DashMap::new(),
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
        self.inner.forget_client_backend_inflight(session_id);
        let dropped_jobs = self.inner.forget_client_job_routes(session_id);
        self.inner
            .forget_client_reverse_indexes(session_id, &dropped_jobs);
        // #732: drop any resource subscriptions owned by this session.
        // We deliberately do NOT forward `resources/unsubscribe` to the
        // backend here — the session's broken SSE stream already means
        // the backend cannot reach this client, and other sessions on
        // the same gateway may still be subscribed to the same resource.
        let _ = self.forget_client_resource_subs(session_id);
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
        if let Some((_, route)) = removed
            && let Some(set) = self.inner.session_jobs.get(&route.client_session_id)
        {
            set.value().remove(job_id);
        }
        self.inner
            .request_to_job
            .retain(|_, jid| jid.as_str() != job_id);
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

    /// Return the stable `Mcp-Session-Id` the gateway uses for every
    /// request to `backend_url` — both the long-lived SSE GET and any
    /// forwarded JSON-RPC POST that must land on the same backend
    /// session (e.g. `resources/subscribe` / `resources/unsubscribe`
    /// under #732).
    ///
    /// Returns `None` when no subscriber exists for that URL yet, OR
    /// when the subscriber has started but has not yet completed its
    /// `initialize` handshake with the backend. Callers that hit this
    /// path can retry after a short delay; in practice the handshake
    /// completes within milliseconds of `ensure_subscribed`.
    pub fn backend_session_id(&self, backend_url: &str) -> Option<String> {
        self.inner
            .backends
            .get(backend_url)
            .and_then(|entry| entry.shared.session_id.lock().clone())
    }

    /// Wait up to `budget` for the backend SSE subscriber to complete
    /// its `initialize` handshake and publish a session id. Returns
    /// `None` when either the subscriber does not exist or the
    /// handshake did not land before the deadline.
    pub async fn wait_for_backend_session_id(
        &self,
        backend_url: &str,
        budget: std::time::Duration,
    ) -> Option<String> {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if let Some(id) = self.backend_session_id(backend_url) {
                return Some(id);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    /// Abort and remove any subscriber whose URL is **not** in `live_urls`.
    ///
    /// Called after each `FileRegistry` scan so that dead backends (ports that
    /// have gone away or become stale) stop burning reconnect-loop cycles and
    /// log spam. Issue #402.
    pub fn prune_unlisted(&self, live_urls: &[String]) {
        let dead: Vec<String> = self
            .inner
            .backends
            .iter()
            .filter_map(|e| {
                if !live_urls.contains(e.key()) {
                    Some(e.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for url in &dead {
            if let Some((_, mut sub)) = self.inner.backends.remove(url) {
                sub.abort();
                tracing::debug!(
                    backend = %url,
                    "gateway SSE: pruned subscriber for dead/stale backend"
                );
            }
        }
        if !dead.is_empty() {
            tracing::info!(
                pruned = dead.len(),
                "gateway SSE: removed {} dead backend subscriber(s)",
                dead.len()
            );
        }
    }

    // ── Introspection helpers (for tests) ──────────────────────────────

    #[cfg(test)]
    pub(crate) fn pending_count(&self, backend_url: &str) -> usize {
        self.inner
            .backends
            .get(backend_url)
            .map(|b| b.shared.pending.lock().len())
            .unwrap_or(0)
    }
}
