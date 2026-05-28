//! Canonical DDL for the gateway admin SQLite database (single source of truth).

/// Bootstrap script executed once per writer connection (WAL + tables + indexes).
pub const GATEWAY_ADMIN_SQLITE_DDL: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS traces (
  request_id TEXT PRIMARY KEY NOT NULL,
  started_ms INTEGER NOT NULL,
  trace_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audits (
  request_id TEXT PRIMARY KEY NOT NULL,
  ts_ms INTEGER NOT NULL,
  audit_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_paths_custom (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE,
  created_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS deregistered_instances (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_ms INTEGER NOT NULL,
  dcc_type TEXT NOT NULL,
  instance_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  entry_json TEXT NOT NULL
);
-- Mirror of per-DCC SkillCatalog.loaded + active_groups (#1405).
-- Source of truth is the per-DCC JSON file at
-- <data_dir>/skills/<dcc>/loaded.json; this table exists so the admin UI
-- can render currently-loaded skills across all DCC instances on one
-- machine without each DCC needing its own admin HTTP surface.
CREATE TABLE IF NOT EXISTS skill_loaded_state (
  dcc_type TEXT NOT NULL,
  skill_name TEXT NOT NULL,
  skill_version TEXT,
  skill_path TEXT,
  loaded_at_ms INTEGER NOT NULL,
  PRIMARY KEY (dcc_type, skill_name)
);
CREATE TABLE IF NOT EXISTS skill_active_groups (
  dcc_type TEXT NOT NULL,
  group_name TEXT NOT NULL,
  activated_at_ms INTEGER NOT NULL,
  PRIMARY KEY (dcc_type, group_name)
);
CREATE INDEX IF NOT EXISTS idx_traces_started ON traces(started_ms);
CREATE INDEX IF NOT EXISTS idx_audits_ts ON audits(ts_ms);
CREATE INDEX IF NOT EXISTS idx_deregistered_instances_ts ON deregistered_instances(ts_ms);
CREATE INDEX IF NOT EXISTS idx_skill_loaded_state_dcc ON skill_loaded_state(dcc_type);
CREATE INDEX IF NOT EXISTS idx_skill_active_groups_dcc ON skill_active_groups(dcc_type);
"#;
