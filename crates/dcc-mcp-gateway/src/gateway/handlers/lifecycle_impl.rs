use super::*;

use crate::gateway::capability_service::{
    ServiceError, safe_discovery_target, service_error_to_json,
};
use crate::gateway::http_registration::{SOURCE_HTTP, entry_registry_source};

/// Metadata aliases under which a launched instance advertises the lifecycle
/// operation that owns it. Mirrors the CLI-side contract so the gateway can
/// verify an operation-scoped stop without depending on the CLI crate.
const OPERATION_METADATA_KEYS: &[&str] = &[
    "dcc_mcp_operation_id",
    "operation_id",
    "dcc_mcp.operation_id",
];

#[derive(Debug, Default, Deserialize)]
pub struct StopInstanceBody {
    expected_owner: Option<String>,
    expected_session: Option<String>,
    operation_id: Option<String>,
}

/// `POST /v1/dcc/{dcc_type}/instances/{instance_id}/stop` — request a safe
/// stop for a test-owned instance that explicitly advertises a safe-stop URL.
///
/// The gateway never kills a process directly. Test launchers opt in by adding
/// `safe_stop_url` (or `dcc_mcp_safe_stop_url`) to registry metadata. Both
/// `expected_owner` and `expected_session` must match public metadata aliases
/// before the gateway forwards the stop request.
///
/// `operation_id` is optional but authoritative when present: it scopes the
/// stop to the instance that the given start-instance operation launched, so a
/// caller cannot stop a neighbouring instance by guessing its id. An instance
/// that advertises no operation id cannot prove that ownership and is
/// rejected rather than stopped.
pub async fn handle_v1_dcc_instance_stop(
    State(gs): State<GatewayState>,
    Path((dcc_type, instance_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<StopInstanceBody>,
) -> Response {
    if let Err(err) = gs.auth.authorize_register(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        &dcc_type,
    ) {
        return (
            StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::UNAUTHORIZED),
            Json(service_error_to_json(&ServiceError::new(
                err.kind(),
                err.message(),
            ))),
        )
            .into_response();
    }

    let expected_owner = body
        .expected_owner
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let expected_session = body
        .expected_session
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if expected_owner.is_none() || expected_session.is_none() {
        return lifecycle_guard_response(
            "owner and session",
            "both expected_owner and expected_session are required",
            None,
        );
    }
    let expected_operation = body
        .operation_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let entry = match gs
        .resolve_instance_async(Some(instance_id.as_str()), Some(dcc_type.as_str()))
        .await
    {
        Ok(e) => e,
        Err(err) => {
            return super::rest_impl::resolve_instance_http_response(err).into_response();
        }
    };

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
    if let Some(expected) = expected_owner
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
    if let Some(expected) = expected_session
        && Some(expected) != session
    {
        return lifecycle_guard_response("session", expected, session);
    }

    // An operation-scoped stop is authoritative: the instance must advertise
    // the operation that launched it, and it must be the same one. Absent
    // metadata fails closed — the gateway cannot prove ownership.
    if let Some(expected) = expected_operation {
        let operation = metadata_value(&entry, OPERATION_METADATA_KEYS);
        if operation != Some(expected) {
            return lifecycle_guard_response("operation", expected, operation);
        }
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

    if let Err(message) = validate_safe_stop_url(&entry, stop_url) {
        return (
            StatusCode::CONFLICT,
            Json(service_error_to_json(&ServiceError::new(
                "unsafe-backend-target",
                message,
            ))),
        )
            .into_response();
    }

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
    let stop_client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(gs.backend_timeout.min(std::time::Duration::from_secs(10)))
        .timeout(gs.backend_timeout)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(service_error_to_json(&ServiceError::new(
                    "backend-error",
                    format!("safe_stop_url client setup failed: {error}"),
                ))),
            )
                .into_response();
        }
    };
    match stop_client.post(stop_url).json(&request).send().await {
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

fn validate_safe_stop_url(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
    raw: &str,
) -> Result<(), String> {
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|_| "safe_stop_url must be a valid HTTP(S) URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("safe_stop_url must not contain credentials, query, or fragment".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "safe_stop_url is missing a host".to_string())?;
    if !host.eq_ignore_ascii_case(&entry.host) {
        return Err("safe_stop_url host must match the registered instance host".into());
    }
    let advertised = metadata_value(entry, &["mcp_url", "dcc_mcp_url"])
        .ok_or_else(|| "safe_stop_url requires an advertised MCP URL".to_string())?;
    let advertised_url = reqwest::Url::parse(advertised)
        .map_err(|_| "safe_stop_url requires a valid advertised MCP URL".to_string())?;
    let stop_path = url.path().trim_end_matches('/');
    if url.scheme() != advertised_url.scheme()
        || url.port_or_known_default() != advertised_url.port_or_known_default()
        || !matches!(stop_path, "/stop" | "/safe-stop")
    {
        return Err(
            "safe_stop_url must use the registered MCP scheme/port and a /stop or /safe-stop path"
                .into(),
        );
    }
    if entry_registry_source(entry) == SOURCE_HTTP && !safe_discovery_target(entry) {
        return Err("safe_stop_url requires a validated public HTTP registration".into());
    }
    Ok(())
}

fn metadata_value<'a>(
    entry: &'a dcc_mcp_transport::discovery::types::ServiceEntry,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| entry.metadata.get(*key).map(String::as_str))
        .find(|value| !value.trim().is_empty())
}

/// Refuses a guarded stop when a guard value does not match.
///
/// The instance's stored value is never echoed back: it is registry metadata a
/// caller could otherwise harvest by probing the guard, which would hand over
/// the very secret the guard exists to check. Presence or absence is enough to
/// diagnose a mismatch. `expected` is the caller's own input, so it is safe to
/// repeat.
fn lifecycle_guard_response(field: &str, expected: &str, actual: Option<&str>) -> Response {
    let message = if actual.is_some() {
        format!("expected {field}='{expected}' but the instance advertises a different {field}")
    } else {
        format!("expected {field}='{expected}' but the instance advertises no {field}")
    };
    (
        StatusCode::CONFLICT,
        Json(service_error_to_json(&ServiceError::new(
            "lifecycle-guard-mismatch",
            message,
        ))),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::http_registration::{
        DISCOVERY_MCP_URL_METADATA_KEY, MCP_URL_METADATA_KEY, REGISTRY_SOURCE_METADATA_KEY,
    };
    use dcc_mcp_transport::discovery::types::ServiceEntry;

    fn public_http_entry() -> ServiceEntry {
        let mut entry = ServiceEntry::new("maya", "93.184.216.34", 8765);
        entry.metadata.insert(
            REGISTRY_SOURCE_METADATA_KEY.to_string(),
            SOURCE_HTTP.to_string(),
        );
        entry.metadata.insert(
            MCP_URL_METADATA_KEY.to_string(),
            "http://93.184.216.34:8765/mcp".to_string(),
        );
        entry.metadata.insert(
            DISCOVERY_MCP_URL_METADATA_KEY.to_string(),
            "http://93.184.216.34:8765/mcp".to_string(),
        );
        entry
    }

    #[test]
    fn safe_stop_requires_plain_same_host_url() {
        let entry = public_http_entry();
        for raw in [
            "http://93.184.216.34:8765/stop?token=secret",
            "http://user:pass@93.184.216.34:8765/stop",
            "http://93.184.216.34:8765/stop#fragment",
            "http://198.51.100.9:8765/stop",
        ] {
            assert!(
                validate_safe_stop_url(&entry, raw).is_err(),
                "unsafe stop URL must be rejected: {raw}"
            );
        }
    }

    #[test]
    fn safe_stop_accepts_validated_public_registration() {
        let entry = public_http_entry();
        assert!(validate_safe_stop_url(&entry, "http://93.184.216.34:8765/stop").is_ok());
    }

    #[test]
    fn safe_stop_is_bound_to_registered_scheme_port_and_path() {
        let entry = public_http_entry();
        for raw in [
            "https://93.184.216.34:8765/stop",
            "http://93.184.216.34:9999/stop",
            "http://93.184.216.34:8765/admin/stop",
        ] {
            assert!(
                validate_safe_stop_url(&entry, raw).is_err(),
                "stop target must stay bound to the registered endpoint: {raw}"
            );
        }
    }

    /// A guard mismatch must not hand the caller the value the guard is
    /// protecting: echoing it turns the guard into an oracle for harvesting
    /// registry metadata by probing.
    #[tokio::test]
    async fn guard_mismatch_does_not_echo_the_stored_value() {
        for (field, actual) in [
            ("owner", Some("release-smoke-test")),
            ("session", Some("sess-9f3c1a")),
            ("operation", Some("op-1")),
            ("operation", None),
        ] {
            let response = lifecycle_guard_response(field, "expected-value", actual);
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let body = String::from_utf8(bytes.to_vec()).unwrap();

            assert!(
                body.contains("lifecycle-guard-mismatch"),
                "{field}: unexpected body {body}"
            );
            if let Some(actual) = actual {
                assert!(
                    !body.contains(actual),
                    "{field}: guard response leaked the stored value: {body}"
                );
            }
            assert!(
                body.contains("expected-value"),
                "{field}: the caller's own value should still be repeated: {body}"
            );
        }
    }
}
