//! Reproducible experiment HTTP and persistence adapters.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::{
    ExperimentJudgeValidation, project_experiment_detail, project_experiment_list,
    valid_experiment_id, validate_experiment_definition, validate_experiment_judge,
    validate_experiment_run,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::state::AdminState;
use super::recordings::caller_session;

const MAX_EVENTS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExperimentReadError {
    InvalidId,
    NotFound,
    SqliteUnavailable,
}

impl ExperimentReadError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidId => "experiment_id must be a bounded identifier",
            Self::NotFound => "experiment not found",
            Self::SqliteUnavailable => "experiment persistence is unavailable",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ExperimentListQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CreateExperimentBody {
    name: String,
    scenario_id: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    recording_id: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct RunBody {
    #[serde(default)]
    run_id: Option<String>,
    status: String,
    #[serde(default)]
    parent_run_id: Option<String>,
    #[serde(default)]
    parent_session_id: Option<String>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    parameters: Value,
    #[serde(default)]
    metrics: Value,
    #[serde(default)]
    evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct JudgeResultBody {
    run_id: String,
    evaluator_id: String,
    evaluator_version: String,
    rubric_hash: String,
    status: String,
    #[serde(default)]
    scores: Value,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    cost: Option<f64>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    evidence: Vec<String>,
}

pub async fn handle_experiments_list(
    State(state): State<AdminState>,
    Query(query): Query<ExperimentListQuery>,
) -> impl IntoResponse {
    match experiment_list_payload(&state, query.limit.unwrap_or(100)) {
        Ok(payload) => Json(payload).into_response(),
        Err(ExperimentReadError::SqliteUnavailable) => sqlite_unavailable(),
        Err(error) => invalid_request(error.message()),
    }
}

pub async fn handle_experiment_create(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CreateExperimentBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return session_required();
    };
    if let Err(message) = validate_experiment_definition(
        &body.name,
        &body.scenario_id,
        &body.description,
        body.workflow_id.as_deref(),
        body.recording_id.as_deref(),
        &body.tags,
        &body.metadata,
    ) {
        return invalid_request(message);
    }
    let Some(lane) = &state.admin_sqlite_lane else {
        return sqlite_unavailable();
    };
    let experiment_id = uuid::Uuid::new_v4().to_string();
    let event = json!({
        "session_id": session_id,
        "event_type": "experiment.created",
        "created_at_ms": now_ms(),
        "experiment_id": experiment_id,
        "name": body.name,
        "description": body.description,
        "scenario_id": body.scenario_id,
        "workflow_id": body.workflow_id,
        "recording_id": body.recording_id,
        "tags": body.tags,
        "metadata": body.metadata,
        "schema_version": 1,
    });
    lane.try_persist_session_event(&event);
    (StatusCode::CREATED, Json(event)).into_response()
}

pub async fn handle_experiment_run(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(experiment_id): Path<String>,
    Json(body): Json<RunBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return session_required();
    };
    if !valid_experiment_id(&experiment_id) {
        return invalid_request("experiment_id must be a bounded identifier");
    }
    let run_id = body
        .run_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Err(message) = validate_experiment_run(
        &run_id,
        &body.status,
        body.parent_run_id.as_deref(),
        body.parent_session_id.as_deref(),
        &body.parameters,
        &body.metrics,
        &body.evidence,
    ) {
        return invalid_request(message);
    }
    let Some(lane) = &state.admin_sqlite_lane else {
        return sqlite_unavailable();
    };
    let event = json!({
        "session_id": session_id,
        "event_type": format!("experiment.run.{}", body.status),
        "created_at_ms": now_ms(),
        "experiment_id": experiment_id,
        "run_id": run_id,
        "status": body.status,
        "parent_run_id": body.parent_run_id,
        "parent_session_id": body.parent_session_id,
        "seed": body.seed,
        "parameters": body.parameters,
        "metrics": body.metrics,
        "evidence": body.evidence,
        "schema_version": 1,
    });
    lane.try_persist_session_event(&event);
    (StatusCode::ACCEPTED, Json(event)).into_response()
}

pub async fn handle_experiment_judge_result(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(experiment_id): Path<String>,
    Json(body): Json<JudgeResultBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return session_required();
    };
    if !valid_experiment_id(&experiment_id) {
        return invalid_request("invalid or oversized judge result payload");
    }
    if let Err(message) = validate_experiment_judge(ExperimentJudgeValidation {
        run_id: &body.run_id,
        evaluator_id: &body.evaluator_id,
        evaluator_version: &body.evaluator_version,
        rubric_hash: &body.rubric_hash,
        status: &body.status,
        scores: &body.scores,
        summary: &body.summary,
        cost: body.cost,
        evidence: &body.evidence,
    }) {
        return invalid_request(message);
    }
    let Some(lane) = &state.admin_sqlite_lane else {
        return sqlite_unavailable();
    };
    let event = json!({
        "session_id": session_id,
        "event_type": "experiment.judge.result",
        "created_at_ms": now_ms(),
        "experiment_id": experiment_id,
        "run_id": body.run_id,
        "evaluator_id": body.evaluator_id,
        "evaluator_version": body.evaluator_version,
        "rubric_hash": body.rubric_hash,
        "status": body.status,
        "scores": body.scores,
        "summary": body.summary,
        "model": body.model,
        "cost": body.cost,
        "duration_ms": body.duration_ms,
        "evidence": body.evidence,
        "authority": "evidence_only",
        "schema_version": 1,
    });
    lane.try_persist_session_event(&event);
    (StatusCode::ACCEPTED, Json(event)).into_response()
}

pub async fn handle_experiment_detail(
    State(state): State<AdminState>,
    Path(experiment_id): Path<String>,
) -> impl IntoResponse {
    match experiment_detail_payload(&state, &experiment_id) {
        Ok(payload) => Json(payload).into_response(),
        Err(ExperimentReadError::InvalidId) => {
            invalid_request(ExperimentReadError::InvalidId.message())
        }
        Err(ExperimentReadError::SqliteUnavailable) => sqlite_unavailable(),
        Err(ExperimentReadError::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "experiment_not_found", "experiment_id": experiment_id})),
        )
            .into_response(),
    }
}

pub(crate) fn experiment_list_payload(
    state: &AdminState,
    limit: usize,
) -> Result<Value, ExperimentReadError> {
    let Some(lane) = &state.admin_sqlite_lane else {
        return Err(ExperimentReadError::SqliteUnavailable);
    };
    let experiments = lane.reader().list_experiments(limit.clamp(1, 1_000));
    Ok(project_experiment_list(experiments))
}

pub(crate) fn experiment_detail_payload(
    state: &AdminState,
    experiment_id: &str,
) -> Result<Value, ExperimentReadError> {
    if !valid_experiment_id(experiment_id) {
        return Err(ExperimentReadError::InvalidId);
    }
    let Some(lane) = &state.admin_sqlite_lane else {
        return Err(ExperimentReadError::SqliteUnavailable);
    };
    let events = lane
        .reader()
        .list_experiment_events(experiment_id, MAX_EVENTS);
    project_experiment_detail(events).ok_or(ExperimentReadError::NotFound)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn session_required() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "session_required",
            "message": "A bounded x-dcc-mcp-agent-session-id header is required."
        })),
    )
        .into_response()
}

fn sqlite_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "sqlite_not_available"})),
    )
        .into_response()
}

fn invalid_request(message: &'static str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "invalid_experiment_request", "message": message})),
    )
        .into_response()
}
