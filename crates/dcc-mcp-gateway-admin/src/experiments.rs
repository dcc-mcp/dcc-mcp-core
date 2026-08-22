//! Pure experiment projections for the admin API.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

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
}
