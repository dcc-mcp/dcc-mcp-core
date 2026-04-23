use super::*;

impl SubscriberManager {
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
    pub(super) fn publish_job_event(&self, job_id: &str, value: &Value) {
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
}
