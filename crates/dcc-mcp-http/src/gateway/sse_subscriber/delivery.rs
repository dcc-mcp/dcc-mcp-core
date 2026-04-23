use super::*;

impl SubscriberManager {
    /// Deliver an MCP notification JSON to the right client session, or
    /// buffer it if we cannot resolve the target yet.
    pub(super) fn deliver(&self, value: Value, backend_shared: &BackendShared) {
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

    pub(super) fn buffer_pending(&self, shared: &BackendShared, value: Value) {
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
    pub(super) fn flush_pending_for_backend(&self, backend_url: &str) {
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
    pub(super) fn emit_gateway_reconnect(&self, backend_url: &str) {
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
}
