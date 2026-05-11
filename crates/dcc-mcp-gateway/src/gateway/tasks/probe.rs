use std::time::Duration;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::GATEWAY_SENTINEL_DCC_TYPE;
use dcc_mcp_transport::error::TransportResult;

/// On gateway startup, probe every registered instance's TCP port and
/// deregister any that are unreachable. Complements `prune_dead_pids`
/// which only checks PID liveness — a process may be alive but its MCP
/// listener already shut down (issue #556).
pub(crate) async fn probe_and_evict_dead_instances(
    registry: &FileRegistry,
    stale_timeout: Duration,
    own_host: &str,
    own_port: u16,
) -> TransportResult<usize> {
    let entries: Vec<_> = registry
        .list_all()
        .into_iter()
        .filter(|e| {
            e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                && !e.is_stale(stale_timeout)
                && !crate::gateway::is_own_instance(e, own_host, own_port)
        })
        .collect();

    let mut evicted = 0usize;
    for entry in entries {
        let addr = format!("{}:{}", entry.host, entry.port);
        let reachable = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        .is_ok_and(|r| r.is_ok());

        if !reachable {
            registry.deregister(&entry.key())?;
            evicted += 1;
            tracing::info!(
                dcc_type = %entry.dcc_type,
                instance_id = %entry.instance_id,
                addr = %addr,
                "Startup probe: instance unreachable — deregistered"
            );
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
