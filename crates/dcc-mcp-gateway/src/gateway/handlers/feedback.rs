//! Gateway-owned feedback endpoint.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use dcc_mcp_models::{FeedbackReport, FindingV1};
use serde_json::{Value, json};

use crate::gateway::event_log::{EventKind, notify_updated, record_event};
use crate::gateway::state::GatewayState;

const GATEWAY_EVENTS_URI: &str = "resources://gateway/events";

/// `POST /v1/feedback` — record feedback without requiring a live DCC instance.
pub async fn handle_v1_feedback(
    State(gateway): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let submission = match parse_submission(body) {
        Ok(submission) => submission,
        Err(error) => return correlate_response(invalid_feedback(error), &headers),
    };

    let feedback_id = uuid::Uuid::new_v4().to_string();
    let recorded_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let dcc_type = submission.dcc_type();
    let instance_id = submission.instance_id();
    let event_context = json!({
        "feedback_id": feedback_id,
        "report": submission.report_value(),
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
        severity = submission.severity(),
        dcc_type,
        instance_id,
        "gateway feedback recorded"
    );

    let mut receipt = json!({
        "ok": true,
        "success": true,
        "feedback_id": feedback_id,
        "recorded_at": recorded_at,
        "event_resource_uri": GATEWAY_EVENTS_URI,
    });
    if let Submission::Finding(finding) = &submission {
        receipt["schema_version"] = json!(finding.schema_version);
        receipt["fingerprint"] = json!(finding.fingerprint);
    }

    correlate_response(
        (StatusCode::CREATED, Json(receipt)).into_response(),
        &headers,
    )
}

enum Submission {
    Finding(Box<FindingV1>),
    Legacy(FeedbackReport),
}

impl Submission {
    fn dcc_type(&self) -> &str {
        match self {
            Self::Finding(finding) => &finding.dcc_type,
            Self::Legacy(report) => report.dcc_type.as_deref().unwrap_or("gateway"),
        }
    }

    fn instance_id(&self) -> &str {
        match self {
            Self::Finding(finding) => finding
                .evidence
                .instance_id
                .as_deref()
                .unwrap_or("unscoped"),
            Self::Legacy(report) => report.instance_id.as_deref().unwrap_or("unscoped"),
        }
    }

    fn severity(&self) -> String {
        match self {
            Self::Finding(finding) => finding.severity.to_string(),
            Self::Legacy(report) => report.severity.to_string(),
        }
    }

    fn report_value(&self) -> Value {
        match self {
            Self::Finding(finding) => serde_json::to_value(finding),
            Self::Legacy(report) => serde_json::to_value(report),
        }
        .expect("validated feedback submissions are serializable")
    }
}

fn parse_submission(body: Value) -> Result<Submission, String> {
    if body.get("schema_version").is_some() {
        let finding =
            serde_json::from_value::<FindingV1>(body).map_err(|error| error.to_string())?;
        finding.validate().map_err(|error| error.to_string())?;
        Ok(Submission::Finding(Box::new(finding)))
    } else {
        let report =
            serde_json::from_value::<FeedbackReport>(body).map_err(|error| error.to_string())?;
        report.validate().map_err(|error| error.to_string())?;
        Ok(Submission::Legacy(report))
    }
}

fn correlate_response(mut response: Response, request_headers: &HeaderMap) -> Response {
    if let Some(request_id) = request_headers.get("x-request-id") {
        response
            .headers_mut()
            .insert("x-request-id", request_id.clone());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use crate::gateway::handlers::rest_impl::rest_impl_tests::{response_json, test_gateway_state};

    #[tokio::test]
    async fn gateway_feedback_records_with_zero_live_dcc_instances() {
        let gateway = test_gateway_state("1.2.3");
        assert!(gateway.live_instances_async().await.is_empty());
        let mut updates = gateway.events_tx.subscribe();
        let app = crate::gateway::router::build_gateway_router(gateway.clone());
        let request_body = json!({
            "tool_name": "houdini.ui_control__act",
            "intent": "Open the render menu",
            "attempt": "Invoked the semantic menu action",
            "blocker": "The owning DCC process exited",
            "severity": "blocked",
            "dcc_type": "houdini",
            "instance_id": "aaaaaaaa-0000-0000-0000-000000000000",
            "request_id": "request-42",
            "job_id": "job-42"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/feedback")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .header("x-request-id", "feedback-request-42")
                    .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("feedback-request-42")
        );
        let (status, body) = response_json(response).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["ok"], true);
        assert_eq!(body["event_resource_uri"], GATEWAY_EVENTS_URI);
        assert!(uuid::Uuid::parse_str(body["feedback_id"].as_str().unwrap()).is_ok());

        let events = gateway.event_log.recent_events(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, EventKind::FeedbackReported);
        assert_eq!(events[0].dcc_type, "houdini");
        assert_eq!(
            events[0].instance_id,
            "aaaaaaaa-0000-0000-0000-000000000000"
        );
        assert!(
            events[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("request-42"))
        );
        let notification = updates
            .try_recv()
            .expect("event resource update notification");
        assert!(notification.contains(GATEWAY_EVENTS_URI));
    }

    #[tokio::test]
    async fn gateway_feedback_rejects_empty_required_text() {
        let gateway = test_gateway_state("1.2.3");
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-request-id", "invalid-feedback-42".parse().unwrap());
        let response = handle_v1_feedback(
            State(gateway),
            headers,
            Json(json!({
                "tool_name": "maya_scene__save",
                "intent": "Save the scene",
                "blocker": " ",
                "severity": "suggestion"
            })),
        )
        .await;
        assert_eq!(
            response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("invalid-feedback-42")
        );
        let (status, body) = response_json(response).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["kind"], "invalid-feedback");
    }

    #[tokio::test]
    async fn gateway_feedback_accepts_and_echoes_finding_v1_fingerprint() {
        let gateway = test_gateway_state("1.2.3");
        let app = crate::gateway::router::build_gateway_router(gateway.clone());
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let request_body = json!({
            "schema_version": 1,
            "fingerprint": fingerprint,
            "dcc_type": "photoshop",
            "adapter": "dcc-mcp-photoshop",
            "adapter_version": "0.9.7",
            "core_version": "0.20.11",
            "host_version": "26.4.1",
            "os": "windows",
            "phase": "skill",
            "severity": "degraded",
            "tool_slug": "photoshop_layers__merge",
            "intent": "Merge the selected layers",
            "observed": "The document remained locked",
            "expected": "The selected layers are merged",
            "repro": {"steps": ["Open a layered document", "Call merge"]},
            "evidence": {"error_kind": "document_locked", "request_id": "request-42"},
            "redaction_status": {
                "mode": "needs-review",
                "redaction_markers_detected": false,
                "raw_payloads_excluded": true
            }
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/feedback")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = response_json(response).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["schema_version"], 1);
        assert_eq!(body["fingerprint"], request_body["fingerprint"]);
        assert!(
            gateway.event_log.recent_events(1)[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("document_locked"))
        );
    }
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
