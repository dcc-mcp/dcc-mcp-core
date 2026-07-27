//! Reproducible experiment projections backed by the existing session timeline.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::recordings::caller_session;
use super::state::AdminState;

const MAX_METADATA_BYTES: usize = 16 * 1024;
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
    if let Err(message) = validate_create(&body) {
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
    if !valid_id(&experiment_id) {
        return invalid_request("experiment_id must be a bounded identifier");
    }
    let run_id = body
        .run_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if !valid_id(&run_id)
        || !matches!(
            body.status.as_str(),
            "pending" | "running" | "passed" | "failed" | "error" | "cancelled"
        )
        || body
            .parent_run_id
            .as_deref()
            .is_some_and(|value| !valid_id(value))
        || body
            .parent_session_id
            .as_deref()
            .is_some_and(|value| !valid_id(value))
        || !bounded_json(&body.parameters)
        || !bounded_json(&body.metrics)
        || !valid_evidence(&body.evidence)
    {
        return invalid_request("invalid or oversized experiment run payload");
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
    if !valid_id(&experiment_id)
        || !valid_id(&body.run_id)
        || !valid_id(&body.evaluator_id)
        || body.evaluator_version.is_empty()
        || body.evaluator_version.len() > 64
        || body.rubric_hash.is_empty()
        || body.rubric_hash.len() > 256
        || !matches!(body.status.as_str(), "passed" | "failed" | "error")
        || body.summary.len() > 2_048
        || !bounded_json(&body.scores)
        || !valid_evidence(&body.evidence)
        || body
            .cost
            .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return invalid_request("invalid or oversized judge result payload");
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
    Ok(json!({"total": experiments.len(), "experiments": experiments}))
}

pub(crate) fn experiment_detail_payload(
    state: &AdminState,
    experiment_id: &str,
) -> Result<Value, ExperimentReadError> {
    if !valid_id(experiment_id) {
        return Err(ExperimentReadError::InvalidId);
    }
    let Some(lane) = &state.admin_sqlite_lane else {
        return Err(ExperimentReadError::SqliteUnavailable);
    };
    let events = lane
        .reader()
        .list_experiment_events(experiment_id, MAX_EVENTS);
    let Some(experiment) = events
        .iter()
        .find(|event| event["event_type"] == "experiment.created")
        .cloned()
    else {
        return Err(ExperimentReadError::NotFound);
    };

    let mut runs = BTreeMap::<String, Value>::new();
    let mut judge_results = Vec::new();
    for event in &events {
        match event["event_type"].as_str().unwrap_or_default() {
            value if value.starts_with("experiment.run.") => {
                if let Some(run_id) = event["run_id"].as_str() {
                    runs.insert(run_id.to_owned(), event.clone());
                }
            }
            "experiment.judge.result" => judge_results.push(event.clone()),
            _ => {}
        }
    }
    let runs = runs.into_values().collect::<Vec<_>>();
    let session_dag = session_dag(&runs);
    let metrics = summary_metrics(&runs, &judge_results);

    Ok(json!({
        "experiment": experiment,
        "runs": runs,
        "session_dag": session_dag,
        "judge_results": judge_results,
        "metrics": metrics,
        "events": events,
    }))
}

fn validate_create(body: &CreateExperimentBody) -> Result<(), &'static str> {
    if body.name.trim().is_empty()
        || body.name.len() > 256
        || !valid_id(&body.scenario_id)
        || body.description.len() > 2_048
        || body
            .workflow_id
            .as_deref()
            .is_some_and(|value| !valid_id(value))
        || body
            .recording_id
            .as_deref()
            .is_some_and(|value| !valid_id(value))
        || body.tags.len() > 32
        || body
            .tags
            .iter()
            .any(|value| value.is_empty() || value.len() > 64)
        || !bounded_json(&body.metadata)
    {
        return Err("invalid or oversized experiment definition");
    }
    Ok(())
}

fn session_dag(runs: &[Value]) -> Value {
    let nodes = runs
        .iter()
        .map(|run| {
            json!({
                "run_id": run["run_id"],
                "session_id": run["session_id"],
                "parent_run_id": run["parent_run_id"],
                "parent_session_id": run["parent_session_id"],
                "status": run["status"],
            })
        })
        .collect::<Vec<_>>();
    let edges = runs
        .iter()
        .filter_map(|run| {
            let from = run["parent_run_id"]
                .as_str()
                .or_else(|| run["parent_session_id"].as_str())?;
            Some(json!({
                "from": from,
                "to": run["run_id"].as_str().unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();
    json!({"nodes": nodes, "edges": edges})
}

fn summary_metrics(runs: &[Value], judges: &[Value]) -> Value {
    let mut run_counts = Map::new();
    let mut judge_counts = Map::new();
    for run in runs {
        increment(&mut run_counts, run["status"].as_str().unwrap_or("unknown"));
    }
    for judge in judges {
        increment(
            &mut judge_counts,
            judge["status"].as_str().unwrap_or("unknown"),
        );
    }
    run_counts.insert("total".into(), json!(runs.len()));
    judge_counts.insert("total".into(), json!(judges.len()));
    json!({"runs": run_counts, "judges": judge_counts})
}

fn increment(counts: &mut Map<String, Value>, key: &str) {
    let next = counts.get(key).and_then(Value::as_u64).unwrap_or(0) + 1;
    counts.insert(key.to_owned(), json!(next));
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn bounded_json(value: &Value) -> bool {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() <= MAX_METADATA_BYTES)
        .unwrap_or(false)
}

fn valid_evidence(values: &[String]) -> bool {
    values.len() <= 32
        && values
            .iter()
            .all(|value| !value.is_empty() && value.len() <= 2_048)
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
