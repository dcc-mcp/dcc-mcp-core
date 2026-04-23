use super::*;

/// Compute a fingerprint of the aggregated tool list across every live backend.
///
/// The fingerprint is a stable, sorted concatenation of `{instance_id}:{tool_name}`
/// across every live backend.  When this string changes between two polls, we
/// know at least one backend's tool list mutated (skill loaded / unloaded) and
/// we can push a single `notifications/tools/list_changed` to all connected
/// gateway SSE clients.
///
/// Deliberately excludes tool descriptions / schemas: we only want to detect
/// set-level add/remove changes, not metadata edits.
pub async fn compute_tools_fingerprint(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
) -> String {
    let instances: Vec<ServiceEntry> = {
        let reg = registry.read().await;
        reg.list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(stale_timeout)
                    && e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE
                    && !matches!(
                        e.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::ShuttingDown
                            | dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                    )
            })
            .collect()
    };

    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let tools = fetch_tools(http_client, &url, backend_timeout).await;
        (entry.instance_id, tools)
    });
    let results = join_all(futs).await;

    let mut parts: Vec<String> = results
        .into_iter()
        .flat_map(|(iid, tools)| {
            tools
                .into_iter()
                .map(move |t| format!("{iid}:{}", t.name))
                .collect::<Vec<_>>()
        })
        .collect();
    parts.sort_unstable();
    parts.join("|")
}
