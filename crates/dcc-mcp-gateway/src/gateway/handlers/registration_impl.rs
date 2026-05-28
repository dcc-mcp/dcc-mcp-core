use super::*;

use std::time::SystemTime;

use axum::http::HeaderMap;

use crate::gateway::capability::remove_instance;
use crate::gateway::http_registration::{
    HttpInstanceDeregisterRequest, HttpInstanceHeartbeatRequest, HttpInstanceRegistrationRequest,
    RegistrationError, RegistrationOutcome, unix_secs,
};
use crate::gateway::security::AuthError;

pub async fn handle_v1_instances_register(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<HttpInstanceRegistrationRequest>,
) -> Response {
    // #1365 — bearer-token + DCC-scope auth on the cross-host registration
    // plane. `GatewayAuth::disabled()` (the default) keeps the historical
    // local-trust behaviour: every request is accepted regardless of the
    // Authorization header.
    if let Err(err) = gs.auth.authorize_register(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        &body.dcc_type,
    ) {
        return registration_auth_error_response(err);
    }

    let outcome = {
        let mut registry = gs.http_instance_registry.write();
        registry.register(body, SystemTime::now())
    };
    match outcome {
        Ok(outcome) => {
            broadcast_resource_list_changed(&gs);
            registration_ok_response(outcome, "registered")
        }
        Err(err) => registration_error_response(err),
    }
}

pub async fn handle_v1_instances_heartbeat(
    State(gs): State<GatewayState>,
    Json(body): Json<HttpInstanceHeartbeatRequest>,
) -> Response {
    let outcome = {
        let mut registry = gs.http_instance_registry.write();
        registry.heartbeat(body, SystemTime::now())
    };
    match outcome {
        Ok(outcome) => registration_ok_response(outcome, "heartbeat"),
        Err(err) => registration_error_response(err),
    }
}

pub async fn handle_v1_instances_deregister(
    State(gs): State<GatewayState>,
    Json(body): Json<HttpInstanceDeregisterRequest>,
) -> Response {
    let removed = {
        let mut registry = gs.http_instance_registry.write();
        registry.deregister(body)
    };
    match removed {
        Ok(Some(entry)) => {
            remove_instance(&gs.capability_index, entry.instance_id);
            broadcast_resource_list_changed(&gs);
            Json(json!({
                "ok": true,
                "success": true,
                "operation": "deregistered",
                "instance_id": entry.instance_id.to_string(),
                "instance_short": instance_short(&entry.instance_id),
            }))
            .into_response()
        }
        Ok(None) => Json(json!({
            "ok": true,
            "success": true,
            "operation": "not_registered",
        }))
        .into_response(),
        Err(err) => registration_error_response(err),
    }
}

fn registration_ok_response(outcome: RegistrationOutcome, operation: &str) -> Response {
    Json(json!({
        "ok": true,
        "success": true,
        "operation": operation,
        "instance_id": outcome.entry.instance_id.to_string(),
        "instance_short": instance_short(&outcome.entry.instance_id),
        "registered_at": unix_secs(outcome.entry.registered_at),
        "heartbeat_interval_secs": outcome.heartbeat_interval_secs,
    }))
    .into_response()
}

fn registration_error_response(err: RegistrationError) -> Response {
    let (status, kind) = match &err {
        RegistrationError::InvalidField { .. } => (StatusCode::BAD_REQUEST, "bad-request"),
        RegistrationError::NotFound { .. } => (StatusCode::NOT_FOUND, "not-found"),
    };
    (
        status,
        Json(json!({
            "ok": false,
            "success": false,
            "error": {
                "kind": kind,
                "message": err.to_string(),
            }
        })),
    )
        .into_response()
}

fn registration_auth_error_response(err: AuthError) -> Response {
    let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::UNAUTHORIZED);
    let kind = err.kind();
    let message = err.message();
    let mut body = json!({
        "ok": false,
        "success": false,
        "error": {
            "kind": kind,
            "message": message,
        }
    });
    if let AuthError::DccScopeMismatch { presented_dcc } = &err {
        body["error"]["dcc_type"] = json!(presented_dcc);
    }
    (status, Json(body)).into_response()
}

fn instance_short(instance_id: &uuid::Uuid) -> String {
    instance_id.simple().to_string()[..8].to_string()
}

fn broadcast_resource_list_changed(gs: &GatewayState) {
    if gs.events_tx.receiver_count() == 0 {
        return;
    }
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/resources/list_changed",
        "params": {},
    });
    let _ = gs.events_tx.send(notification.to_string());
}

#[cfg(test)]
#[path = "registration_impl_tests.rs"]
mod registration_impl_tests;
