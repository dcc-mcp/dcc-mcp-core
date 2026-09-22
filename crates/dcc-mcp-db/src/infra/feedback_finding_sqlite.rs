//! `feedback_findings` dedup persistence for the gateway admin SQLite store.
//!
//! This module owns every statement that touches the `feedback_findings` table:
//! the `(repo, fingerprint)` dedup upsert plus the reads that project rows back
//! into [`FeedbackFindingRow`].
//!
//! `feedback_findings` is the #2253-E2 dedup aggregate. It is deliberately a
//! different table from `feedback_reports` (#2253-E1, owned by
//! `feedback_report_sqlite`), which is the durable per-submission log keyed by
//! the gateway-minted `feedback_id`.
//!
//! Extracted from `gateway_admin_sqlite` so both files stay inside the
//! 1 500-line production Rust limit enforced by
//! `.github/workflows/check-file-size.yml`.

use rusqlite::{Connection, params};

use crate::domain::error::DbError;
use crate::domain::feedback_finding::{FeedbackFindingInsert, FeedbackFindingRow};

const FEEDBACK_REPORT_COLUMNS: &str = "id, repo, fingerprint, issues_url, route_rationale, dcc_type, \
    phase, severity, first_seen_ms, last_seen_ms, occurrence_count, report_json";

fn row_to_feedback_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedbackFindingRow> {
    Ok(FeedbackFindingRow {
        id: row.get(0)?,
        repo: row.get(1)?,
        fingerprint: row.get(2)?,
        issues_url: row.get(3)?,
        route_rationale: row.get(4)?,
        dcc_type: row.get(5)?,
        phase: row.get(6)?,
        severity: row.get(7)?,
        first_seen_ms: row.get(8)?,
        last_seen_ms: row.get(9)?,
        occurrence_count: row.get(10)?,
        report_json: row.get(11)?,
    })
}

/// Collapse one report onto its `(repo, fingerprint)` row, bumping the counter.
///
/// The `ON CONFLICT` clause is what makes ingest idempotent: the unique index
/// turns a repeat report into an `occurrence_count` increment and a
/// `last_seen_ms` refresh, leaving exactly one row per finding per repo.
///
/// `observed_at_ms` comes from the reporting host, so clocks can disagree or
/// step backwards across instances. `first_seen_ms`/`last_seen_ms` are
/// therefore clamped with `MIN`/`MAX` instead of overwritten, keeping the
/// window monotonic. `list_feedback_findings` orders by `last_seen_ms DESC`,
/// so an unclamped regression would reorder the list.
pub(super) fn upsert_feedback_finding(
    conn: &mut Connection,
    row: &FeedbackFindingInsert,
) -> Result<FeedbackFindingRow, DbError> {
    let tx = conn
        .transaction()
        .map_err(|error| DbError::Backend(error.to_string()))?;
    tx.execute(
        "INSERT INTO feedback_findings \
         (repo, fingerprint, issues_url, route_rationale, dcc_type, phase, severity, \
          first_seen_ms, last_seen_ms, occurrence_count, report_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1, ?9) \
         ON CONFLICT (repo, fingerprint) DO UPDATE SET \
           first_seen_ms = MIN(first_seen_ms, excluded.first_seen_ms), \
           last_seen_ms = MAX(last_seen_ms, excluded.last_seen_ms), \
           occurrence_count = occurrence_count + 1, \
           issues_url = COALESCE(excluded.issues_url, issues_url), \
           route_rationale = COALESCE(excluded.route_rationale, route_rationale), \
           dcc_type = excluded.dcc_type, \
           phase = excluded.phase, \
           severity = excluded.severity, \
           report_json = excluded.report_json",
        params![
            row.repo,
            row.fingerprint,
            row.issues_url,
            row.route_rationale,
            row.dcc_type,
            row.phase,
            row.severity,
            row.observed_at_ms,
            row.report_json,
        ],
    )
    .map_err(|error| DbError::Backend(error.to_string()))?;
    let persisted = select_feedback_finding(&tx, &row.repo, &row.fingerprint)?;
    tx.commit()
        .map_err(|error| DbError::Backend(error.to_string()))?;
    Ok(persisted)
}

/// Read one report back by its `(repo, fingerprint)` dedup key.
pub(super) fn select_feedback_finding(
    conn: &Connection,
    repo: &str,
    fingerprint: &str,
) -> Result<FeedbackFindingRow, DbError> {
    conn.query_row(
        &format!(
            "SELECT {FEEDBACK_REPORT_COLUMNS} FROM feedback_findings \
             WHERE repo = ?1 AND fingerprint = ?2"
        ),
        params![repo, fingerprint],
        row_to_feedback_report,
    )
    .map_err(|error| DbError::Backend(error.to_string()))
}

/// Most recently seen reports, newest first, bounded by `limit`.
pub(super) fn list_feedback_findings(conn: &Connection, limit: usize) -> Vec<FeedbackFindingRow> {
    let mut stmt = match conn.prepare_cached(
        "SELECT id, repo, fingerprint, issues_url, route_rationale, dcc_type, phase, severity, \
         first_seen_ms, last_seen_ms, occurrence_count, report_json \
         FROM feedback_findings ORDER BY last_seen_ms DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![limit as i64], row_to_feedback_report);
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

#[cfg(all(test, feature = "gateway-admin-sqlite"))]
mod tests {
    use super::*;
    use crate::infra::gateway_admin_sqlite::GatewayAdminSqliteLane;
    use tempfile::tempdir;

    fn report(repo: &str, fingerprint: &str, seen_ms: i64) -> FeedbackFindingInsert {
        FeedbackFindingInsert {
            repo: repo.to_string(),
            fingerprint: fingerprint.to_string(),
            issues_url: Some(format!("https://github.com/{repo}/issues")),
            route_rationale: Some("adapter_phase".to_string()),
            dcc_type: "maya".to_string(),
            phase: "dispatch".to_string(),
            severity: "degraded".to_string(),
            observed_at_ms: seen_ms,
            report_json: format!("{{\"fingerprint\":\"{fingerprint}\"}}"),
        }
    }

    #[test]
    fn repeated_reports_collapse_into_one_row_with_a_counter() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "a".repeat(64));

        for seen_ms in [1_000_i64, 2_000, 3_000] {
            let row = lane
                .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, seen_ms))
                .expect("upsert");
            assert_eq!(row.occurrence_count, seen_ms / 1_000);
            assert_eq!(row.first_seen_ms, 1_000);
            assert_eq!(row.last_seen_ms, seen_ms);
            assert_eq!(row.repo, "dcc-mcp/dcc-mcp-maya");
        }

        let stored = lane.list_feedback_findings(10);
        assert_eq!(stored.len(), 1, "three reports must stay one row");
        assert_eq!(stored[0].occurrence_count, 3);
        assert_eq!(stored[0].last_seen_ms, 3_000);
        assert_eq!(
            stored[0].issues_url.as_deref(),
            Some("https://github.com/dcc-mcp/dcc-mcp-maya/issues")
        );
        assert_eq!(stored[0].route_rationale.as_deref(), Some("adapter_phase"));
    }

    #[test]
    fn upsert_keeps_the_row_id_stable_across_occurrences() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "b".repeat(64));

        let first = lane
            .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("first upsert");
        let second = lane
            .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 20))
            .expect("second upsert");

        assert_eq!(first.id, second.id, "a repeat report reuses the row id");
        assert_eq!(second.occurrence_count, 2);
    }

    #[test]
    fn the_same_fingerprint_in_two_repos_stays_two_rows() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "c".repeat(64));

        lane.upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("maya upsert");
        lane.upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-core", &fingerprint, 10))
            .expect("core upsert");

        let stored = lane.list_feedback_findings(10);
        assert_eq!(stored.len(), 2, "the dedup key is (repo, fingerprint)");
    }

    #[test]
    fn unrouted_reports_dedup_under_the_empty_repo() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "d".repeat(64));
        let unrouted = FeedbackFindingInsert {
            repo: String::new(),
            issues_url: None,
            route_rationale: None,
            ..report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10)
        };

        lane.upsert_feedback_finding(&unrouted).expect("first");
        let row = lane.upsert_feedback_finding(&unrouted).expect("second");

        assert_eq!(row.occurrence_count, 2);
        assert_eq!(lane.list_feedback_findings(10).len(), 1);
    }

    /// `observed_at_ms` comes from the reporting host, so a clock that steps
    /// backwards (or trails another instance) must not drag `last_seen_ms`
    /// back with it — the list view orders by `last_seen_ms DESC`.
    #[test]
    fn a_regressing_clock_does_not_move_the_seen_window_backwards() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "f".repeat(64));

        let first = lane
            .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 5_000))
            .expect("first upsert");
        // A second sighting that claims an earlier timestamp than the first.
        let regressing = lane
            .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 1_000))
            .expect("regressing upsert");

        assert_eq!(first.first_seen_ms, 5_000);
        assert_eq!(
            regressing.first_seen_ms, 1_000,
            "the window widens backwards"
        );
        assert_eq!(
            regressing.last_seen_ms, 5_000,
            "last_seen_ms must not regress below the highest value already seen"
        );
        assert_eq!(regressing.occurrence_count, 2);

        // And a later sighting still advances the window.
        let advancing = lane
            .upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 9_000))
            .expect("advancing upsert");
        assert_eq!(advancing.last_seen_ms, 9_000);
        assert_eq!(advancing.first_seen_ms, 1_000);
    }

    #[test]
    fn lookup_by_dedup_key_finds_the_collapsed_row() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "e".repeat(64));
        lane.upsert_feedback_finding(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("upsert");

        let found = lane
            .get_feedback_finding("dcc-mcp/dcc-mcp-maya", &fingerprint)
            .expect("row exists");
        assert_eq!(found.occurrence_count, 1);
        assert!(
            lane.get_feedback_finding("dcc-mcp/other", &fingerprint)
                .is_none()
        );
    }
}
