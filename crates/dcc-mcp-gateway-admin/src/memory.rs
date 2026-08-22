//! Pure agent-memory summary projections for the admin dashboard.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Summarize already-loaded memory rows without owning persistence or queries.
#[must_use]
pub fn memory_summary(rows: &[Value]) -> Value {
    let mut by_dcc = BTreeMap::<String, usize>::new();
    let mut positive = 0usize;
    let mut negative = 0usize;
    let mut ok_count = 0u64;
    let mut fail_count = 0u64;

    for row in rows {
        let dcc = row
            .get("dcc_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *by_dcc.entry(dcc).or_default() += 1;
        let score = row.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        if score > 0.0 {
            positive += 1;
        } else if score < 0.0 {
            negative += 1;
        }
        let (ok, fail) = outcome_counts(row.get("payload").unwrap_or(&Value::Null));
        ok_count += ok;
        fail_count += fail;
    }
    let hit_rate_pct = if ok_count + fail_count > 0 {
        Some((ok_count as f64 / (ok_count + fail_count) as f64) * 100.0)
    } else {
        None
    };
    json!({
        "total": rows.len(),
        "by_dcc": by_dcc,
        "positive": positive,
        "negative": negative,
        "ok_count": ok_count,
        "fail_count": fail_count,
        "hit_rate_pct": hit_rate_pct,
    })
}

fn outcome_counts(payload: &Value) -> (u64, u64) {
    let ok = payload.get("ok_count").and_then(Value::as_u64);
    let fail = payload.get("fail_count").and_then(Value::as_u64);
    if ok.is_some() || fail.is_some() {
        return (ok.unwrap_or(0), fail.unwrap_or(0));
    }
    match payload.get("ok").and_then(Value::as_bool) {
        Some(true) => (1, 0),
        Some(false) => (0, 1),
        None => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_groups_dcc_scores_and_outcomes() {
        let rows = vec![
            json!({"dcc_name": "maya", "score": 1.0, "payload": {"ok": true}}),
            json!({"dcc_name": "maya", "score": -0.5, "payload": {"ok_count": 2, "fail_count": 1}}),
            json!({"dcc_name": "houdini", "score": 0.0, "payload": {"ok": false}}),
        ];

        let summary = memory_summary(&rows);

        assert_eq!(summary["total"], 3);
        assert_eq!(summary["by_dcc"], json!({"houdini": 1, "maya": 2}));
        assert_eq!(summary["positive"], 1);
        assert_eq!(summary["negative"], 1);
        assert_eq!(summary["ok_count"], 3);
        assert_eq!(summary["fail_count"], 2);
        assert_eq!(summary["hit_rate_pct"], 60.0);
    }

    #[test]
    fn empty_summary_has_no_hit_rate() {
        let summary = memory_summary(&[]);

        assert_eq!(summary["total"], 0);
        assert!(summary["hit_rate_pct"].is_null());
    }
}
