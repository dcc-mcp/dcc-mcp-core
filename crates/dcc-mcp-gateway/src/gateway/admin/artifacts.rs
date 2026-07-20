//! Artifact verification API handler.
//!
//! Provides `GET /admin/api/artifacts` — aggregates artifact data from
//! trace/audit logs and the dcc-mcp-artefact store.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

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
                    artifacts.push(fr);
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
                    // Deduplicate by URI
                    let uri = fr.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                    if !artifacts.iter().any(|a| {
                        a.get("uri")
                            .and_then(|v| v.as_str())
                            .is_some_and(|u| u == uri)
                    }) {
                        artifacts.push(fr);
                    }
                }
            }
        }
    }

    // 3. Enrich with verification status from artefact store (best-effort)
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
/// Recognises JSON payloads that contain `files`, `artifacts`, `file_refs`,
/// or a single file-like object with a `uri` field.
fn extract_file_refs(output: &Value) -> Vec<Value> {
    let mut refs = Vec::new();

    // Case 1: { "files": [...] }
    if let Some(files) = output.get("files").and_then(|v| v.as_array()) {
        for f in files {
            if f.get("uri").and_then(|v| v.as_str()).is_some() {
                refs.push(f.clone());
            }
        }
    }

    // Case 2: { "artifacts": [...] }
    if let Some(arts) = output.get("artifacts").and_then(|v| v.as_array()) {
        for a in arts {
            if a.get("uri").and_then(|v| v.as_str()).is_some() {
                refs.push(a.clone());
            }
        }
    }

    // Case 3: { "file_refs": [...] }
    if let Some(frs) = output.get("file_refs").and_then(|v| v.as_array()) {
        for fr in frs {
            if fr.get("uri").and_then(|v| v.as_str()).is_some() {
                refs.push(fr.clone());
            }
        }
    }

    // Case 4: output itself is a file ref { "uri": "...", ... }
    if refs.is_empty() && output.get("uri").and_then(|v| v.as_str()).is_some() {
        refs.push(output.clone());
    }

    refs
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
