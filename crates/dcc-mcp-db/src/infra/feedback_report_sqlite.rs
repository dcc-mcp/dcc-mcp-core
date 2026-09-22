//! SQLite adapter for the durable agent-feedback table (#2253-E1).
//!
//! Kept separate from [`super::gateway_admin_sqlite`] so that module stays
//! within the 1500-line file-size gate; this file owns the feedback SQL and
//! nothing else.

use rusqlite::{Connection, ToSql, params, params_from_iter};
use serde::Deserialize;

/// Maximum rows a reader may return in one call.
const MAX_READ_ROWS: usize = 1_000;

/// Indexed columns extracted from the persisted feedback report envelope.
///
/// The full envelope is stored verbatim in `report_json`; these fields only feed
/// the secondary indexes and the admin API's `dcc` / `severity` filters.
#[derive(Deserialize)]
struct FeedbackReportPersisted {
    id: String,
    kind: String,
    #[serde(default)]
    schema_version: i64,
    #[serde(default)]
    fingerprint: Option<String>,
    severity: String,
    dcc_type: String,
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    tool_slug: Option<String>,
    recorded_at: String,
    timestamp_ms: i64,
    recorded_at_ms: i64,
    /// The feedback record exactly as the per-DCC JSONL mirror writes it.
    report: serde_json::Value,
}

/// Insert (or replace) one feedback report envelope.
///
/// Idempotent by primary key: `id` is the gateway-minted `feedback_id`, so a
/// retried submission overwrites its own row instead of creating a duplicate.
pub fn insert_feedback_report(conn: &Connection, json: &str) -> rusqlite::Result<()> {
    let report: FeedbackReportPersisted = serde_json::from_str(json)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    conn.execute(
        "INSERT OR REPLACE INTO feedback_reports \
         (id, kind, schema_version, fingerprint, severity, dcc_type, \
          instance_id, tool_slug, recorded_at, occurred_at_ms, \
          recorded_at_ms, report_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            report.id,
            report.kind,
            report.schema_version,
            report.fingerprint,
            report.severity,
            report.dcc_type,
            report.instance_id,
            report.tool_slug,
            report.recorded_at,
            report.timestamp_ms,
            report.recorded_at_ms,
            report.report.to_string(),
        ],
    )?;
    Ok(())
}

/// Raw `report_json` rows for persisted feedback, newest first, bounded by `limit`.
///
/// `cutoff_ms` filters on `occurred_at_ms`; `dcc` / `severity` are
/// case-insensitive equality filters, matching the JSONL fallback path.
pub fn list_feedback_reports_json(
    conn: &Connection,
    cutoff_ms: Option<i64>,
    dcc: Option<&str>,
    severity: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<String>> {
    let mut sql = String::from("SELECT report_json FROM feedback_reports WHERE 1 = 1");
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(cutoff) = cutoff_ms {
        sql.push_str(" AND occurred_at_ms >= ?");
        values.push(Box::new(cutoff));
    }
    if let Some(value) = non_empty(dcc) {
        sql.push_str(" AND dcc_type = ? COLLATE NOCASE");
        values.push(Box::new(value.to_ascii_lowercase()));
    }
    if let Some(value) = non_empty(severity) {
        sql.push_str(" AND severity = ? COLLATE NOCASE");
        values.push(Box::new(value.to_ascii_lowercase()));
    }
    sql.push_str(" ORDER BY occurred_at_ms DESC, id DESC LIMIT ?");
    values.push(Box::new(limit.clamp(1, MAX_READ_ROWS) as i64));
    let refs: Vec<&dyn ToSql> = values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(refs), |row| row.get::<_, String>(0))?;
    rows.collect()
}

/// Drop feedback rows older than `cutoff_ms`, as part of retention pruning.
pub fn prune_feedback_reports(conn: &Connection, cutoff_ms: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM feedback_reports WHERE occurred_at_ms < ?1",
        params![cutoff_ms],
    )?;
    Ok(())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{insert_feedback_report, list_feedback_reports_json, prune_feedback_reports};
    use crate::infra::gateway_admin_schema::GATEWAY_ADMIN_SQLITE_DDL;
    use rusqlite::Connection;
    use serde_json::{Value, json};

    fn open() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(GATEWAY_ADMIN_SQLITE_DDL)
            .expect("apply admin schema");
        conn
    }

    fn feedback_row(id: &str, timestamp_ms: i64, dcc_type: &str, severity: &str) -> String {
        json!({
            "id": id,
            "timestamp_ms": timestamp_ms,
            "recorded_at_ms": timestamp_ms,
            "recorded_at": "2026-09-21T17:22:56.000Z",
            "kind": "finding",
            "schema_version": 1,
            "fingerprint": format!("sha256:{}", "a".repeat(64)),
            "severity": severity,
            "dcc_type": dcc_type,
            "instance_id": "instance-1",
            "tool_slug": "maya_scene__save",
            "report": {
                "id": id,
                "timestamp": timestamp_ms as f64 / 1000.0,
                "dcc_type": dcc_type,
                "severity": severity,
            },
        })
        .to_string()
    }

    fn ids(rows: &[String]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| serde_json::from_str::<Value>(row).ok())
            .map(|row| row["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn roundtrip_feedback_report_json() {
        let conn = open();
        insert_feedback_report(
            &conn,
            &feedback_row("fb-1", 1_700_000_000_000, "maya", "blocked"),
        )
        .expect("insert");

        let rows = list_feedback_reports_json(&conn, None, None, None, 10).expect("list");
        assert_eq!(rows.len(), 1);
        let stored: Value = serde_json::from_str(&rows[0]).unwrap();
        assert_eq!(stored["id"], "fb-1");
        assert_eq!(stored["dcc_type"], "maya");
    }

    #[test]
    fn feedback_reports_are_ordered_newest_first() {
        let conn = open();
        insert_feedback_report(&conn, &feedback_row("fb-old", 1_000, "maya", "blocked")).unwrap();
        insert_feedback_report(&conn, &feedback_row("fb-new", 2_000, "maya", "blocked")).unwrap();

        let rows = list_feedback_reports_json(&conn, None, None, None, 10).unwrap();
        assert_eq!(ids(&rows), vec!["fb-new", "fb-old"]);
    }

    #[test]
    fn feedback_reports_filter_by_cutoff_dcc_and_severity() {
        let conn = open();
        insert_feedback_report(&conn, &feedback_row("fb-old", 1_000, "maya", "blocked")).unwrap();
        insert_feedback_report(&conn, &feedback_row("fb-new", 2_000, "maya", "degraded")).unwrap();
        insert_feedback_report(
            &conn,
            &feedback_row("fb-houdini", 3_000, "houdini", "blocked"),
        )
        .unwrap();

        assert_eq!(
            ids(&list_feedback_reports_json(&conn, Some(2_000), None, None, 10).unwrap()),
            vec!["fb-houdini", "fb-new"]
        );
        assert_eq!(
            ids(&list_feedback_reports_json(&conn, None, Some("houdini"), None, 10).unwrap()),
            vec!["fb-houdini"]
        );
        assert_eq!(
            ids(&list_feedback_reports_json(&conn, None, None, Some("blocked"), 10).unwrap()),
            vec!["fb-houdini", "fb-old"]
        );
        // Filters are case-insensitive, matching the JSONL fallback path.
        assert_eq!(
            ids(&list_feedback_reports_json(&conn, None, Some("HOUDINI"), None, 10).unwrap()),
            vec!["fb-houdini"]
        );
    }

    #[test]
    fn feedback_reports_replace_on_retry() {
        let conn = open();
        let row = feedback_row("fb-1", 1_000, "maya", "blocked");
        insert_feedback_report(&conn, &row).unwrap();
        insert_feedback_report(&conn, &row).unwrap();

        let rows = list_feedback_reports_json(&conn, None, None, None, 10).unwrap();
        assert_eq!(rows.len(), 1, "the same feedback_id is idempotent");
    }

    #[test]
    fn prune_drops_expired_feedback_reports() {
        let conn = open();
        insert_feedback_report(&conn, &feedback_row("fb-old", 1_000, "maya", "blocked")).unwrap();
        insert_feedback_report(&conn, &feedback_row("fb-new", 2_000, "maya", "blocked")).unwrap();

        prune_feedback_reports(&conn, 1_500).expect("prune");

        let rows = list_feedback_reports_json(&conn, None, None, None, 10).unwrap();
        assert_eq!(ids(&rows), vec!["fb-new"]);
    }

    #[test]
    fn list_tolerates_a_missing_table() {
        // A build that ran an older schema must not make the admin reader fail;
        // callers fall back to the JSONL mirror when this errors.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        assert!(list_feedback_reports_json(&conn, None, None, None, 10).is_err());
    }
}
