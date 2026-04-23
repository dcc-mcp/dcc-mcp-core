use super::*;

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
