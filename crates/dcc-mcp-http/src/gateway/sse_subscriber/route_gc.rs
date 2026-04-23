use super::*;

impl SubscriberManager {
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
}
