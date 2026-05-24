use super::*;

use crate::gateway::capability_service::{ServiceError, service_error_to_json};

#[derive(Debug, Default, Deserialize)]
pub struct StopInstanceBody {
    expected_owner: Option<String>,
    expected_session: Option<String>,
}

/// `POST /v1/dcc/{dcc_type}/instances/{instance_id}/stop` — request a safe
/// stop for a test-owned instance that explicitly advertises a safe-stop URL.
///
/// The gateway never kills a process directly. Test launchers opt in by adding
/// `safe_stop_url` (or `dcc_mcp_safe_stop_url`) to registry metadata. Optional
/// `expected_owner` / `expected_session` fields must match public metadata
/// aliases before the gateway forwards the stop request.
pub async fn handle_v1_dcc_instance_stop(
    State(gs): State<GatewayState>,
    Path((dcc_type, instance_id)): Path<(String, String)>,
    Json(body): Json<StopInstanceBody>,
) -> Response {
    let registry = gs.registry.read().await;
    let entry = match gs.resolve_instance(
        &registry,
        Some(instance_id.as_str()),
        Some(dcc_type.as_str()),
    ) {
        Ok(e) => e,
        Err(err) => {
            drop(registry);
            return super::rest_impl::resolve_instance_http_response(err).into_response();
        }
    };
    drop(registry);

    if !entry.dcc_type.eq_ignore_ascii_case(dcc_type.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "path dcc_type does not match resolved registry row",
            ))),
        )
            .into_response();
    }

    let owner = metadata_value(
        &entry,
        &[
            "owner",
            "test_owner",
            "dcc_mcp_owner",
            "dcc_mcp_test_owner",
            "dcc_mcp.owner",
        ],
    );
    if let Some(expected) = body.expected_owner.as_deref().map(str::trim)
        && Some(expected) != owner
    {
        return lifecycle_guard_response("owner", expected, owner);
    }

    let session = metadata_value(
        &entry,
        &[
            "session",
            "test_session",
            "dcc_mcp_session",
            "dcc_mcp_test_session",
            "dcc_mcp.session",
        ],
    );
    if let Some(expected) = body.expected_session.as_deref().map(str::trim)
        && Some(expected) != session
    {
        return lifecycle_guard_response("session", expected, session);
    }

    let Some(stop_url) = metadata_value(
        &entry,
        &[
            "safe_stop_url",
            "dcc_mcp_safe_stop_url",
            "dcc_mcp.safe_stop_url",
            "stop_url",
        ],
    ) else {
        return (
            StatusCode::CONFLICT,
            Json(service_error_to_json(&ServiceError::new(
                "optional-capability-unsupported",
                "instance does not advertise safe_stop_url metadata; refusing to stop it",
            ))),
        )
            .into_response();
    };

    let method = metadata_value(
        &entry,
        &[
            "safe_stop_method",
            "dcc_mcp_safe_stop_method",
            "dcc_mcp.safe_stop_method",
        ],
    )
    .unwrap_or("POST");
    if !method.eq_ignore_ascii_case("POST") {
        return (
            StatusCode::CONFLICT,
            Json(service_error_to_json(&ServiceError::new(
                "optional-capability-unsupported",
                format!("unsupported safe_stop_method '{method}'; only POST is supported"),
            ))),
        )
            .into_response();
    }

    let request = json!({
        "instance_id": entry.instance_id.to_string(),
        "dcc_type": entry.dcc_type.clone(),
        "owner": owner,
        "session": session,
    });
    match gs.http_client.post(stop_url).json(&request).send().await {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let backend_response =
                serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!(text));
            if status.is_success() {
                (
                    StatusCode::OK,
                    Json(json!({
                        "ok": true,
                        "stopping": true,
                        "instance_id": entry.instance_id.to_string(),
                        "dcc_type": entry.dcc_type.clone(),
                        "safe_stop_url": stop_url,
                        "response": backend_response,
                    })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(service_error_to_json(&ServiceError::new(
                        "backend-error",
                        format!("safe_stop_url returned HTTP {status}: {text}"),
                    ))),
                )
                    .into_response()
            }
        }
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(service_error_to_json(&ServiceError::new(
                "backend-error",
                format!("safe_stop_url request failed: {err}"),
            ))),
        )
            .into_response(),
    }
}

fn metadata_value<'a>(
    entry: &'a dcc_mcp_transport::discovery::types::ServiceEntry,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| entry.metadata.get(*key).map(String::as_str))
        .find(|value| !value.trim().is_empty())
}

fn lifecycle_guard_response(field: &str, expected: &str, actual: Option<&str>) -> Response {
    (
        StatusCode::CONFLICT,
        Json(service_error_to_json(&ServiceError::new(
            "lifecycle-guard-mismatch",
            format!("expected {field}='{expected}' but instance metadata has {actual:?}"),
        ))),
    )
        .into_response()
}
