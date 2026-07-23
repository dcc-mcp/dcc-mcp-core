use std::time::Duration;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};
use dcc_mcp_transport::error::TransportResult;
use futures::future::join_all;

/// On gateway startup, probe every registered instance's TCP port and mark
/// unreachable rows without deleting live-owned registrations.
///
/// Probes run **in parallel** so many DCC instances do not stretch gateway
/// startup by `N × timeout` (sequential behaviour made 4+ instances flaky on
/// busy hosts where each connect approached the deadline).
pub(crate) async fn probe_and_mark_unreachable_instances(
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
    let mut marked = Vec::new();
    for (observed, reachable, addr, dcc_type, instance_id) in outcomes {
        if !reachable {
            if registry.update_status_if_unchanged(
                &observed,
                dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable,
            )? {
                tracing::info!(
                    dcc_type = %dcc_type,
                    instance_id = %instance_id,
                    addr = %addr,
                    "Startup probe: instance unreachable — retained for owner/TTL recovery"
                );
                marked.push(observed);
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
    Ok(marked)
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

        let marked = probe_and_mark_unreachable_instances(
            &registry,
            Duration::from_secs(30),
            "127.0.0.1",
            9765,
        )
        .await
        .unwrap();

        assert!(marked.is_empty());
        assert!(
            registry.get(&key).is_some(),
            "port=0 sidecar rows are booting diagnostics, not startup-probe evictions"
        );
    }

    #[tokio::test]
    async fn startup_probe_marks_unreachable_row_without_removing_it() {
        let dir = tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let entry = ServiceEntry::new("maya", "127.0.0.1", port);
        let instance_id = entry.instance_id;
        let key = entry.key();
        registry.register(entry).unwrap();

        let marked = probe_and_mark_unreachable_instances(
            &registry,
            Duration::from_secs(30),
            "127.0.0.1",
            9765,
        )
        .await
        .unwrap();

        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].instance_id, instance_id);
        let row = registry
            .get(&key)
            .expect("startup probe must retain the row");
        assert_eq!(
            row.status,
            dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
        );
    }
}
