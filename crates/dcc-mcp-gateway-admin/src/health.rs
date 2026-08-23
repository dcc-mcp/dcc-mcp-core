//! Backend-neutral gateway health projections.

use serde_json::{Value, json};

/// Split gateway sentinel rows into the active instance and ordered candidates.
#[must_use]
pub fn gateway_health_payload(mut rows: Vec<Value>) -> Value {
    rows.sort_by(|a, b| {
        let role_a = a.get("role").and_then(Value::as_str).unwrap_or("");
        let role_b = b.get("role").and_then(Value::as_str).unwrap_or("");
        let rank_a = usize::from(role_a != "active");
        let rank_b = usize::from(role_b != "active");
        rank_a.cmp(&rank_b).then_with(|| {
            let timestamp_a = a
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let timestamp_b = b
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            timestamp_b.cmp(&timestamp_a)
        })
    });
    let current = rows
        .iter()
        .find(|row| row.get("role").and_then(Value::as_str) == Some("active"))
        .cloned()
        .or_else(|| rows.first().cloned());
    let candidates: Vec<Value> = rows
        .into_iter()
        .filter(|row| row.get("role").and_then(Value::as_str) != Some("active"))
        .collect();
    json!({
        "current": current,
        "candidates": candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_active_gateway_and_orders_candidates_by_heartbeat() {
        let payload = gateway_health_payload(vec![
            json!({"name": "old", "role": "candidate", "last_heartbeat_unix": 10}),
            json!({"name": "active", "role": "active", "last_heartbeat_unix": 5}),
            json!({"name": "new", "role": "candidate", "last_heartbeat_unix": 20}),
        ]);

        assert_eq!(payload["current"]["name"], "active");
        assert_eq!(payload["candidates"][0]["name"], "new");
        assert_eq!(payload["candidates"][1]["name"], "old");
    }

    #[test]
    fn falls_back_to_newest_candidate_when_no_active_gateway_exists() {
        let payload = gateway_health_payload(vec![
            json!({"name": "old", "role": "candidate", "last_heartbeat_unix": 10}),
            json!({"name": "new", "role": "candidate", "last_heartbeat_unix": 20}),
        ]);

        assert_eq!(payload["current"]["name"], "new");
        assert_eq!(payload["candidates"].as_array().unwrap().len(), 2);
    }
}
