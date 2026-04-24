use super::*;

/// `POST /mcp/{instance_id}` — transparent proxy to a specific DCC instance.
pub async fn handle_proxy_instance(
    State(gs): State<GatewayState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let registry = gs.registry.read().await;
    let entry = registry.list_all().into_iter().find(|entry| {
        let entry_id = entry.instance_id.to_string();
        entry_id == instance_id || entry_id.starts_with(&instance_id)
    });
    drop(registry);

    match entry {
        Some(entry) => {
            let url = format!("http://{}:{}/mcp", entry.host, entry.port);
            proxy_request(&gs.http_client, &url, headers, body).await
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Instance '{}' not found", instance_id)})),
        )
            .into_response(),
    }
}

/// `POST /mcp/dcc/{dcc_type}` — proxy to best available instance of a DCC type.
pub async fn handle_proxy_dcc(
    State(gs): State<GatewayState>,
    Path(dcc_type): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let registry = gs.registry.read().await;
    let mut candidates = gs
        .live_instances(&registry)
        .into_iter()
        .filter(|entry| entry.dcc_type == dcc_type)
        .collect::<Vec<_>>();
    drop(registry);

    if candidates.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("No live '{}' instances", dcc_type)})),
        )
            .into_response();
    }

    candidates.sort_by_key(|entry| matches!(entry.status, ServiceStatus::Busy) as u8);
    let url = format!("http://{}:{}/mcp", candidates[0].host, candidates[0].port);
    proxy_request(&gs.http_client, &url, headers, body).await
}
