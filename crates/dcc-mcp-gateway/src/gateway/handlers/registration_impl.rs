use super::*;

use std::time::SystemTime;

use crate::gateway::capability::remove_instance;
use crate::gateway::http_registration::{
    HttpInstanceDeregisterRequest, HttpInstanceHeartbeatRequest, HttpInstanceRegistrationRequest,
    RegistrationError, RegistrationOutcome, unix_secs,
};
use crate::gateway::middleware::{CallResult, record_gateway_event};
use crate::gateway::{GatewayAuthScope, security::auth_error_value};

pub async fn handle_v1_instances_register(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<HttpInstanceRegistrationRequest>,
) -> Response {
    if let Err(err) =
        gs.security
            .authorize(&headers, GatewayAuthScope::Register, Some(&body.dcc_type))
    {
        audit_registration_auth_failure(
            &gs,
            &headers,
            "gateway.register",
            Some(&body.dcc_type),
            &err,
        )
        .await;
        return err.response();
    }
    let outcome = {
        let mut registry = gs.http_instance_registry.write();
        registry.register(body, SystemTime::now())
    };
    match outcome {
        Ok(outcome) => {
            broadcast_resource_list_changed(&gs);
            audit_registration_success(&gs, &headers, "gateway.register", &outcome, "registered")
                .await;
            registration_ok_response(outcome, "registered")
        }
        Err(err) => {
            audit_registration_error(&gs, &headers, "gateway.register", &err).await;
            registration_error_response(err)
        }
    }
}

pub async fn handle_v1_instances_heartbeat(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<HttpInstanceHeartbeatRequest>,
) -> Response {
    if let Err(err) = gs
        .security
        .authorize(&headers, GatewayAuthScope::Register, None)
    {
        audit_registration_auth_failure(&gs, &headers, "gateway.heartbeat", None, &err).await;
        return err.response();
    }
    let outcome = {
        let mut registry = gs.http_instance_registry.write();
        registry.heartbeat(body, SystemTime::now())
    };
    match outcome {
        Ok(outcome) => {
            audit_registration_success(&gs, &headers, "gateway.heartbeat", &outcome, "heartbeat")
                .await;
            registration_ok_response(outcome, "heartbeat")
        }
        Err(err) => {
            audit_registration_error(&gs, &headers, "gateway.heartbeat", &err).await;
            registration_error_response(err)
        }
    }
}

pub async fn handle_v1_instances_deregister(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<HttpInstanceDeregisterRequest>,
) -> Response {
    if let Err(err) = gs
        .security
        .authorize(&headers, GatewayAuthScope::Register, None)
    {
        audit_registration_auth_failure(&gs, &headers, "gateway.deregister", None, &err).await;
        return err.response();
    }
    let removed = {
        let mut registry = gs.http_instance_registry.write();
        registry.deregister(body)
    };
    match removed {
        Ok(Some(entry)) => {
            remove_instance(&gs.capability_index, entry.instance_id);
            broadcast_resource_list_changed(&gs);
            record_gateway_event(
                &gs.middleware_chain,
                Some(&headers),
                "gateway.deregister",
                Some(&entry.dcc_type),
                Some(&entry.instance_id.to_string()),
                json!({
                    "operation": "deregistered",
                    "instance_id": entry.instance_id.to_string(),
                    "source": "http",
                }),
                CallResult::from_tuple("deregistered", false),
            )
            .await;
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
        Err(err) => {
            audit_registration_error(&gs, &headers, "gateway.deregister", &err).await;
            registration_error_response(err)
        }
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

async fn audit_registration_success(
    gs: &GatewayState,
    headers: &HeaderMap,
    method: &str,
    outcome: &RegistrationOutcome,
    operation: &str,
) {
    record_gateway_event(
        &gs.middleware_chain,
        Some(headers),
        method,
        Some(&outcome.entry.dcc_type),
        Some(&outcome.entry.instance_id.to_string()),
        json!({
            "operation": operation,
            "instance_id": outcome.entry.instance_id.to_string(),
            "source": "http",
        }),
        CallResult::from_tuple(operation, false),
    )
    .await;
}

async fn audit_registration_error(
    gs: &GatewayState,
    headers: &HeaderMap,
    method: &str,
    err: &RegistrationError,
) {
    record_gateway_event(
        &gs.middleware_chain,
        Some(headers),
        method,
        None,
        None,
        json!({"error": err.to_string()}),
        CallResult::from_tuple(err.to_string(), true),
    )
    .await;
}

async fn audit_registration_auth_failure(
    gs: &GatewayState,
    headers: &HeaderMap,
    method: &str,
    dcc_type: Option<&str>,
    err: &crate::gateway::security::GatewayAuthError,
) {
    record_gateway_event(
        &gs.middleware_chain,
        Some(headers),
        method,
        dcc_type,
        None,
        auth_error_value(err),
        CallResult::from_tuple(err.message.clone(), true),
    )
    .await;
}

#[cfg(test)]
#[path = "registration_impl_tests.rs"]
mod registration_impl_tests;
