use std::time::Duration;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};
use dcc_mcp_transport::error::TransportResult;
use futures::future::join_all;

fn evict_probe_snapshot(
    registry: &FileRegistry,
    observed: &ServiceEntry,
) -> TransportResult<Option<ServiceEntry>> {
    registry.deregister_if_unchanged(observed)
}

/// On gateway startup, probe every registered instance's TCP port and
/// deregister any that are unreachable. Complements `prune_dead_pids`
/// which only checks PID liveness — a process may be alive but its MCP
/// listener already shut down (issue #556).
///
/// Probes run **in parallel** so many DCC instances do not stretch gateway
/// startup by `N × timeout` (sequential behaviour made 4+ instances flaky on
/// busy hosts where each connect approached the deadline).
pub(crate) async fn probe_and_evict_dead_instances(
    registry: &FileRegistry,
    stale_timeout: Duration,
    own_host: &str,
    own_port: u16,
) -> TransportResult<Vec<ServiceEntry>> {
    let entries: Vec<_> = registry
        .list_all()
        .into_iter()
        .filter(|e| {
            e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                && e.port != 0
                && !e.is_stale(stale_timeout)
                && !crate::gateway::is_own_instance(e, own_host, own_port)
        })
        .collect();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    /// Tuned for many instances: accept completes quickly on healthy listeners;
    /// unhealthy rows fail fast without holding the gateway boot for N×3s.
    const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);

    let futures: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let addr = format!("{}:{}", entry.host, entry.port);
            let dcc_type = entry.dcc_type.clone();
            let instance_id = entry.instance_id;
            async move {
                let reachable = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    tokio::net::TcpStream::connect(addr.as_str()),
                )
                .await
                .is_ok_and(|r| r.is_ok());
                (entry, reachable, addr, dcc_type, instance_id)
            }
        })
        .collect();

    let outcomes = join_all(futures).await;
    let mut evicted = Vec::new();
    for (observed, reachable, addr, dcc_type, instance_id) in outcomes {
        if !reachable {
            let removed = evict_probe_snapshot(registry, &observed)?;
            if let Some(entry) = removed {
                tracing::info!(
                    dcc_type = %dcc_type,
                    instance_id = %instance_id,
                    addr = %addr,
                    "Startup probe: instance unreachable — deregistered"
                );
                evicted.push(entry);
            } else {
                tracing::debug!(
                    dcc_type = %dcc_type,
                    instance_id = %instance_id,
                    addr = %addr,
                    "Startup probe result was stale — keeping refreshed instance"
                );
            }
        }
    }
    Ok(evicted)
}

/// Verify that the gateway accept-loop is actually running by connecting to it.
///
/// Retries a small number of times with short back-off to give the Tokio
/// runtime a chance to schedule the `axum::serve` task — necessary under
/// PyO3-embedded hosts where workers are slow to pick up newly spawned tasks
/// (issue #303).
pub(crate) async fn self_probe_listener(addr: std::net::SocketAddr) -> Result<(), std::io::Error> {
    let addr = probe_addr(addr);
    const MAX_ATTEMPTS: u32 = 10;
    const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(200);
    const BACKOFF: Duration = Duration::from_millis(100);

    let mut last_err: Option<std::io::Error> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe succeeded");
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, attempt, error = %e, "Gateway self-probe: connect error");
                last_err = Some(e);
            }
            Err(_) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe: connect timed out");
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "self-probe connect timed out",
                ));
            }
        }
        tokio::time::sleep(BACKOFF).await;
    }

    Err(last_err.unwrap_or_else(|| std::io::Error::other("self-probe failed with no error")))
}

fn probe_addr(addr: std::net::SocketAddr) -> std::net::SocketAddr {
    if !addr.ip().is_unspecified() {
        return addr;
    }
    match addr {
        std::net::SocketAddr::V4(addr) => std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            addr.port(),
        ),
        std::net::SocketAddr::V6(addr) => std::net::SocketAddr::new(
            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            addr.port(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceEntry;
    use tempfile::tempdir;

    #[tokio::test]
    async fn startup_probe_skips_port_zero_rows() {
        let dir = tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let entry = ServiceEntry::new("3dsmax", "127.0.0.1", 0);
        let key = entry.key();
        registry.register(entry).unwrap();

        let evicted =
            probe_and_evict_dead_instances(&registry, Duration::from_secs(30), "127.0.0.1", 9765)
                .await
                .unwrap();

        assert!(evicted.is_empty());
        assert!(
            registry.get(&key).is_some(),
            "port=0 sidecar rows are booting diagnostics, not startup-probe evictions"
        );
    }

    #[tokio::test]
    async fn startup_probe_returns_evicted_rows_for_persistence() {
        let dir = tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let entry = ServiceEntry::new("maya", "127.0.0.1", port);
        let instance_id = entry.instance_id;
        let key = entry.key();
        registry.register(entry).unwrap();

        let evicted =
            probe_and_evict_dead_instances(&registry, Duration::from_secs(30), "127.0.0.1", 9765)
                .await
                .unwrap();

        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].instance_id, instance_id);
        assert!(
            registry.get(&key).is_none(),
            "startup probe must remove unreachable rows from the live registry"
        );
    }

    #[test]
    fn stale_probe_does_not_remove_refreshed_row() {
        let dir = tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let entry = ServiceEntry::new("houdini", "127.0.0.1", 18814);
        let key = entry.key();
        registry.register(entry).unwrap();

        let observed = registry.get(&key).unwrap();
        std::thread::sleep(Duration::from_millis(1));
        assert!(registry.heartbeat(&key).unwrap());

        let removed = evict_probe_snapshot(&registry, &observed).unwrap();

        assert!(removed.is_none());
        assert!(registry.get(&key).is_some());
    }
}
