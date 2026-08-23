//! Artifact verification API handler.
//!
//! Provides `GET /admin/api/artifacts` — aggregates artifact data from
//! trace/audit logs and delegates the pure projection to the admin domain.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::{ArtifactFilter, artifact_payload, artifact_refs};
use serde::Deserialize;
use serde_json::Value;

use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;

use super::super::state::AdminState;

#[derive(Debug, Default, Deserialize)]
pub struct ArtifactsQuery {
    /// Max rows (default 100).
    limit: Option<usize>,
    /// Filter by DCC type.
    dcc_type: Option<String>,
    /// Filter by verification status: "verified", "unverified", "failed".
    status: Option<String>,
}

/// `GET /admin/api/artifacts` — list artifacts derived from trace and audit data.
///
/// Aggregates tool-call outputs that produced file references (artifacts) and
/// enriches them with verification metadata from the artefact store.
///
/// Returns `{ artifacts: [...], total, summary: { verified, unverified, failed } }`.
pub async fn handle_admin_artifacts(
    State(s): State<AdminState>,
    Query(params): Query<ArtifactsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    let mut artifacts: Vec<Value> = Vec::new();

    if let Some(trace_log) = &s.trace_log {
        for trace in trace_log.recent(limit) {
            if let Some(output) = &trace.output
                && let Ok(parsed) = serde_json::from_str::<Value>(&output.content)
            {
                artifacts.extend(artifact_refs(&parsed, trace.dcc_type.as_deref()));
            }
        }
    }

    if let Some(ref lane) = s.admin_sqlite_lane {
        let reader = lane.reader();
        for trace in reader.list_traces_since(None, limit) {
            if let Some(output) = &trace.output
                && let Ok(parsed) = serde_json::from_str::<Value>(&output.content)
            {
                artifacts.extend(artifact_refs(&parsed, trace.dcc_type.as_deref()));
            }
        }
    }

    // Direct UI Control calls do not necessarily traverse gateway tracing, so
    // retain the existing redacted file-audit source at the infrastructure edge.
    let log_dir = std::env::var(ENV_DCC_MCP_LOG_DIR)
        .unwrap_or_else(|_| dcc_mcp_db::default_gateway_log_dir());
    if let Ok(rows) =
        tokio::task::spawn_blocking(move || read_gateway_log_dir_rows_recent(&log_dir, limit)).await
    {
        for row in rows {
            let dcc_type = row.get("dcc_type").and_then(Value::as_str);
            artifacts.extend(artifact_refs(&row, dcc_type));
        }
    }

    Json(artifact_payload(
        artifacts,
        &ArtifactFilter {
            dcc_type: params.dcc_type,
            status: params.status,
        },
        limit,
    ))
    .into_response()
}
