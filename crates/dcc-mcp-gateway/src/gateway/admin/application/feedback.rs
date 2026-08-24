//! Persisted agent-feedback aggregation for the gateway admin API.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::super::state::AdminState;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1_000;
const MAX_FILES: usize = 1_024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_SCAN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MATCHING_ENTRIES: usize = 100_000;
const VALID_SEVERITIES: &[&str] = &[
    "blocked",
    "blocker",
    "degraded",
    "workaround_found",
    "suggestion",
];

#[derive(Debug, Default, Deserialize)]
pub(super) struct FeedbackQuery {
    range: Option<String>,
    dcc: Option<String>,
    severity: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Clone)]
struct ValidatedQuery {
    range: String,
    cutoff: Option<f64>,
    dcc: Option<String>,
    severity: Option<String>,
    limit: usize,
}

#[derive(Debug)]
struct FeedbackEntry {
    id: String,
    timestamp: f64,
    value: Value,
}

#[derive(Debug, Default)]
struct FeedbackScan {
    entries: Vec<Value>,
    total: usize,
    skipped_invalid: usize,
    deduplicated: usize,
    files_scanned: usize,
}

/// `GET /admin/api/feedback` — aggregate bounded per-instance feedback JSONL.
pub(super) async fn handle_admin_feedback(
    State(state): State<AdminState>,
    Query(raw_query): Query<FeedbackQuery>,
) -> Response {
    let query = match validate_query(raw_query) {
        Ok(query) => query,
        Err(message) => return query_error(message),
    };
    let feedback_dir = state.gateway.registry.registry_dir().join("feedback");
    let scan_query = query.clone();
    let scan = match tokio::task::spawn_blocking(move || scan_feedback(&feedback_dir, &scan_query))
        .await
    {
        Ok(Ok(scan)) => scan,
        Ok(Err(message)) => return read_error(message),
        Err(error) => return read_error(format!("feedback scan task failed: {error}")),
    };

    Json(json!({
        "success": true,
        "source": "registry-jsonl",
        "total": scan.total,
        "count": scan.entries.len(),
        "truncated": scan.total > scan.entries.len(),
        "skipped_invalid": scan.skipped_invalid,
        "deduplicated": scan.deduplicated,
        "files_scanned": scan.files_scanned,
        "filters": {
            "range": query.range,
            "dcc": query.dcc,
            "severity": query.severity,
            "limit": query.limit,
        },
        "entries": scan.entries,
    }))
    .into_response()
}

fn validate_query(raw: FeedbackQuery) -> Result<ValidatedQuery, String> {
    let range = raw.range.unwrap_or_else(|| "7d".to_string());
    let range_secs = match range.as_str() {
        "1h" => Some(60.0 * 60.0),
        "24h" => Some(24.0 * 60.0 * 60.0),
        "7d" => Some(7.0 * 24.0 * 60.0 * 60.0),
        "all" => None,
        _ => return Err("range must be one of: 1h, 24h, 7d, all".to_string()),
    };
    let limit = raw.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(format!("limit must be between 1 and {MAX_LIMIT}"));
    }
    let dcc = normalize_filter("dcc", raw.dcc)?;
    let severity = normalize_filter("severity", raw.severity)?;
    if let Some(severity) = &severity
        && !VALID_SEVERITIES.contains(&severity.as_str())
    {
        return Err(format!(
            "severity must be one of: {}",
            VALID_SEVERITIES.join(", ")
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
        .as_secs_f64();
    Ok(ValidatedQuery {
        range,
        cutoff: range_secs.map(|seconds| now - seconds),
        dcc,
        severity,
        limit,
    })
}

fn normalize_filter(name: &str, value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    if value.len() > 256 {
        return Err(format!("{name} exceeds the 256-byte limit"));
    }
    Ok(Some(value))
}

fn scan_feedback(feedback_dir: &Path, query: &ValidatedQuery) -> Result<FeedbackScan, String> {
    if !feedback_dir.exists() {
        return Ok(FeedbackScan::default());
    }
    let directory = std::fs::read_dir(feedback_dir)
        .map_err(|error| format!("could not read feedback directory: {error}"))?;
    let mut files = Vec::new();
    let mut total_scan_bytes = 0_u64;
    for entry in directory {
        let entry =
            entry.map_err(|error| format!("could not read feedback directory row: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect feedback file type: {error}"))?;
        if !file_type.is_file() || !is_feedback_file(&entry.file_name().to_string_lossy()) {
            continue;
        }
        if files.len() == MAX_FILES {
            return Err(format!(
                "feedback directory exceeds the bounded {MAX_FILES}-file scan limit"
            ));
        }
        let file_bytes = entry
            .metadata()
            .map_err(|error| format!("could not inspect feedback file size: {error}"))?
            .len();
        if file_bytes > MAX_FILE_BYTES {
            return Err(format!(
                "feedback file {} exceeds the bounded {MAX_FILE_BYTES}-byte scan limit",
                entry.file_name().to_string_lossy()
            ));
        }
        total_scan_bytes = total_scan_bytes
            .checked_add(file_bytes)
            .ok_or_else(|| "feedback scan size overflowed".to_string())?;
        if total_scan_bytes > MAX_TOTAL_SCAN_BYTES {
            return Err(format!(
                "feedback files exceed the bounded {MAX_TOTAL_SCAN_BYTES}-byte scan limit"
            ));
        }
        files.push((entry.path(), file_bytes));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut candidates = Vec::new();
    let mut skipped_invalid = 0;
    for (path, snapshot_bytes) in &files {
        scan_file(
            path,
            *snapshot_bytes,
            query,
            &mut candidates,
            &mut skipped_invalid,
        )?;
    }
    candidates.sort_by(|left, right| {
        right
            .timestamp
            .partial_cmp(&left.timestamp)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut seen = HashSet::new();
    let mut deduplicated = 0;
    candidates.retain(|entry| {
        if seen.insert(entry.id.clone()) {
            true
        } else {
            deduplicated += 1;
            false
        }
    });
    let total = candidates.len();
    let entries = candidates
        .into_iter()
        .take(query.limit)
        .map(|entry| entry.value)
        .collect();
    Ok(FeedbackScan {
        entries,
        total,
        skipped_invalid,
        deduplicated,
        files_scanned: files.len(),
    })
}

fn scan_file(
    path: &Path,
    snapshot_bytes: u64,
    query: &ValidatedQuery,
    entries: &mut Vec<FeedbackEntry>,
    skipped_invalid: &mut usize,
) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>");
    let file = File::open(path)
        .map_err(|error| format!("could not open feedback file {file_name}: {error}"))?;
    let mut reader = BufReader::new(file.take(snapshot_bytes));
    let source_dcc = feedback_file_dcc(path);
    while let Some(line) = read_bounded_line(&mut reader)
        .map_err(|error| format!("could not read feedback file {file_name}: {error}"))?
    {
        let Ok(line) = line else {
            *skipped_invalid += 1;
            continue;
        };
        let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&line) else {
            *skipped_invalid += 1;
            continue;
        };
        match normalize_record(record, source_dcc.as_deref(), query) {
            Err(()) => *skipped_invalid += 1,
            Ok(None) => {}
            Ok(Some(entry)) => {
                if entries.len() == MAX_MATCHING_ENTRIES {
                    return Err(format!(
                        "feedback query exceeds the bounded {MAX_MATCHING_ENTRIES}-record scan limit"
                    ));
                }
                entries.push(entry);
            }
        }
    }
    Ok(())
}

fn normalize_record(
    mut record: Map<String, Value>,
    source_dcc: Option<&str>,
    query: &ValidatedQuery,
) -> Result<Option<FeedbackEntry>, ()> {
    if record.get("dcc_type").and_then(Value::as_str).is_none()
        && let Some(source_dcc) = source_dcc
    {
        record.insert(
            "dcc_type".to_string(),
            Value::String(source_dcc.to_string()),
        );
    }
    let Some(id) = record.get("id").and_then(Value::as_str) else {
        return Err(());
    };
    if id.trim().is_empty() {
        return Err(());
    }
    let id = id.to_string();
    let Some(timestamp) = record.get("timestamp").and_then(Value::as_f64) else {
        return Err(());
    };
    if !timestamp.is_finite() {
        return Err(());
    }
    if query.cutoff.is_some_and(|cutoff| timestamp < cutoff) {
        return Ok(None);
    }
    if let Some(dcc) = &query.dcc
        && record
            .get("dcc_type")
            .and_then(Value::as_str)
            .is_none_or(|value| !value.eq_ignore_ascii_case(dcc))
    {
        return Ok(None);
    }
    if let Some(severity) = &query.severity
        && record
            .get("severity")
            .and_then(Value::as_str)
            .is_none_or(|value| !value.eq_ignore_ascii_case(severity))
    {
        return Ok(None);
    }
    Ok(Some(FeedbackEntry {
        id,
        timestamp,
        value: Value::Object(record),
    }))
}

fn read_bounded_line(reader: &mut impl BufRead) -> io::Result<Option<Result<Vec<u8>, ()>>> {
    let mut line = Vec::new();
    let mut saw_bytes = false;
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            if line.len() + consumed <= MAX_LINE_BYTES {
                line.extend_from_slice(&available[..consumed]);
            } else {
                line.clear();
                oversized = true;
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    if !saw_bytes {
        Ok(None)
    } else if oversized {
        Ok(Some(Err(())))
    } else {
        Ok(Some(Ok(line)))
    }
}

fn is_feedback_file(name: &str) -> bool {
    if name.ends_with(".jsonl") {
        return true;
    }
    name.rsplit_once(".jsonl.").is_some_and(|(_, suffix)| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn feedback_file_dcc(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let base = name
        .strip_suffix(".jsonl")
        .or_else(|| name.rsplit_once(".jsonl.").map(|(base, _)| base))?;
    let (dcc, pid) = base.rsplit_once('-')?;
    if dcc.is_empty() || pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(dcc.to_ascii_lowercase())
}

fn query_error(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "error": {"kind": "invalid-feedback-query", "message": message},
        })),
    )
        .into_response()
}

fn read_error(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "success": false,
            "error": {"kind": "feedback-read-failed", "message": message},
        })),
    )
        .into_response()
}
