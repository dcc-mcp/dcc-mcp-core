//! Artifact verification API handler.
//!
//! Provides `GET /admin/api/artifacts` — aggregates artifact data from
//! trace/audit logs and the dcc-mcp-artefact store.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;

use super::state::AdminState;

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

    // Collect artifacts from audit records and traces.
    // An artifact is identified when a trace/audit entry has output payloads
    // that contain file references (URI, MIME, size).
    let mut artifacts: Vec<Value> = Vec::new();

    // 1. From trace log ring buffer
    if let Some(trace_log) = &s.trace_log {
        for trace in trace_log.recent(limit) {
            if let Some(output) = &trace.output
                && let Ok(parsed) = serde_json::from_str::<Value>(&output.content)
            {
                let file_refs = extract_file_refs(&parsed);
                for fr in file_refs {
                    artifacts.push(with_dcc_type(fr, trace.dcc_type.as_deref()));
                }
            }
        }
    }

    // 2. From SQLite (traces table)
    if let Some(ref lane) = s.admin_sqlite_lane {
        let reader = lane.reader();
        let traces = reader.list_traces_since(None, limit);
        for trace in traces {
            if let Some(output) = &trace.output
                && let Ok(parsed) = serde_json::from_str::<Value>(&output.content)
            {
                let file_refs = extract_file_refs(&parsed);
                for fr in file_refs {
                    let fr = with_dcc_type(fr, trace.dcc_type.as_deref());
                    if !artifacts
                        .iter()
                        .any(|existing| same_artifact_identity(existing, &fr))
                    {
                        artifacts.push(fr);
                    }
                }
            }
        }
    }

    // 3. From UI Control's existing redacted file audit stream. Direct
    // instance calls do not necessarily traverse the gateway trace middleware.
    let log_dir = std::env::var(ENV_DCC_MCP_LOG_DIR)
        .unwrap_or_else(|_| dcc_mcp_db::default_gateway_log_dir());
    if let Ok(rows) =
        tokio::task::spawn_blocking(move || read_gateway_log_dir_rows_recent(&log_dir, limit)).await
    {
        for row in rows {
            let dcc_type = row.get("dcc_type").and_then(Value::as_str);
            for fr in extract_file_refs(&row) {
                let fr = with_dcc_type(fr, dcc_type);
                if !artifacts
                    .iter()
                    .any(|existing| same_artifact_identity(existing, &fr))
                {
                    artifacts.push(fr);
                }
            }
        }
    }

    // 4. Enrich with verification status from artefact store (best-effort)
    let mut verified_count = 0usize;
    let mut unverified_count = 0usize;
    let mut failed_count = 0usize;

    for artifact in &mut artifacts {
        let verification = check_artifact_verification(artifact);
        let status = verification
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unverified");
        match status {
            "verified" => verified_count += 1,
            "failed" => failed_count += 1,
            _ => unverified_count += 1,
        }
        if let Some(obj) = artifact.as_object_mut() {
            obj.insert("verification".to_string(), verification);
        }
    }

    // Apply filters
    if let Some(ref dcc_filter) = params.dcc_type {
        artifacts.retain(|a| {
            a.get("dcc_type")
                .and_then(|v| v.as_str())
                .is_some_and(|d| d.eq_ignore_ascii_case(dcc_filter))
        });
    }
    if let Some(ref status_filter) = params.status {
        artifacts.retain(|a| {
            a.get("verification")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(status_filter))
        });
    }

    artifacts.truncate(limit);

    let total = artifacts.len();
    if total == 0 && verified_count == 0 && unverified_count == 0 {
        // If no artifacts were found from traces, include a placeholder with
        // gateway health data so the frontend panel is not empty.
        return Json(json!({
            "total": 0,
            "artifacts": [],
            "summary": {
                "verified": 0,
                "unverified": 0,
                "failed": 0,
            },
            "message": "No artifacts found. Artifacts are derived from tool-call output payloads containing file references.",
        }))
        .into_response();
    }

    Json(json!({
        "total": total,
        "artifacts": artifacts,
        "summary": {
            "verified": verified_count,
            "unverified": unverified_count,
            "failed": failed_count,
        },
    }))
    .into_response()
}

/// Extract file references from a tool-call output payload.
///
/// Recognises wrapped or direct JSON payloads containing `files`, `artifacts`,
/// `file_refs`, or a single file-like object with a `uri` field.
fn extract_file_refs(output: &Value) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_file_refs(output, &mut refs, true, false);
    refs
}

fn collect_file_refs(value: &Value, refs: &mut Vec<Value>, root: bool, file_collection: bool) {
    match value {
        Value::Object(object) => {
            if (root || file_collection) && value.get("uri").and_then(Value::as_str).is_some() {
                push_file_ref(value, refs);
                return;
            }
            for (key, child) in object {
                collect_file_refs(
                    child,
                    refs,
                    false,
                    matches!(key.as_str(), "files" | "artifacts" | "file_refs"),
                );
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_file_refs(item, refs, false, file_collection);
            }
        }
        _ => {}
    }
}

fn push_file_ref(value: &Value, refs: &mut Vec<Value>) {
    if !refs
        .iter()
        .any(|existing| same_artifact_identity(existing, value))
    {
        refs.push(value.clone());
    }
}

fn same_artifact_identity(left: &Value, right: &Value) -> bool {
    ["uri", "session_id", "correlation_id"]
        .into_iter()
        .all(|key| left.get(key) == right.get(key))
}

fn with_dcc_type(mut artifact: Value, dcc_type: Option<&str>) -> Value {
    if let (Some(object), Some(dcc_type)) = (artifact.as_object_mut(), dcc_type) {
        object.insert("dcc_type".to_string(), json!(dcc_type));
    }
    artifact
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nested_artifacts_once_for_admin_tracing() {
        let value = json!({
            "result": {
                "structuredContent": {
                    "context": {
                        "artifacts": [{
                            "uri": "artefact://sha256/abc",
                            "display_name": "ui-control-snapshot-session-snapshot.png",
                            "digest": "sha256:abc",
                            "session_id": "session-a",
                            "correlation_id": "snapshot-a"
                        }]
                    }
                }
            },
            "duplicate": {"artifacts": [{
                "uri": "artefact://sha256/abc",
                "session_id": "session-a",
                "correlation_id": "snapshot-a"
            }]},
            "same_pixels_new_capture": {"artifacts": [{
                "uri": "artefact://sha256/abc",
                "session_id": "session-b",
                "correlation_id": "snapshot-b"
            }]}
        });

        let artifacts = extract_file_refs(&value);

        assert_eq!(artifacts.len(), 2);
        assert_eq!(
            artifacts[0]["display_name"],
            "ui-control-snapshot-session-snapshot.png"
        );
        assert_eq!(
            with_dcc_type(artifacts[0].clone(), Some("unity"))["dcc_type"],
            "unity"
        );
    }

    #[test]
    fn extracts_ui_control_artifact_from_file_audit_row() {
        let row = json!({
            "dcc_type": "unity",
            "artifacts": [{
                "uri": "artefact://sha256/abc",
                "display_name": "ui-control-snapshot-session-snapshot.png",
                "session_id": "session-a",
                "correlation_id": "snapshot-a"
            }]
        });

        let artifact = with_dcc_type(extract_file_refs(&row).remove(0), row["dcc_type"].as_str());
        assert_eq!(artifact["dcc_type"], "unity");
    }
}

/// Check verification status of an artifact.
///
/// Currently a best-effort check: if the artifact has a `sha256` field,
/// we mark it as "verified"; if it has an `error` field, "failed";
/// otherwise "unverified".
///
/// Future: integrate with `dcc-mcp-artefact::ArtefactStore` to check
/// on-disk file existence and SHA-256 integrity.
fn check_artifact_verification(artifact: &Value) -> Value {
    if artifact.get("error").is_some() {
        return json!({
            "status": "failed",
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "reason": artifact.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error"),
        });
    }

    if artifact.get("sha256").is_some() || artifact.get("digest").is_some() {
        return json!({
            "status": "verified",
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "method": "sha256_metadata",
        });
    }

    json!({
        "status": "unverified",
        "checked_at": chrono::Utc::now().to_rfc3339(),
        "reason": "no integrity metadata available",
    })
}
