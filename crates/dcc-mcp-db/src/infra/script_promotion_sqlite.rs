//! SQLite adapter for the durable script-promotion counter (#2297-A3).
//!
//! Kept separate from [`super::gateway_admin_sqlite`] so that module stays
//! within the 1500-line file-size gate; this file owns the counter's SQL and
//! nothing else.

use rusqlite::{Connection, params};

use crate::domain::script_promotion::{
    ScriptPromotionBumpJson, ScriptPromotionCounter, ScriptPromotionProposalState,
};

/// Maximum rows a reader may return in one call.
const MAX_READ_ROWS: usize = 1_000;

/// Record one observation of `bump`'s script.
///
/// Idempotent by primary key: every observation of the same
/// `(sha256, dcc_type, tool_name)` lands on the same row and increments
/// `count` by exactly one — repeated bumps never create a second row.
///
/// `proposal_state` is derived inside the statement so the counter can be
/// bumped from the fire-and-forget writer lane without a read-back. States
/// [`ScriptPromotionProposalState::Proposed`] and
/// [`ScriptPromotionProposalState::Dismissed`] are sticky and never regress.
pub fn bump_script_promotion_counter(
    conn: &Connection,
    bump: &ScriptPromotionBumpJson,
) -> rusqlite::Result<()> {
    let policy = bump.policy();
    let observed_at_ms = i64::try_from(bump.observed_at_ms).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT INTO script_promotion_counters \
         (sha256, dcc_type, tool_name, count, first_seen_ms, last_seen_ms, proposal_state) \
         VALUES (?1, ?2, ?3, 1, ?4, ?4, ?5) \
         ON CONFLICT (sha256, dcc_type, tool_name) DO UPDATE SET \
           count = count + 1, \
           last_seen_ms = excluded.last_seen_ms, \
           proposal_state = CASE \
             WHEN proposal_state IN ('proposed', 'dismissed') THEN proposal_state \
             WHEN count + 1 >= ?6 THEN 'proposed' \
             ELSE 'pending' \
           END",
        params![
            bump.sha256,
            bump.dcc_type,
            bump.tool_name,
            observed_at_ms,
            policy.state_for(1).as_str(),
            policy.min_repeats,
        ],
    )?;
    Ok(())
}

/// Read one counter row as JSON, or `None` when the key was never observed.
pub fn get_script_promotion_counter_json(
    conn: &Connection,
    sha256: &str,
    dcc_type: &str,
    tool_name: &str,
) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT sha256, dcc_type, tool_name, count, first_seen_ms, last_seen_ms, proposal_state \
         FROM script_promotion_counters \
         WHERE sha256 = ?1 AND dcc_type = ?2 AND tool_name = ?3",
    )?;
    let mut rows = stmt.query(params![sha256, dcc_type, tool_name])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_json(row)?)),
        None => Ok(None),
    }
}

/// Read counters as JSON, most recently bumped first, bounded by `limit`.
pub fn list_script_promotion_counters_json(
    conn: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<String>> {
    let limit = i64::try_from(limit.clamp(1, MAX_READ_ROWS)).unwrap_or(MAX_READ_ROWS as i64);
    let mut stmt = conn.prepare_cached(
        "SELECT sha256, dcc_type, tool_name, count, first_seen_ms, last_seen_ms, proposal_state \
         FROM script_promotion_counters \
         ORDER BY last_seen_ms DESC, sha256 ASC \
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_json)?;
    rows.collect()
}

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    let counter = ScriptPromotionCounter {
        sha256: row.get(0)?,
        dcc_type: row.get(1)?,
        tool_name: row.get(2)?,
        count: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
        first_seen_ms: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
        last_seen_ms: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
        proposal_state: ScriptPromotionProposalState::parse(&row.get::<_, String>(6)?),
    };
    serde_json::to_string(&counter).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        bump_script_promotion_counter, get_script_promotion_counter_json,
        list_script_promotion_counters_json,
    };
    use crate::domain::script_promotion::{
        ScriptPromotionBumpJson, ScriptPromotionCounter, ScriptPromotionProposalState,
    };
    use crate::infra::gateway_admin_schema::GATEWAY_ADMIN_SQLITE_DDL;
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn open() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(GATEWAY_ADMIN_SQLITE_DDL)
            .expect("apply ddl");
        conn
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn bump(sha256: &str, tool: &str, min_repeats: Option<u32>) -> ScriptPromotionBumpJson {
        ScriptPromotionBumpJson {
            sha256: sha256.into(),
            dcc_type: "maya".into(),
            tool_name: tool.into(),
            observed_at_ms: now_ms(),
            min_repeats,
        }
    }

    fn read(conn: &Connection, sha256: &str, tool: &str) -> ScriptPromotionCounter {
        let raw =
            get_script_promotion_counter_json(conn, sha256, "maya", tool).expect("counter read");
        let raw = raw.unwrap_or_else(|| panic!("counter {sha256}/{tool} missing"));
        serde_json::from_str(&raw).expect("counter json")
    }

    #[test]
    fn three_observations_promote_the_counter() {
        let conn = open();
        let sha = "a".repeat(64);
        for _ in 0..3 {
            bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", None))
                .expect("bump");
        }

        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(counter.count, 3);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Proposed
        );
        assert!(counter.is_candidate());
    }

    #[test]
    fn bump_is_idempotent_per_key() {
        let conn = open();
        let sha = "b".repeat(64);
        for _ in 0..4 {
            bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", None))
                .expect("bump");
        }

        let all = list_script_promotion_counters_json(&conn, 100).expect("list");
        assert_eq!(all.len(), 1, "repeated bumps must not create new rows");
        assert_eq!(read(&conn, &sha, "execute_python").count, 4);
    }

    #[test]
    fn keys_are_scoped_by_sha_dcc_and_tool() {
        let conn = open();
        bump_script_promotion_counter(
            &conn,
            &bump("c".repeat(64).as_str(), "execute_python", None),
        )
        .expect("bump");
        bump_script_promotion_counter(&conn, &bump("c".repeat(64).as_str(), "execute_mel", None))
            .expect("bump");
        let houdini = ScriptPromotionBumpJson {
            sha256: "c".repeat(64),
            dcc_type: "houdini".into(),
            tool_name: "execute_python".into(),
            observed_at_ms: now_ms(),
            min_repeats: None,
        };
        bump_script_promotion_counter(&conn, &houdini).expect("bump");

        assert_eq!(
            list_script_promotion_counters_json(&conn, 100)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn threshold_is_configurable_per_bump() {
        let conn = open();
        let sha = "d".repeat(64);
        for _ in 0..2 {
            bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(2)))
                .expect("bump");
        }
        assert_eq!(
            read(&conn, &sha, "execute_python").proposal_state,
            ScriptPromotionProposalState::Proposed
        );

        let other = "e".repeat(64);
        for _ in 0..2 {
            bump_script_promotion_counter(&conn, &bump(&other, "execute_python", Some(5)))
                .expect("bump");
        }
        assert_eq!(
            read(&conn, &other, "execute_python").proposal_state,
            ScriptPromotionProposalState::Pending
        );
    }

    /// A first observation below the threshold is stored as `pending`.
    ///
    /// This pins the `INSERT` branch, which is the path every never-seen
    /// script takes: `count` starts at `1` and the state comes from
    /// [`ScriptPromotionPolicy::state_for`], not from the `DO UPDATE` clause.
    /// The upsert half of that contract is guarded by
    /// [`threshold_compares_the_count_being_stored`].
    #[test]
    fn single_bump_below_threshold_stays_pending() {
        let conn = open();
        let sha = "1".repeat(64);
        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(2))).expect("bump");

        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(counter.count, 1, "one observation must store count = 1");
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Pending,
            "count 1 is below min_repeats 2, so the counter stays pending"
        );
        assert!(!counter.is_candidate());
    }

    /// Positive control for [`single_bump_below_threshold_stays_pending`]:
    /// the threshold semantics are not frozen as "always pending". With
    /// `min_repeats = 1` the very first observation already reaches the
    /// threshold, so the row must be promoted straight away.
    #[test]
    fn single_bump_at_threshold_one_is_proposed() {
        let conn = open();
        let sha = "2".repeat(64);
        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(1))).expect("bump");

        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(counter.count, 1);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Proposed,
            "min_repeats 1 promotes on the first observation"
        );
        assert!(counter.is_candidate());
    }

    /// Off-by-one guard for the upsert's threshold comparison.
    ///
    /// SQLite evaluates the `DO UPDATE SET` expressions against the row as it
    /// was **before** the update, so `WHEN count + 1 >= ?6` means "the count I
    /// am about to store reaches the threshold". Two observations of a script
    /// with `min_repeats = 3` must therefore still read `pending`: the stored
    /// `count` is `2`, and `2 >= 3` is false.
    ///
    /// This is the discriminating case the counting tests cannot cover — at
    /// `min_repeats = 2` with two bumps, and at `min_repeats = 5` with two
    /// bumps, both readings of `count` agree. Here they do not:
    ///
    /// * reading the *written* value compares `2 + 1 >= 3` and promotes one
    ///   observation early, failing the second assertion;
    /// * dropping the `+ 1` after moving the state check behind the counter
    ///   write compares the *old* value `2 >= 3` and never promotes, failing
    ///   the third assertion.
    #[test]
    fn threshold_compares_the_count_being_stored() {
        let conn = open();
        let sha = "3".repeat(64);

        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(3))).expect("bump");
        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(counter.count, 1);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Pending,
            "one observation is two short of min_repeats = 3"
        );

        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(3))).expect("bump");
        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(
            (counter.count, counter.proposal_state),
            (2, ScriptPromotionProposalState::Pending),
            "two observations are still one short of min_repeats = 3"
        );

        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", Some(3))).expect("bump");
        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(
            (counter.count, counter.proposal_state),
            (3, ScriptPromotionProposalState::Proposed),
            "the third observation reaches min_repeats = 3"
        );
        assert!(counter.is_candidate());
    }

    #[test]
    fn proposed_state_is_sticky() {
        let conn = open();
        let sha = "f".repeat(64);
        for _ in 0..3 {
            bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", None))
                .expect("bump");
        }
        // Simulate an operator dismissal: the state must survive later bumps.
        conn.execute(
            "UPDATE script_promotion_counters SET proposal_state = 'dismissed' WHERE sha256 = ?1",
            rusqlite::params![sha],
        )
        .expect("dismiss");
        bump_script_promotion_counter(&conn, &bump(&sha, "execute_python", None)).expect("bump");

        let counter = read(&conn, &sha, "execute_python");
        assert_eq!(counter.count, 4);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Dismissed
        );
    }

    /// The DDL is additive: re-running it over a database created by the
    /// previous schema version must create the new table without touching
    /// existing rows or their values.
    #[test]
    fn schema_upgrade_preserves_existing_rows() {
        let conn = open();
        conn.execute(
            "INSERT INTO audits (request_id, ts_ms, audit_json) VALUES ('rid-1', 42, '{}')",
            [],
        )
        .expect("seed legacy audit row");
        conn.execute(
            "INSERT INTO traces (request_id, started_ms, trace_json) VALUES ('rid-1', 42, '{}')",
            [],
        )
        .expect("seed legacy trace row");
        conn.execute(
            "INSERT INTO skill_paths_custom (path, created_ms) VALUES ('/tmp/skills', 42)",
            [],
        )
        .expect("seed legacy skill path");

        // Re-applying the canonical DDL is what happens on every lane spawn.
        conn.execute_batch(GATEWAY_ADMIN_SQLITE_DDL)
            .expect("re-apply ddl");

        let audits: i64 = conn
            .query_row("SELECT COUNT(*) FROM audits", [], |row| row.get(0))
            .expect("count audits");
        let traces: i64 = conn
            .query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))
            .expect("count traces");
        let paths: i64 = conn
            .query_row("SELECT COUNT(*) FROM skill_paths_custom", [], |row| {
                row.get(0)
            })
            .expect("count skill paths");
        assert_eq!((audits, traces, paths), (1, 1, 1));

        // The new table is usable immediately afterwards.
        bump_script_promotion_counter(
            &conn,
            &bump("9".repeat(64).as_str(), "execute_python", None),
        )
        .expect("bump after upgrade");
        assert_eq!(
            list_script_promotion_counters_json(&conn, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn missing_key_reads_back_as_none() {
        let conn = open();
        let raw = get_script_promotion_counter_json(&conn, "nope", "maya", "execute_python")
            .expect("counter read");
        assert!(raw.is_none());
        assert!(
            list_script_promotion_counters_json(&conn, 10)
                .expect("list")
                .is_empty()
        );
    }

    /// End-to-end roundtrip through the real writer lane: three observations
    /// of the same script must land on one row that reads back as `proposed`,
    /// and a lane restart must continue the same row instead of adding one.
    #[test]
    fn lane_roundtrip_counts_three_observations_once() {
        use crate::infra::gateway_admin_sqlite::GatewayAdminSqliteLane;
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("counters.sqlite");
        let sha = "a".repeat(64);

        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        for i in 0..3u64 {
            let bump = ScriptPromotionBumpJson {
                sha256: sha.clone(),
                dcc_type: "maya".into(),
                tool_name: "execute_python".into(),
                observed_at_ms: 1_700_000_000_000 + i,
                min_repeats: None,
            };
            let json = serde_json::to_string(&bump).expect("serialize bump");
            lane.try_bump_script_promotion_counter_json(&json);
        }
        drop(lane);

        let reader = crate::infra::gateway_admin_sqlite::GatewayAdminSqliteReader::new(db.clone());
        let rows = reader.list_script_promotion_counters_json(10);
        assert_eq!(rows.len(), 1, "one row per script key");

        let counter: ScriptPromotionCounter = serde_json::from_str(&rows[0]).expect("counter json");
        assert_eq!(counter.count, 3);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Proposed,
            "three repeats reach the default min_repeats=3 threshold"
        );
        assert_eq!(counter.first_seen_ms, 1_700_000_000_000);
        assert_eq!(counter.last_seen_ms, 1_700_000_000_002);

        // Restarting the lane keeps counting on the same row.
        let relaunched = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let bump = ScriptPromotionBumpJson {
            sha256: sha.clone(),
            dcc_type: "maya".into(),
            tool_name: "execute_python".into(),
            observed_at_ms: 1_700_000_000_003,
            min_repeats: None,
        };
        relaunched.try_bump_script_promotion_counter_json(&serde_json::to_string(&bump).unwrap());
        drop(relaunched);

        let rows = reader.list_script_promotion_counters_json(10);
        assert_eq!(rows.len(), 1, "relaunch must not duplicate the row");
        let counter: ScriptPromotionCounter = serde_json::from_str(&rows[0]).expect("counter json");
        assert_eq!(counter.count, 4);
        assert_eq!(counter.first_seen_ms, 1_700_000_000_000);
        assert_eq!(counter.last_seen_ms, 1_700_000_000_003);
    }

    /// The audit row and the counter row are written from the same lane, and
    /// neither write disturbs the other.
    #[test]
    fn lane_roundtrip_survives_concurrent_audit_writes() {
        use crate::domain::gateway_admin_audit::GatewayAdminAuditPersistedJson;
        use crate::infra::gateway_admin_sqlite::GatewayAdminSqliteLane;
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("audits-and-counters.sqlite");
        let sha = "b".repeat(64);

        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        for i in 0..3u64 {
            let audit = GatewayAdminAuditPersistedJson {
                timestamp_ms: 1_700_000_000_000 + i,
                request_id: format!("rid-{i}"),
                trace_id: None,
                span_id: None,
                parent_span_id: None,
                method: Some("tools/call".into()),
                instance_id: None,
                session_id: None,
                transport: Some("mcp".into()),
                agent_id: None,
                agent_name: None,
                agent_model: None,
                actor_id: None,
                actor_name: None,
                actor_email_hash: None,
                client_platform: None,
                client_os: None,
                client_host: None,
                auth_subject: None,
                source_ip: None,
                attribution_trust: None,
                parent_request_id: None,
                action: "execute_python".into(),
                dcc_type: Some("maya".into()),
                success: true,
                error: None,
                duration_ms: Some(5),
                script_execution: Some(serde_json::json!({
                    "sha256": sha,
                    "reused": false,
                })),
                token_accounting: None,
                llm_usage: None,
            };
            lane.try_persist_audit_json(&serde_json::to_string(&audit).unwrap());
            let bump = ScriptPromotionBumpJson {
                sha256: sha.clone(),
                dcc_type: "maya".into(),
                tool_name: "execute_python".into(),
                observed_at_ms: 1_700_000_000_000 + i,
                min_repeats: None,
            };
            lane.try_bump_script_promotion_counter_json(&serde_json::to_string(&bump).unwrap());
        }
        drop(lane);

        let reader = crate::infra::gateway_admin_sqlite::GatewayAdminSqliteReader::new(db);
        assert_eq!(reader.list_audits_recent_json(10).len(), 3);
        let rows = reader.list_script_promotion_counters_json(10);
        assert_eq!(rows.len(), 1);
        let counter: ScriptPromotionCounter = serde_json::from_str(&rows[0]).expect("counter json");
        assert_eq!(counter.count, 3);
        assert_eq!(
            counter.proposal_state,
            ScriptPromotionProposalState::Proposed
        );
    }
}
