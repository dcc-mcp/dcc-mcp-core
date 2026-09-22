//! Canonical DDL for the gateway admin SQLite database (single source of truth).

/// Bootstrap script executed once per writer connection (WAL + tables + indexes).
/// Schema version 5 — adds the feedback_findings dedup table (#2253-E2).
/// Version 4 added feedback_reports (#2253-E1); version 3 added
/// script_promotion_counters (#2297-A3); version 2 added sessions,
/// tool_calls, and session_events (PIP-2751).
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
-- #2297-A3: Durable repeat counter for escape-hatch scripts, replacing a
-- per-query scan of the audit log. One row per (sha256, dcc_type, tool_name);
-- `count` is bumped by an idempotent upsert and `proposal_state` flips to
-- 'proposed' once the configured repeat threshold is reached.
CREATE TABLE IF NOT EXISTS script_promotion_counters (
  sha256 TEXT NOT NULL,
  dcc_type TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  count INTEGER NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  proposal_state TEXT NOT NULL,
  PRIMARY KEY (sha256, dcc_type, tool_name)
);
-- #2253-E1: Durable agent-feedback reports so `dcc_feedback__report`
-- submissions survive a gateway restart and stay queryable across instances.
-- `report_json` is the object the per-DCC JSONL mirror writes plus the
-- gateway-minted `id` / `timestamp` / `recorded_at` envelope. The admin read
-- path merges both sources by `id` with this table winning, so the two are
-- equivalent but not byte-identical.
-- `occurred_at_ms` mirrors `recorded_at_ms`: the gateway only learns about a
-- report when it is posted, so both mean "accepted by the gateway", and both
-- retention pruning and the admin `range` cutoff are anchored on that instant.
CREATE TABLE IF NOT EXISTS feedback_reports (
  id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 0,
  fingerprint TEXT,
  severity TEXT NOT NULL,
  dcc_type TEXT NOT NULL,
  instance_id TEXT,
  tool_slug TEXT,
  recorded_at TEXT NOT NULL,
  occurred_at_ms INTEGER NOT NULL,
  recorded_at_ms INTEGER NOT NULL,
  report_json TEXT NOT NULL
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
CREATE INDEX IF NOT EXISTS idx_session_events_type ON session_events(event_type, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_script_promotion_counters_state
  ON script_promotion_counters(proposal_state, last_seen_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_reports_occurred ON feedback_reports(occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_reports_recorded ON feedback_reports(recorded_at_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_reports_dcc ON feedback_reports(dcc_type, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_reports_severity ON feedback_reports(severity, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_reports_fingerprint ON feedback_reports(fingerprint);
-- #2253-E2: Cross-instance ingest dedup for findings.
--
-- This is a separate table from `feedback_reports` on purpose. `feedback_reports`
-- (#2253-E1) is the durable per-submission log, keyed by the gateway-minted
-- `feedback_id`, so every accepted submission keeps its own row. This table is a
-- dedup aggregate: many submissions of the same finding collapse onto one
-- `(repo, fingerprint)` row that carries `occurrence_count`.
--
-- Collapsing them into a single table is not possible without breaking one of
-- the two contracts: E1 requires two submissions that share a fingerprint to
-- remain two rows, while E2 requires them to become one.
--
-- Growth and retention: deliberately NOT touched by `prune_old_rows`, so
-- `sqlite_retention_days` does not apply. Deleting a row would reset its
-- `occurrence_count` and discard the evidence that a long-lived finding is
-- still recurring, so time-based pruning here is a product decision rather than
-- a housekeeping default.
--
-- Rows are bounded by the number of distinct `(repo, fingerprint)` pairs, not
-- by submission volume: a repeat report bumps `occurrence_count` instead of
-- inserting. That bound is NOT self-enforcing, though -- the `fingerprint` on
-- a finding is supplied by the caller and the server only checks its *shape*
-- (`sha256:` + 64 lowercase hex digits, see `FindingV1::validate`); the
-- server does not recompute it, so a caller can mint arbitrarily many
-- distinct values and each one lands a new row. Verifying the digest
-- server-side is not possible on this path either: `finding_fingerprint`
-- takes the owning repo as its first input, and that repo is exactly what
-- routing has to derive, so trusting the client-supplied fingerprint is the
-- existing Finding v1 contract.
--
-- No bound on row count is therefore in effect today. The optional per-IP
-- rate limit on the gateway ingress (`rate_limit_per_minute_per_ip`) is a
-- growth-RATE limit, not a growth bound: when configured it throttles how fast
-- one source IP can add rows, it does not stop the table from growing without
-- limit over time or across many source IPs, and it is off unless configured.
-- If row count ever becomes a concern, cap by `last_seen_ms` or evict
-- low-`occurrence_count` rows rather than pruning on age alone.
CREATE TABLE IF NOT EXISTS feedback_findings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  repo TEXT NOT NULL DEFAULT '',
  fingerprint TEXT NOT NULL,
  issues_url TEXT,
  route_rationale TEXT,
  dcc_type TEXT NOT NULL,
  phase TEXT NOT NULL,
  severity TEXT NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  occurrence_count INTEGER NOT NULL DEFAULT 1,
  report_json TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_feedback_findings_repo_fingerprint
  ON feedback_findings(repo, fingerprint);
CREATE INDEX IF NOT EXISTS idx_feedback_findings_last_seen ON feedback_findings(last_seen_ms);
CREATE INDEX IF NOT EXISTS idx_feedback_findings_repo_last_seen
  ON feedback_findings(repo, last_seen_ms);
"#;
