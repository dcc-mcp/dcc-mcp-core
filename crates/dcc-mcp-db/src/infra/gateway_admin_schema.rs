//! Canonical DDL for the gateway admin SQLite database (single source of truth).

/// Bootstrap script executed once per writer connection (WAL + tables + indexes).
/// Schema version 2 — adds sessions, tool_calls, and session_events tables (PIP-2751).
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
CREATE TABLE IF NOT EXISTS agent_memory (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  layer TEXT NOT NULL,
  key TEXT NOT NULL,
  session_id TEXT NOT NULL,
  dcc_name TEXT NOT NULL,
  score REAL NOT NULL,
  created_unix_secs REAL NOT NULL,
  payload_json TEXT NOT NULL
);
-- PIP-2751: Structured tool-call events for traceability and aggregation.
CREATE TABLE IF NOT EXISTS tool_calls (
  request_id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL,
  parent_request_id TEXT,
  batch_id TEXT,
  tool_name TEXT NOT NULL,
  skill_name TEXT,
  dcc_type TEXT,
  instance_id TEXT,
  agent_id TEXT,
  transport TEXT,
  via_gateway INTEGER,
  started_at_ms INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  success INTEGER NOT NULL,
  error_message TEXT,
  error_kind TEXT,
  mcp_method TEXT,
  trace_id TEXT,
  span_id TEXT
);
-- PIP-2751: Session lifecycle tracking with parent-child support.
CREATE TABLE IF NOT EXISTS sessions (
  session_id TEXT PRIMARY KEY NOT NULL,
  parent_session_id TEXT,
  dcc_type TEXT NOT NULL,
  instance_id TEXT,
  status TEXT NOT NULL,
  started_at_ms INTEGER NOT NULL,
  last_activity_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER,
  end_reason_json TEXT,
  tool_call_count INTEGER NOT NULL DEFAULT 0,
  error_count INTEGER NOT NULL DEFAULT 0,
  core_version TEXT NOT NULL,
  adapter_version TEXT,
  build_sha TEXT
);
-- PIP-2751: Session lifecycle events for time-series replay.
CREATE TABLE IF NOT EXISTS session_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  event_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_traces_started ON traces(started_ms);
CREATE INDEX IF NOT EXISTS idx_audits_ts ON audits(ts_ms);
CREATE INDEX IF NOT EXISTS idx_deregistered_instances_ts ON deregistered_instances(ts_ms);
CREATE INDEX IF NOT EXISTS idx_skill_loaded_state_dcc ON skill_loaded_state(dcc_type);
CREATE INDEX IF NOT EXISTS idx_skill_active_groups_dcc ON skill_active_groups(dcc_type);
CREATE INDEX IF NOT EXISTS idx_agent_memory_layer_created ON agent_memory(layer, created_unix_secs);
CREATE INDEX IF NOT EXISTS idx_agent_memory_dcc_created ON agent_memory(dcc_name, created_unix_secs);
CREATE INDEX IF NOT EXISTS idx_agent_memory_session_layer ON agent_memory(session_id, layer);
CREATE INDEX IF NOT EXISTS idx_agent_memory_key ON agent_memory(key);
CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_tool_calls_parent ON tool_calls(parent_request_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_batch ON tool_calls(batch_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_tool ON tool_calls(tool_name, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_tool_calls_dcc ON tool_calls(dcc_type, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_dcc ON sessions(dcc_type, started_at_ms);
CREATE INDEX IF NOT EXISTS idx_session_events_session ON session_events(session_id, created_at_ms);
"#;
