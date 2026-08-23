//! Pure experiment contracts and projections for the admin API.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

const MAX_METADATA_BYTES: usize = 16 * 1024;

/// Validate a persisted experiment identifier.
#[must_use]
pub fn valid_experiment_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Validate the bounded fields accepted when an experiment is created.
pub fn validate_experiment_definition(
    name: &str,
    scenario_id: &str,
    description: &str,
    workflow_id: Option<&str>,
    recording_id: Option<&str>,
    tags: &[String],
    metadata: &Value,
) -> Result<(), &'static str> {
    if name.trim().is_empty()
        || name.len() > 256
        || !valid_experiment_id(scenario_id)
        || description.len() > 2_048
        || workflow_id.is_some_and(|value| !valid_experiment_id(value))
        || recording_id.is_some_and(|value| !valid_experiment_id(value))
        || tags.len() > 32
        || tags
            .iter()
            .any(|value| value.is_empty() || value.len() > 64)
        || !bounded_json(metadata)
    {
        return Err("invalid or oversized experiment definition");
    }
    Ok(())
}

/// Validate the bounded fields accepted for an experiment run event.
pub fn validate_experiment_run(
    run_id: &str,
    status: &str,
    parent_run_id: Option<&str>,
    parent_session_id: Option<&str>,
    parameters: &Value,
    metrics: &Value,
    evidence: &[String],
) -> Result<(), &'static str> {
    if !valid_experiment_id(run_id)
        || !matches!(
            status,
            "pending" | "running" | "passed" | "failed" | "error" | "cancelled"
        )
        || parent_run_id.is_some_and(|value| !valid_experiment_id(value))
        || parent_session_id.is_some_and(|value| !valid_experiment_id(value))
        || !bounded_json(parameters)
        || !bounded_json(metrics)
        || !valid_evidence(evidence)
    {
        return Err("invalid or oversized experiment run payload");
    }
    Ok(())
}

/// Fields required to validate one judge-result event.
#[derive(Debug, Clone, Copy)]
pub struct ExperimentJudgeValidation<'a> {
    pub run_id: &'a str,
    pub evaluator_id: &'a str,
    pub evaluator_version: &'a str,
    pub rubric_hash: &'a str,
    pub status: &'a str,
    pub scores: &'a Value,
    pub summary: &'a str,
    pub cost: Option<f64>,
    pub evidence: &'a [String],
}

/// Validate the bounded fields accepted for an experiment judge result.
pub fn validate_experiment_judge(input: ExperimentJudgeValidation<'_>) -> Result<(), &'static str> {
    if !valid_experiment_id(input.run_id)
        || !valid_experiment_id(input.evaluator_id)
        || input.evaluator_version.is_empty()
        || input.evaluator_version.len() > 64
        || input.rubric_hash.is_empty()
        || input.rubric_hash.len() > 256
        || !matches!(input.status, "passed" | "failed" | "error")
        || input.summary.len() > 2_048
        || !bounded_json(input.scores)
        || !valid_evidence(input.evidence)
        || input
            .cost
            .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err("invalid or oversized judge result payload");
    }
    Ok(())
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

/// Project persisted experiment summaries into the list response.
#[must_use]
pub fn project_experiment_list(experiments: Vec<Value>) -> Value {
    json!({"total": experiments.len(), "experiments": experiments})
}

/// Project persisted experiment events into the detail response.
///
/// Returns `None` when the event stream does not contain an
/// `experiment.created` event.
#[must_use]
pub fn project_experiment_detail(events: Vec<Value>) -> Option<Value> {
    let experiment = events
        .iter()
        .find(|event| event["event_type"] == "experiment.created")
        .cloned()?;

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

    Some(json!({
        "experiment": experiment,
        "runs": runs,
        "session_dag": session_dag(&runs),
        "judge_results": judge_results,
        "metrics": summary_metrics(&runs, &judge_results),
        "events": events,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_projection_reports_total() {
        let payload = project_experiment_list(vec![json!({"experiment_id": "exp-1"})]);
        assert_eq!(payload["total"], 1);
        assert_eq!(payload["experiments"][0]["experiment_id"], "exp-1");
    }

    #[test]
    fn detail_projection_keeps_latest_run_and_builds_dag_and_metrics() {
        let events = vec![
            json!({"event_type": "experiment.created", "experiment_id": "exp-1"}),
            json!({
                "event_type": "experiment.run.running",
                "run_id": "run-1",
                "session_id": "maya-session",
                "status": "running"
            }),
            json!({
                "event_type": "experiment.run.passed",
                "run_id": "run-1",
                "session_id": "maya-session",
                "parent_session_id": "photoshop-session",
                "status": "passed"
            }),
            json!({
                "event_type": "experiment.judge.result",
                "run_id": "run-1",
                "status": "passed"
            }),
        ];

        let payload = project_experiment_detail(events).unwrap();
        assert_eq!(payload["runs"].as_array().unwrap().len(), 1);
        assert_eq!(payload["runs"][0]["status"], "passed");
        assert_eq!(
            payload["session_dag"]["edges"][0]["from"],
            "photoshop-session"
        );
        assert_eq!(payload["metrics"]["runs"]["passed"], 1);
        assert_eq!(payload["metrics"]["judges"]["passed"], 1);
    }

    #[test]
    fn detail_projection_requires_creation_event() {
        assert!(
            project_experiment_detail(vec![json!({
                "event_type": "experiment.run.passed",
                "run_id": "run-1"
            })])
            .is_none()
        );
    }

    #[test]
    fn validation_accepts_backend_neutral_experiment_contracts() {
        assert!(
            validate_experiment_definition(
                "Cross-DCC validation",
                "scene-v1",
                "",
                Some("workflow-1"),
                None,
                &["maya".to_string(), "photoshop".to_string()],
                &json!({"seed": 7}),
            )
            .is_ok()
        );
        assert!(
            validate_experiment_run(
                "run-1",
                "passed",
                None,
                Some("session-1"),
                &json!({"dcc": "custom"}),
                &json!({"score": 0.9}),
                &["artefact://sha256/example".to_string()],
            )
            .is_ok()
        );
    }

    #[test]
    fn validation_rejects_unsafe_or_oversized_values() {
        assert!(!valid_experiment_id("bad/id"));
        assert!(
            validate_experiment_run(
                "run-1",
                "unknown",
                None,
                None,
                &Value::Null,
                &Value::Null,
                &[],
            )
            .is_err()
        );
        assert!(
            validate_experiment_judge(ExperimentJudgeValidation {
                run_id: "run-1",
                evaluator_id: "quality",
                evaluator_version: "1",
                rubric_hash: "sha256:abc",
                status: "passed",
                scores: &json!({}),
                summary: "ok",
                cost: Some(f64::NAN),
                evidence: &[],
            })
            .is_err()
        );
    }
}
