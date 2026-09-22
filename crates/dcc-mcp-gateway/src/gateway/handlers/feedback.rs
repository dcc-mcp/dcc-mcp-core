//! Gateway-owned feedback endpoint.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use dcc_mcp_catalog::CatalogEntry;
use dcc_mcp_gateway_admin::{FeedbackReportRow, FeedbackSubmissionKind};
use dcc_mcp_models::{
    FeedbackReport, FeedbackRoute, FeedbackRouteTarget, FindingV1, route_finding,
};
use serde_json::{Value, json};

use crate::gateway::event_log::{EventKind, notify_updated, record_event};
use crate::gateway::state::GatewayState;

const GATEWAY_EVENTS_URI: &str = "resources://gateway/events";

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Catalog bundled into the binary so ingest routing never depends on a live
/// marketplace fetch. Mirrors the CLI filing path, which uses the same file.
const BUNDLED_CATALOG: &str = include_str!("../../../../../dcc-mcp-catalog.yml");

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
    let recorded_at_ms = now_millis();
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
    // #2253-E1: durable per-submission row. This is the raw submission log;
    // the #2253-E2 dedup aggregate below is a separate table.
    persist_feedback_report(
        &gateway,
        FeedbackReportRow {
            id: feedback_id.clone(),
            timestamp_ms: recorded_at_ms,
            recorded_at_ms,
            recorded_at: recorded_at.clone(),
            kind: submission.kind(),
            schema_version: submission.schema_version(),
            fingerprint: submission.fingerprint(),
            severity: submission.severity(),
            dcc_type: dcc_type.to_string(),
            instance_id: submission.report_instance_id(),
            tool_slug: submission.tool_slug(),
            report: json!({
                "id": feedback_id,
                "timestamp": recorded_at_ms as f64 / 1000.0,
                "recorded_at": recorded_at,
            }),
        },
        submission.report_value(),
    );
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
        // Route at ingest so the persisted row carries its owner without a
        // later CLI filing pass. Unroutable findings are still persisted,
        // deduplicated under the empty repo.
        let route = route_finding_for_ingest(finding);
        if let Some(route) = &route {
            receipt["routed"] = json!(true);
            receipt["repo"] = json!(route.repo);
            receipt["issues_url"] = json!(route.issues_url);
            receipt["route_rationale"] = json!(route.rationale.as_str());
        } else {
            receipt["routed"] = json!(false);
        }
        if let Some(row) = persist_finding(&gateway, finding, route.as_ref()).await {
            receipt["report_id"] = json!(row.id);
            receipt["occurrence_count"] = json!(row.occurrence_count);
            receipt["first_seen_ms"] = json!(row.first_seen_ms);
            receipt["last_seen_ms"] = json!(row.last_seen_ms);
            receipt["duplicate"] = json!(row.occurrence_count > 1);
        }
    }

    correlate_response(
        (StatusCode::CREATED, Json(receipt)).into_response(),
        &headers,
    )
}

/// Resolve the owning issue tracker for one finding, or `None` when the finding
/// cannot be routed deterministically.
///
/// Routing failure is not an ingest failure: the report is still stored so the
/// finding is never silently dropped.
fn route_finding_for_ingest(finding: &FindingV1) -> Option<FeedbackRoute> {
    let targets = catalog_targets()
        .iter()
        .map(|entry| FeedbackRouteTarget::new(&entry.name, entry.issues_url.as_deref()))
        .collect::<Vec<_>>();
    match route_finding(finding, &targets) {
        Ok(route) => Some(route),
        Err(error) => {
            tracing::debug!(
                error = %error,
                adapter = %finding.adapter,
                phase = %finding.phase,
                "feedback ingest: finding could not be routed"
            );
            None
        }
    }
}

/// One catalog entry reduced to the fields routing needs.
struct CatalogTarget {
    name: String,
    issues_url: Option<String>,
}

/// Route targets derived from the bundled catalog, parsed once per process.
///
/// The CLI resolves the catalog once per process; without memoisation the
/// gateway would re-parse the bundled YAML on every `POST /v1/feedback`.
/// A catalog that fails to parse degrades to no targets, which leaves the
/// finding unrouted rather than rejected.
fn catalog_targets() -> &'static [CatalogTarget] {
    static TARGETS: OnceLock<Vec<CatalogTarget>> = OnceLock::new();
    TARGETS
        .get_or_init(|| {
            let entries: Vec<CatalogEntry> = match dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG) {
                Ok(entries) => entries,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "feedback ingest: bundled catalog failed to parse"
                    );
                    Vec::new()
                }
            };
            entries
                .into_iter()
                .map(|entry| CatalogTarget {
                    name: entry.name,
                    issues_url: entry.issues_url,
                })
                .collect()
        })
        .as_slice()
}

/// Collapse the finding onto its `(repo, fingerprint)` row and return it.
///
/// Returns `None` when SQLite persistence is disabled or unavailable; ingest
/// still succeeds in that case so the feedback endpoint keeps working.
#[cfg(feature = "admin-persist-sqlite")]
async fn persist_finding(
    gateway: &GatewayState,
    finding: &FindingV1,
    route: Option<&FeedbackRoute>,
) -> Option<dcc_mcp_db::FeedbackFindingRow> {
    let lane = gateway.admin_sqlite_lane.as_ref()?.clone();
    let report_json = serde_json::to_string(finding).ok()?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let insert = dcc_mcp_db::FeedbackFindingInsert {
        repo: route.map(|route| route.repo.clone()).unwrap_or_default(),
        fingerprint: finding.fingerprint.clone(),
        issues_url: route.map(|route| route.issues_url.clone()),
        route_rationale: route.map(|route| route.rationale.as_str().to_string()),
        dcc_type: finding.dcc_type.clone(),
        phase: finding.phase.to_string(),
        severity: finding.severity.to_string(),
        observed_at_ms: now_ms,
        report_json,
    };
    // The upsert opens a connection, applies the whole DDL batch, and runs a
    // write transaction. Under write contention SQLite waits out the busy
    // timeout (5s by default), which would park a tokio worker thread for the
    // duration, so the blocking call stays off the async executor.
    match tokio::task::spawn_blocking(move || lane.upsert_feedback_finding(&insert)).await {
        Ok(Ok(row)) => Some(row),
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "feedback ingest: report persistence failed");
            None
        }
        Err(error) => {
            tracing::warn!(error = %error, "feedback ingest: persistence task panicked");
            None
        }
    }
}

#[cfg(not(feature = "admin-persist-sqlite"))]
async fn persist_finding(
    _gateway: &GatewayState,
    _finding: &FindingV1,
    _route: Option<&FeedbackRoute>,
) -> Option<dcc_mcp_db::FeedbackFindingRow> {
    None
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

    fn kind(&self) -> FeedbackSubmissionKind {
        match self {
            Self::Finding(_) => FeedbackSubmissionKind::Finding,
            Self::Legacy(_) => FeedbackSubmissionKind::Legacy,
        }
    }

    fn schema_version(&self) -> i64 {
        match self {
            Self::Finding(finding) => i64::from(finding.schema_version),
            Self::Legacy(_) => 0,
        }
    }

    fn fingerprint(&self) -> Option<String> {
        match self {
            Self::Finding(finding) => Some(finding.fingerprint.clone()),
            Self::Legacy(_) => None,
        }
    }

    /// Instance id exactly as the report carries it — `None` when unscoped.
    fn report_instance_id(&self) -> Option<String> {
        match self {
            Self::Finding(finding) => finding.evidence.instance_id.clone(),
            Self::Legacy(report) => report.instance_id.clone(),
        }
    }

    fn tool_slug(&self) -> Option<String> {
        match self {
            Self::Finding(finding) => finding.tool_slug.clone(),
            Self::Legacy(report) => Some(report.tool_name.clone()),
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

fn persist_feedback_report(gateway: &GatewayState, mut row: FeedbackReportRow, report: Value) {
    #[cfg(feature = "admin-persist-sqlite")]
    {
        let Some(lane) = gateway.admin_sqlite_lane.as_ref() else {
            return;
        };
        if let (Value::Object(extra), Value::Object(report)) = (&mut row.report, report) {
            extra.extend(report);
            // Legacy `FeedbackReport` omits `dcc_type` entirely when the caller
            // leaves it unset, while the column always stores the resolved value
            // (`Submission::dcc_type` falls back to "gateway"). Without this
            // backfill the row is selected by `?dcc=gateway` and then dropped by
            // the admin reader's own `dcc_type` check, so a direct
            // `POST /v1/feedback` caller could never see its report (#2253-E1).
            if extra.get("dcc_type").is_none_or(Value::is_null) {
                extra.insert("dcc_type".to_string(), Value::String(row.dcc_type.clone()));
            }
        }
        lane.try_persist_feedback_report(&row);
    }
    #[cfg(not(feature = "admin-persist-sqlite"))]
    {
        let _ = (gateway, &mut row, report);
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

    fn routed_finding(phase: &str, adapter: &str) -> Value {
        json!({
            "schema_version": 1,
            "fingerprint": format!("sha256:{}", "a".repeat(64)),
            "dcc_type": "maya",
            "adapter": adapter,
            "adapter_version": "0.9.7",
            "core_version": "0.20.11",
            "host_version": "2024.1",
            "os": "windows",
            "phase": phase,
            "severity": "degraded",
            "tool_slug": "maya_scene__save",
            "intent": "Save the scene",
            "observed": "The save never completed",
            "expected": "The scene is saved",
            "repro": {"steps": ["Open a scene", "Call save"]},
            "evidence": {"error_kind": "adapter_dispatch_failed", "request_id": "request-42"},
            "redaction_status": {
                "mode": "public-safe",
                "redaction_markers_detected": false,
                "raw_payloads_excluded": true
            }
        })
    }

    #[test]
    fn dispatch_phase_routes_without_gateway_involvement() {
        let finding: FindingV1 = serde_json::from_value(routed_finding("dispatch", "dcc-mcp-maya"))
            .expect("finding parses");
        let route = route_finding_for_ingest(&finding)
            .expect("a dispatch finding for a catalog adapter routes");
        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-maya");
        assert_eq!(
            route.issues_url,
            "https://github.com/dcc-mcp/dcc-mcp-maya/issues"
        );
        assert_eq!(route.rationale.as_str(), "adapter_phase");
    }

    #[test]
    fn unroutable_findings_do_not_fail_ingest() {
        let finding: FindingV1 =
            serde_json::from_value(routed_finding("dispatch", "dcc-mcp-not-a-real-adapter"))
                .expect("finding parses");
        assert!(
            route_finding_for_ingest(&finding).is_none(),
            "an unknown adapter must not be routed, but must still be storable"
        );
    }

    /// Acceptance: the same finding submitted three times keeps one row and
    /// reports `occurrence_count == 3`.
    #[cfg(feature = "admin-persist-sqlite")]
    #[tokio::test]
    async fn repeated_findings_collapse_into_one_row() {
        let dir = tempfile::tempdir().unwrap();
        let lane = crate::gateway::admin::sqlite_lane::AdminSqliteLane::spawn(
            dir.path().join("admin.sqlite"),
            30,
        )
        .expect("lane spawns");
        let mut gateway = test_gateway_state("1.2.3");
        gateway.admin_sqlite_lane = Some(lane.clone());
        let app = crate::gateway::router::build_gateway_router(gateway.clone());
        let body = routed_finding("dispatch", "dcc-mcp-maya");

        for attempt in 1..=3 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/feedback")
                        .header(axum::http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let (status, receipt) = response_json(response).await;
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(receipt["occurrence_count"], attempt);
            assert_eq!(receipt["duplicate"], attempt > 1);
            assert_eq!(receipt["repo"], "dcc-mcp/dcc-mcp-maya");
            assert_eq!(
                receipt["issues_url"],
                "https://github.com/dcc-mcp/dcc-mcp-maya/issues"
            );
            assert_eq!(receipt["route_rationale"], "adapter_phase");
        }

        let rows = lane.list_feedback_findings(10);
        assert_eq!(rows.len(), 1, "three reports must stay one row");
        assert_eq!(rows[0].occurrence_count, 3);
        assert_eq!(rows[0].repo, "dcc-mcp/dcc-mcp-maya");
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
