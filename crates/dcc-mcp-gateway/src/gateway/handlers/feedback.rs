//! Gateway-owned feedback endpoint.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dcc_mcp_models::FeedbackReport;
use serde_json::{Value, json};

use crate::gateway::event_log::{EventKind, notify_updated, record_event};
use crate::gateway::state::GatewayState;

const GATEWAY_EVENTS_URI: &str = "resources://gateway/events";

/// `POST /v1/feedback` — record feedback without requiring a live DCC instance.
pub async fn handle_v1_feedback(
    State(gateway): State<GatewayState>,
    Json(body): Json<Value>,
) -> Response {
    let report = match serde_json::from_value::<FeedbackReport>(body) {
        Ok(report) => report,
        Err(error) => return invalid_feedback(error.to_string()),
    };
    if let Err(error) = report.validate() {
        return invalid_feedback(error.to_string());
    }

    let feedback_id = uuid::Uuid::new_v4().to_string();
    let recorded_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let dcc_type = report.dcc_type.as_deref().unwrap_or("gateway");
    let instance_id = report.instance_id.as_deref().unwrap_or("unscoped");
    let event_context = json!({
        "feedback_id": feedback_id,
        "report": report,
    });

    record_event(
        &gateway.event_log,
        #[cfg(feature = "prometheus")]
        &gateway.gateway_metrics,
        EventKind::FeedbackReported,
        dcc_type,
        instance_id,
        Some(event_context.to_string()),
    );
    notify_updated(&gateway.events_tx);
    tracing::info!(
        feedback_id,
        severity = %report.severity,
        dcc_type,
        instance_id,
        "gateway feedback recorded"
    );

    (
        StatusCode::CREATED,
        Json(json!({
            "ok": true,
            "success": true,
            "feedback_id": feedback_id,
            "recorded_at": recorded_at,
            "event_resource_uri": GATEWAY_EVENTS_URI,
        })),
    )
        .into_response()
}

fn invalid_feedback(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "ok": false,
            "success": false,
            "error": {
                "kind": "invalid-feedback",
                "message": message,
            }
        })),
    )
        .into_response()
}
