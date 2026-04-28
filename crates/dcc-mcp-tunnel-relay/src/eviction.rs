//! Periodic stale-tunnel sweeper.
//!
//! Runs once per `sweep_interval`, walks the [`TunnelRegistry`], and
//! removes any entry whose `last_heartbeat` is older than
//! `RelayConfig::stale_timeout`. Removal drops the registry's
//! `Arc<TunnelHandle>` and (transitively) the `frame_tx` clone, which
//! closes the agent writer's queue and tears down the per-tunnel tasks.
//!
//! The sweeper never reaches into the agent socket directly — that's the
//! control-plane's job. This keeps eviction lock-free at the per-tunnel
//! level and avoids needing a writer-side cancellation token.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::info;

use crate::config::RelayConfig;
use crate::registry::TunnelRegistry;

/// Spawn a periodic sweeper. Returns a [`tokio::task::JoinHandle`] so the
/// caller can shut it down on relay-wide teardown.
pub fn spawn_eviction_loop(
    registry: Arc<TunnelRegistry>,
    config: Arc<RelayConfig>,
    sweep_interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(sweep_interval);
        // The first tick fires immediately; skip it so the first sweep runs
        // one full interval after start (avoids a flurry of evictions in
        // tests that spin up the relay then race the assertion).
        ticker.tick().await;
        loop {
            ticker.tick().await;
            sweep_once(&registry, &config);
        }
    })
}

/// One sweep pass — exposed for tests so they can drive the eviction
/// without waiting on a real timer.
pub fn sweep_once(registry: &TunnelRegistry, config: &RelayConfig) {
    let cutoff = Instant::now().saturating_duration_since(Instant::now()) + config.stale_timeout; // satisfies clippy::useless_conversion
    let now = Instant::now();
    let stale: Vec<String> = registry
        .iter()
        .filter_map(|e| {
            let age = now.saturating_duration_since(e.value().last_seen());
            if age > config.stale_timeout {
                Some(e.key().clone())
            } else {
                None
            }
        })
        .collect();
    for id in stale {
        if let Some(entry) = registry.remove(&id) {
            info!(tunnel_id = %id, age_ms = ?cutoff.as_millis(), "evicting stale tunnel");
            drop(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::Arc;

    use crate::handle::TunnelHandle;
    use crate::registry::TunnelEntry;

    fn entry(id: &str, age: Duration) -> TunnelEntry {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        TunnelEntry {
            tunnel_id: id.into(),
            dcc: "test".into(),
            capabilities: vec![],
            agent_version: "test/0.0".into(),
            registered_at: Instant::now(),
            last_heartbeat: RwLock::new(
                Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
            ),
            handle: Arc::new(TunnelHandle::new(tx)),
        }
    }

    #[test]
    fn evicts_only_tunnels_past_timeout() {
        let reg = TunnelRegistry::new();
        reg.insert(entry("fresh", Duration::from_secs(1)));
        reg.insert(entry("stale", Duration::from_secs(120)));
        let cfg = RelayConfig {
            stale_timeout: Duration::from_secs(30),
            ..RelayConfig::default()
        };
        sweep_once(&reg, &cfg);
        assert!(reg.get(&"fresh".to_string()).is_some());
        assert!(reg.get(&"stale".to_string()).is_none());
    }
}
