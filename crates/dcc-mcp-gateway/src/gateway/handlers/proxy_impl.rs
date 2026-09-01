use super::*;
use crate::gateway::capability_service::safe_discovery_target;
use crate::gateway::http_registration::entry_mcp_url;

/// `POST /mcp/{instance_id}` — transparent proxy to a specific DCC instance.
pub async fn handle_proxy_instance(
    State(gs): State<GatewayState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let entry = gs
        .resolve_instance_async(Some(instance_id.as_str()), None)
        .await
        .ok();

    match entry {
        Some(entry) => {
            if !safe_discovery_target(&entry) {
                return unsafe_backend_target_response(&entry);
            }
            if let Err(error) = crate::gateway::lease_guard::check_raw_mcp_call_owner(&entry, &body)
            {
                return lease_rejection_response(&entry, error, &body);
            }
            let url = entry_mcp_url(&entry);
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
    let matching = gs
        .live_instances_async()
        .await
        .into_iter()
        .filter(|entry| entry.dcc_type == dcc_type)
        .collect::<Vec<_>>();

    if matching.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("No live '{}' instances", dcc_type)})),
        )
            .into_response();
    }

    let mut candidates = matching
        .iter()
        .filter(|entry| safe_discovery_target(entry))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return unsafe_backend_target_response(&matching[0]);
    }

    candidates.sort_by_key(|entry| matches!(entry.status, ServiceStatus::Busy) as u8);
    let entry = candidates[0];
    if let Err(error) = crate::gateway::lease_guard::check_raw_mcp_call_owner(entry, &body) {
        return lease_rejection_response(entry, error, &body);
    }
    let url = entry_mcp_url(entry);
    proxy_request(&gs.http_client, &url, headers, body).await
}

fn unsafe_backend_target_response(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "backend target rejected by gateway safety policy",
            "kind": "unsafe-backend-target",
            "instance_id": entry.instance_id,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_http_private_targets_are_rejected_before_proxy() {
        let mut entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya",
            "::ffff:127.0.0.1",
            8765,
        );
        entry.metadata.insert(
            crate::gateway::http_registration::REGISTRY_SOURCE_METADATA_KEY.to_string(),
            crate::gateway::http_registration::SOURCE_HTTP.to_string(),
        );
        entry.metadata.insert(
            crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
            "http://[::ffff:127.0.0.1]:8765/mcp".to_string(),
        );

        assert!(!safe_discovery_target(&entry));
        let response = unsafe_backend_target_response(&entry);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

fn lease_rejection_response(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
    error: dcc_mcp_transport::discovery::types::LeaseOwnerError,
    body: &[u8],
) -> Response {
    let payload = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    if let Value::Array(requests) = payload {
        let responses = requests
            .into_iter()
            .filter_map(|request| {
                let id = request.get("id")?.clone();
                Some(match crate::gateway::lease_guard::check_raw_request_owner(entry, &request) {
                    Err(error) => lease_error_value(entry, error, id),
                    Ok(()) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32000,
                            "message": "batch was not dispatched because another tools/call item failed active lease validation",
                            "data": {"kind": "batch-rejected"}
                        }
                    }),
                })
            })
            .collect::<Vec<_>>();
        if responses.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return (StatusCode::OK, Json(Value::Array(responses))).into_response();
    }
    let Some(id) = payload.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    (StatusCode::OK, Json(lease_error_value(entry, error, id))).into_response()
}

fn lease_error_value(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
    error: dcc_mcp_transport::discovery::types::LeaseOwnerError,
    id: Value,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": format!("{error} for instance {}", entry.instance_id),
            "data": {
                "kind": error.kind(),
                "instance_id": entry.instance_id,
            }
        }
    })
}
