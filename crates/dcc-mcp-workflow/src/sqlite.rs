//! SQLite persistence DDL for workflow runs.
//!
//! **Skeleton only.** This module defines the schema but wires no writer:
//! the execution PR will populate these tables as steps progress. A fresh
//! `apply_migrations` call on an empty DB creates both tables cleanly.

use rusqlite::Connection;

/// DDL applied by [`apply_migrations`]. Public so downstream code (e.g. a
/// migration runner in the HTTP server) can inspect or re-use it.
pub const MIGRATION_V1: &str = r#"
-- dcc-mcp-workflow v1 schema (issue #348 skeleton)
CREATE TABLE IF NOT EXISTS workflows (
    id              TEXT PRIMARY KEY NOT NULL,
    name            TEXT NOT NULL,
    status          TEXT NOT NULL,
    spec_json       TEXT NOT NULL,
    current_step_id TEXT,
    started_at      INTEGER,
    completed_at    INTEGER,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS workflows_status_idx ON workflows(status);

CREATE TABLE IF NOT EXISTS workflow_steps (
    workflow_id     TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    step_id         TEXT NOT NULL,
    status          TEXT NOT NULL,
    result_json     TEXT,
    started_at      INTEGER,
    completed_at    INTEGER,
    PRIMARY KEY (workflow_id, step_id)
);

CREATE INDEX IF NOT EXISTS workflow_steps_status_idx ON workflow_steps(status);
"#;

/// Apply the v1 schema to `conn`. Idempotent via `IF NOT EXISTS`.
///
/// # Errors
///
/// Propagates any [`rusqlite::Error`] raised by `execute_batch`.
pub fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(MIGRATION_V1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_applies_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();

        // Tables exist.
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        assert!(tables.iter().any(|t| t == "workflows"), "got: {tables:?}");
        assert!(
            tables.iter().any(|t| t == "workflow_steps"),
            "got: {tables:?}"
        );

        // Idempotent.
        apply_migrations(&conn).unwrap();
    }
}
