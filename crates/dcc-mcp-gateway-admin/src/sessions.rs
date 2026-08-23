//! Backend-neutral session list and detail projections.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};

/// Build the stable session-list response from persistence rows.
#[must_use]
pub fn sessions_payload(rows: Vec<Value>) -> Value {
    let mut by_dcc = BTreeMap::<String, usize>::new();
    let mut active = 0usize;
    let mut ended = 0usize;
    let mut crashed = 0usize;
    let mut disconnected = 0usize;

    for row in &rows {
        let dcc = row
            .get("dcc_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        *by_dcc.entry(dcc.to_string()).or_default() += 1;

        match row
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
        {
            "active" => active += 1,
            "ended" => ended += 1,
            "crashed" | "gpu_crashed" => crashed += 1,
            "disconnected" => disconnected += 1,
            _ => {}
        }
    }

    json!({
        "total": rows.len(),
        "sessions": rows,
        "summary": {
            "active": active,
            "ended": ended,
            "crashed": crashed,
            "disconnected": disconnected,
            "by_dcc": by_dcc,
        },
    })
}

/// Build the stable session-detail response from persistence rows.
#[must_use]
pub fn session_detail_payload(session: Value, tool_calls: Vec<Value>, events: Vec<Value>) -> Value {
    let trace_ids: Vec<&str> = tool_calls
        .iter()
        .filter_map(|call| call.get("trace_id").and_then(Value::as_str))
        .collect::<HashSet<_>>()
        .into_iter()
        .take(20)
        .collect();
    let total_calls = tool_calls.len();
    let successful_calls = tool_calls
        .iter()
        .filter(|call| call.get("success").and_then(Value::as_i64) == Some(1))
        .count();

    json!({
        "session": session,
        "tool_calls": tool_calls,
        "events": events,
        "traces": trace_ids,
        "summary": {
            "total_tool_calls": total_calls,
            "successful_tool_calls": successful_calls,
            "failed_tool_calls": total_calls.saturating_sub(successful_calls),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_projection_groups_status_and_dcc_without_backend_types() {
        let payload = sessions_payload(vec![
            json!({"session_id": "a", "dcc_type": "maya", "status": "active"}),
            json!({"session_id": "b", "dcc_type": "maya", "status": "gpu_crashed"}),
            json!({"session_id": "c", "dcc_type": "photoshop", "status": "disconnected"}),
            json!({"session_id": "d", "status": "ended"}),
        ]);

        assert_eq!(payload["total"], 4);
        assert_eq!(payload["summary"]["active"], 1);
        assert_eq!(payload["summary"]["ended"], 1);
        assert_eq!(payload["summary"]["crashed"], 1);
        assert_eq!(payload["summary"]["disconnected"], 1);
        assert_eq!(payload["summary"]["by_dcc"]["maya"], 2);
        assert_eq!(payload["summary"]["by_dcc"]["unknown"], 1);
    }

    #[test]
    fn detail_projection_deduplicates_traces_and_counts_outcomes() {
        let payload = session_detail_payload(
            json!({"session_id": "session-a"}),
            vec![
                json!({"trace_id": "trace-a", "success": 1}),
                json!({"trace_id": "trace-a", "success": 0}),
                json!({"trace_id": "trace-b"}),
            ],
            vec![json!({"event_type": "ended"})],
        );

        assert_eq!(payload["summary"]["total_tool_calls"], 3);
        assert_eq!(payload["summary"]["successful_tool_calls"], 1);
        assert_eq!(payload["summary"]["failed_tool_calls"], 2);
        assert_eq!(payload["traces"].as_array().unwrap().len(), 2);
    }
}
