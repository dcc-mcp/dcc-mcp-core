//! Explicit mutable recording lifecycle and read-only review projection.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use dcc_mcp_workflow::{
    CompileOptions, RecordedEvent, RecordingManifest, RecordingTarget, ReplayToolGuard,
    compile_recording, schema_fingerprint,
};

use crate::gateway::admin::trace::AgentContext;
use crate::gateway::record_replay::{RecordingStoreError, StartRecording};

use super::state::AdminState;

#[derive(Debug, Deserialize)]
pub struct StartBody {
    dcc_type: String,
    #[serde(default)]
    instance_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StopBody {
    recording_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompileBody {
    recording_id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    inputs: Value,
    /// Explicit review acknowledgement; compilation never implies replay approval.
    #[serde(default)]
    reviewed: bool,
}

#[derive(Debug, Deserialize)]
pub struct ReplayValidateBody {
    guards: Vec<ReplayToolGuard>,
}

/// `POST /v1/recordings/start` — grant capture for the current trusted caller session.
pub async fn handle_recording_start(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<StartBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    };
    match state.recordings.start(StartRecording {
        session_id,
        dcc_type: body.dcc_type,
        instance_id: body.instance_id,
    }) {
        Ok(draft) => {
            state.ensure_recording_subscription();
            persist_recording_event(&state, "recording.started", &draft, None);
            (StatusCode::CREATED, Json(json!(draft))).into_response()
        }
        Err(error) => store_error(error),
    }
}

/// `POST /v1/recordings/stop` — stop only the current caller's recording.
pub async fn handle_recording_stop(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<StopBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    };
    let response = match state.recordings.stop(&session_id, &body.recording_id) {
        Ok(draft) => {
            for event in &draft.events {
                persist_recording_event(&state, "recording.tool_call", &draft, Some(json!(event)));
            }
            persist_recording_event(&state, "recording.stopped", &draft, None);
            (StatusCode::OK, Json(json!(draft))).into_response()
        }
        Err(error) => store_error(error),
    };
    state.release_recording_subscription_if_idle();
    response
}

/// `GET /v1/recordings/{recording_id}` — review only within the caller namespace.
pub async fn handle_recording_review(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(recording_id): Path<String>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    };
    match state.recordings.get(&session_id, &recording_id) {
        Some(draft) => (StatusCode::OK, Json(json!(draft))).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            "recording_not_found",
            "Recording does not exist for this caller session.",
        ),
    }
}

/// `POST /v1/recordings/review` — body form for CLI/VRS clients.
pub async fn handle_recording_review_body(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<StopBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    };
    match state.recordings.get(&session_id, &body.recording_id) {
        Some(draft) => (StatusCode::OK, Json(json!(draft))).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            "recording_not_found",
            "Recording does not exist for this caller session.",
        ),
    }
}

/// `POST /v1/recordings/compile` — resolve current schemas and draft a local Skill.
pub async fn handle_recording_compile(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CompileBody>,
) -> impl IntoResponse {
    let Some(session_id) = caller_session(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    };
    if !body.reviewed {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "review_required",
            "Review the stopped recording and set reviewed=true before compilation.",
        );
    }
    let Some(draft) = state.recordings.get(&session_id, &body.recording_id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "recording_not_found",
            "Recording does not exist for this caller session.",
        );
    };
    if draft.status == "recording" {
        return error_response(
            StatusCode::CONFLICT,
            "recording_active",
            "Stop the recording before compilation.",
        );
    }

    let snapshot = state.gateway.capability_index.snapshot();
    let mut events = Vec::with_capacity(draft.events.len());
    let mut ui_queries: HashMap<String, Value> = HashMap::new();
    for captured in &draft.events {
        if captured.success == Some(false) {
            continue;
        }
        if captured.success.is_none() {
            return compile_error(
                "recording_call_unverified",
                captured
                    .request_id
                    .as_deref()
                    .unwrap_or(&captured.tool_slug),
            );
        }
        let Some((captured_dcc, _, captured_tool)) =
            crate::gateway::capability::parse_slug(&captured.tool_slug)
        else {
            return compile_error("invalid_recorded_slug", &captured.tool_slug);
        };
        let matches = snapshot
            .records
            .iter()
            .filter(|record| {
                record.loaded
                    && record.dcc_type.eq_ignore_ascii_case(captured_dcc)
                    && record.callable_id == captured_tool
            })
            .collect::<Vec<_>>();
        let [current] = matches.as_slice() else {
            let code = if matches.is_empty() {
                "tool_missing_or_unloaded"
            } else {
                "tool_ambiguous"
            };
            return compile_error(code, &captured.tool_slug);
        };
        if captured_tool == "ui_control__snapshot"
            || captured_tool == "ui_control__stop_computer_use"
        {
            continue;
        }
        if captured_tool == "ui_control__find" {
            let session = logical_ui_session(&captured.arguments);
            ui_queries.insert(session, semantic_query(&captured.arguments));
            continue;
        }
        if captured_tool == "ui_control__wait_for" {
            continue;
        }
        if captured_tool == "ui_control__act" {
            let action = captured
                .arguments
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if ![
                "click",
                "set_text",
                "toggle",
                "set_checked",
                "select_option",
                "focus",
            ]
            .contains(&action)
            {
                return compile_error("raw_ui_action_requires_visual_guard", &captured.tool_slug);
            }
            let session = logical_ui_session(&captured.arguments);
            let Some(query) = ui_queries.get(&session).cloned() else {
                return compile_error(
                    "semantic_query_missing",
                    "ui_control__act must follow a successful ui_control__find in the same session",
                );
            };
            let postcondition = default_postcondition(&query);
            events.push(RecordedEvent::UiSemanticAction {
                sequence: captured.sequence,
                query,
                action: action.to_owned(),
                value: captured.arguments.get("value").cloned(),
                postcondition,
            });
            continue;
        }
        let (record, tool) = match crate::gateway::capability_service::describe_tool_full(
            &state.gateway,
            &current.tool_slug,
        )
        .await
        {
            Ok(resolved) => resolved,
            Err(error) => return compile_error(&error.kind, &error.message),
        };
        events.push(RecordedEvent::ToolCall {
            sequence: captured.sequence,
            tool: record.callable_id,
            dcc_type: record.dcc_type,
            arguments_template: captured.arguments.clone(),
            schema_fingerprint: schema_fingerprint(&tool.input_schema),
            success: true,
        });
    }
    let manifest = RecordingManifest {
        version: 1,
        recording_id: draft.recording_id,
        session_namespace: draft.session_id,
        target: RecordingTarget {
            dcc_type: draft.dcc_type,
            instance_id: draft.instance_id,
        },
        events,
    };
    match compile_recording(
        &manifest,
        &CompileOptions {
            name: body.name,
            description: body.description,
            inputs: body.inputs,
        },
    ) {
        Ok(compiled) => (
            StatusCode::OK,
            Json(json!({
                "recording": manifest,
                "compiled": compiled,
                "replay_authorized": false,
                "publish_authorized": false,
            })),
        )
            .into_response(),
        Err(error) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "recording_compile_failed",
            &error.to_string(),
        ),
    }
}

/// `POST /v1/recordings/replay/validate` — fail closed on missing, ambiguous, or drifted tools.
pub async fn handle_recording_replay_validate(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ReplayValidateBody>,
) -> impl IntoResponse {
    if caller_session(&headers).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session_required",
            "A bounded x-dcc-mcp-agent-session-id header is required.",
        );
    }
    if body.guards.is_empty() || body.guards.len() > 100 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_replay_guards",
            "Replay requires 1..=100 bounded tool schema guards.",
        );
    }
    let snapshot = state.gateway.capability_index.snapshot();
    let mut validated = Vec::with_capacity(body.guards.len());
    for guard in body.guards {
        let matches = snapshot
            .records
            .iter()
            .filter(|record| {
                record.loaded
                    && record.dcc_type.eq_ignore_ascii_case(&guard.dcc_type)
                    && record.callable_id == guard.tool
            })
            .collect::<Vec<_>>();
        let [current] = matches.as_slice() else {
            return compile_error(
                if matches.is_empty() {
                    "tool_missing_or_unloaded"
                } else {
                    "tool_ambiguous"
                },
                &format!("{}:{}", guard.dcc_type, guard.tool),
            );
        };
        let (_, tool) = match crate::gateway::capability_service::describe_tool_full(
            &state.gateway,
            &current.tool_slug,
        )
        .await
        {
            Ok(resolved) => resolved,
            Err(error) => return compile_error(&error.kind, &error.message),
        };
        let current_fingerprint = schema_fingerprint(&tool.input_schema);
        if current_fingerprint != guard.schema_fingerprint {
            return error_response(
                StatusCode::CONFLICT,
                "tool_schema_drift",
                &format!("Replay guard failed for {}:{}.", guard.dcc_type, guard.tool),
            );
        }
        validated.push(json!({
            "dcc_type": guard.dcc_type,
            "tool": guard.tool,
            "tool_slug": current.tool_slug,
            "schema_fingerprint": current_fingerprint,
        }));
    }
    (
        StatusCode::OK,
        Json(json!({"validated": validated, "replay_authorized": false})),
    )
        .into_response()
}

fn logical_ui_session(arguments: &Value) -> String {
    arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_owned()
}

fn semantic_query(arguments: &Value) -> Value {
    let mut query = serde_json::Map::new();
    for key in ["query", "role", "label", "object_name"] {
        if let Some(value) = arguments.get(key) {
            query.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(query)
}

fn default_postcondition(query: &Value) -> Value {
    let mut condition = serde_json::Map::from_iter([(
        "kind".to_owned(),
        Value::String("control_exists".to_owned()),
    )]);
    if let Some(fields) = query.as_object() {
        condition.extend(fields.clone());
    }
    Value::Object(condition)
}

fn caller_session(headers: &HeaderMap) -> Option<String> {
    AgentContext::from_request_parts_with_server_network(headers, None, None)
        .and_then(|context| context.session_id)
        .filter(|value| !value.trim().is_empty() && value.len() <= 256)
}

fn store_error(error: RecordingStoreError) -> axum::response::Response {
    match error {
        RecordingStoreError::AlreadyRecording => {
            error_response(StatusCode::CONFLICT, "recording_active", &error.to_string())
        }
        RecordingStoreError::NotFound => error_response(
            StatusCode::NOT_FOUND,
            "recording_not_found",
            &error.to_string(),
        ),
        RecordingStoreError::InvalidField(_) => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_recording_request",
            &error.to_string(),
        ),
    }
}

fn error_response(status: StatusCode, error: &str, message: &str) -> axum::response::Response {
    (status, Json(json!({"error": error, "message": message}))).into_response()
}

fn compile_error(error: &str, detail: &str) -> axum::response::Response {
    error_response(
        StatusCode::CONFLICT,
        error,
        &format!("Recording cannot compile safely: {detail}"),
    )
}

fn persist_recording_event(
    state: &AdminState,
    event_type: &str,
    draft: &crate::gateway::record_replay::RecordingDraft,
    payload: Option<Value>,
) {
    let Some(lane) = &state.admin_sqlite_lane else {
        return;
    };
    let created_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    lane.try_persist_session_event(&json!({
        "session_id": draft.session_id,
        "event_type": event_type,
        "created_at_ms": created_at_ms,
        "recording_id": draft.recording_id,
        "dcc_type": draft.dcc_type,
        "status": draft.status,
        "payload": payload,
    }));
}
