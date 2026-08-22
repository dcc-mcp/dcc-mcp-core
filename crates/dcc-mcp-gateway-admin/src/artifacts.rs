//! Artifact extraction and verification projections for the admin dashboard.

use serde_json::{Value, json};

/// Optional filters applied to the admin artifact projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactFilter {
    pub dcc_type: Option<String>,
    pub status: Option<String>,
}

/// Extract file references from a tool output and attach its DCC type.
#[must_use]
pub fn artifact_refs(output: &Value, dcc_type: Option<&str>) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_file_refs(output, &mut refs, true, false);
    refs.into_iter()
        .map(|artifact| with_dcc_type(artifact, dcc_type))
        .collect()
}

/// Build the filtered artifact verification payload returned by the admin API.
#[must_use]
pub fn artifact_payload(artifacts: Vec<Value>, filter: &ArtifactFilter, limit: usize) -> Value {
    let mut artifacts = deduplicate_artifacts(artifacts);
    let mut verified_count = 0usize;
    let mut unverified_count = 0usize;
    let mut failed_count = 0usize;

    for artifact in &mut artifacts {
        let verification = artifact_verification(artifact);
        match verification
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unverified")
        {
            "verified" => verified_count += 1,
            "failed" => failed_count += 1,
            _ => unverified_count += 1,
        }
        if let Some(object) = artifact.as_object_mut() {
            object.insert("verification".to_string(), verification);
        }
    }

    if let Some(dcc_filter) = &filter.dcc_type {
        artifacts.retain(|artifact| {
            artifact
                .get("dcc_type")
                .and_then(Value::as_str)
                .is_some_and(|dcc| dcc.eq_ignore_ascii_case(dcc_filter))
        });
    }
    if let Some(status_filter) = &filter.status {
        artifacts.retain(|artifact| {
            artifact
                .pointer("/verification/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.eq_ignore_ascii_case(status_filter))
        });
    }
    artifacts.truncate(limit.clamp(1, 500));

    let total = artifacts.len();
    if total == 0 && verified_count == 0 && unverified_count == 0 {
        return json!({
            "total": 0,
            "artifacts": [],
            "summary": {
                "verified": 0,
                "unverified": 0,
                "failed": 0,
            },
            "message": "No artifacts found. Artifacts are derived from tool-call output payloads containing file references.",
        });
    }

    json!({
        "total": total,
        "artifacts": artifacts,
        "summary": {
            "verified": verified_count,
            "unverified": unverified_count,
            "failed": failed_count,
        },
    })
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

fn deduplicate_artifacts(artifacts: Vec<Value>) -> Vec<Value> {
    let mut unique = Vec::new();
    for artifact in artifacts {
        if !unique
            .iter()
            .any(|existing| same_artifact_identity(existing, &artifact))
        {
            unique.push(artifact);
        }
    }
    unique
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

fn artifact_verification(artifact: &Value) -> Value {
    if artifact.get("error").is_some() {
        return json!({
            "status": "failed",
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "reason": artifact.get("error").and_then(Value::as_str).unwrap_or("unknown error"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_artifacts_are_deduplicated_by_capture_identity() {
        let value = json!({
            "result": {"artifacts": [{
                "uri": "artefact://sha256/abc",
                "display_name": "snapshot.png",
                "digest": "sha256:abc",
                "session_id": "session-a",
                "correlation_id": "snapshot-a"
            }]},
            "duplicate": {"artifacts": [{
                "uri": "artefact://sha256/abc",
                "session_id": "session-a",
                "correlation_id": "snapshot-a"
            }]},
            "new_capture": {"artifacts": [{
                "uri": "artefact://sha256/abc",
                "session_id": "session-b",
                "correlation_id": "snapshot-b"
            }]}
        });

        let artifacts = artifact_refs(&value, Some("unity"));
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0]["display_name"], "snapshot.png");
        assert_eq!(artifacts[0]["dcc_type"], "unity");
    }

    #[test]
    fn payload_enriches_filters_and_summarizes_verification() {
        let payload = artifact_payload(
            vec![
                json!({"uri": "file:///verified", "dcc_type": "maya", "digest": "sha256:1"}),
                json!({"uri": "file:///failed", "dcc_type": "maya", "error": "missing"}),
                json!({"uri": "file:///plain", "dcc_type": "blender"}),
            ],
            &ArtifactFilter {
                dcc_type: Some("maya".to_string()),
                status: Some("verified".to_string()),
            },
            100,
        );

        assert_eq!(payload["total"], 1);
        assert_eq!(payload["artifacts"][0]["uri"], "file:///verified");
        assert_eq!(payload["summary"]["verified"], 1);
        assert_eq!(payload["summary"]["failed"], 1);
        assert_eq!(payload["summary"]["unverified"], 1);
    }

    #[test]
    fn empty_payload_keeps_operator_message() {
        let payload = artifact_payload(vec![], &ArtifactFilter::default(), 100);
        assert_eq!(payload["total"], 0);
        assert!(payload["message"].as_str().is_some());
    }
}
