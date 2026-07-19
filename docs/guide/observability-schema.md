# Observability Schema & Migration

This document describes the versioning strategy for observability data schemas:
the SQLite database schema, the Python query API envelope, and the migration
path when backward-incompatible changes are required.

---

## 1. SQLite Database Schema

The gateway admin SQLite database (`gateway_admin.sqlite`) is defined by a
single canonical DDL source at:

```
crates/dcc-mcp-db/src/infra/gateway_admin_schema.rs
```

The DDL is embedded as a `const &str` and executed with `CREATE TABLE IF NOT
EXISTS`, making it safe to run on every gateway startup.

### Current version: 2 (PIP-2751)

Schema version 2 introduced `tool_calls`, `sessions`, and `session_events`
tables alongside the original v1 tables (`traces`, `audits`,
`skill_paths_custom`, `deregistered_instances`, `skill_loaded_state`,
`skill_active_groups`, `agent_memory`).

### Full table inventory

| Table | Version added | Purpose |
|-------|--------------|---------|
| `traces` | v1 | Dispatch trace ring buffer mirror |
| `audits` | v1 | Audit call record mirror |
| `skill_paths_custom` | v1 | Admin UI custom skill discovery roots |
| `deregistered_instances` | v1 | Deregistered gateway instance log |
| `skill_loaded_state` | v1 | Per-DCC skill load state mirror |
| `skill_active_groups` | v1 | Per-DCC active skill groups |
| `agent_memory` | v1 | Agent memory key-value store |
| `tool_calls` | v2 (PIP-2751) | Structured tool-call events |
| `sessions` | v2 (PIP-2751) | Session lifecycle tracking |
| `session_events` | v2 (PIP-2751) | Session lifecycle event log |

### Index inventory

All indexes use `CREATE INDEX IF NOT EXISTS` and are idempotent. Adding a new
index is a non-breaking change and does not require a schema version bump.

---

## 2. Query API Versioning

The `ObservabilityQuery` Python class wraps every response in a versioned
envelope defined in `python/dcc_mcp_core/observability_query.py`:

```python
API_VERSION = "v1"
```

### Envelope

```json
{
  "api_version": "v1",
  "query_type": "session_stats",
  "timestamp_ms": 1700000000000,
  "data": { ... },
  "query_params": { ... },
  "warnings": [ ... ]
}
```

### Version policy

The `API_VERSION` constant is incremented when:

1. A field is **removed** or **renamed** in the `data` payload.
2. A field **type changes** (e.g., integer → string).
3. A field's **semantics change** in a way that would silently break an
   existing consumer.

The version is **NOT** incremented for:

- Adding new optional fields to `data`.
- Adding new `query_type` values.
- Adding new optional envelope fields (`warnings`, `query_params`).
- Performance or correctness changes that preserve the response shape.

### Consumer guidance

Consumers that parse the envelope should:

1. Read `api_version` and hard-fail if it matches an unknown version.
2. Use `query_type` to dispatch to the correct parser.
3. Ignore unknown fields in `data` (forward compatibility).
4. Surface `warnings` when present.

---

## 3. Migration Procedures

### 3.1 Adding a new table

1. Add the `CREATE TABLE` statement to `GATEWAY_ADMIN_SQLITE_DDL` in
   `gateway_admin_schema.rs`.
2. Use `CREATE TABLE IF NOT EXISTS` — the statement is idempotent.
3. Add matching indexes with `CREATE INDEX IF NOT EXISTS`.
4. The new table appears on the next gateway restart. Existing data in other
   tables is unaffected.
5. Update `ObservabilityQuery` methods to query the new table.
6. Update the metric dictionary and this schema document.

**Rollback**: Remove the DDL statement and restart the gateway. The table
remains on disk (harmless) but is no longer written to. Drop it manually if
desired: `DROP TABLE IF EXISTS <name>`.

### 3.2 Adding a column to an existing table

1. Write an `ALTER TABLE ... ADD COLUMN` statement guarded by a schema
   version check.
2. Alternatively, add the column to the `CREATE TABLE IF NOT EXISTS`
   statement — it has no effect on existing tables but ensures new databases
   include the column from the start.
3. For write paths: the gateway must tolerate the column being absent on
   old databases (check-and-default before insert).

**Preferred pattern** — alter on startup when the column is absent:

```rust
// Example: adding `llm_usage` to traces
let has_column = db.query_row(
    "SELECT llm_usage FROM traces LIMIT 1",
    [],
    |_| Ok(()),
).is_ok();
if !has_column {
    db.execute_batch("ALTER TABLE traces ADD COLUMN llm_usage TEXT;")?;
}
```

**Rollback**: Remove the `ALTER TABLE` statement and any writes to the new
column. The column remains on disk; old gateway versions ignore it.

### 3.3 Changing a column type

1. **Do not** use `ALTER TABLE ... RENAME COLUMN` — SQLite's column rename
   requires a full table rebuild.
2. Instead, add a new column with the desired type, dual-write both columns
   for one release cycle, then remove the old column.

Example dual-write cycle:

| Release | Action |
|---------|--------|
| N | Add `duration_ms_new INTEGER`, write both `duration_ms` (TEXT) and `duration_ms_new`. Readers prefer `duration_ms_new` when non-NULL. |
| N+1 | Drop `duration_ms`, rename `duration_ms_new` → `duration_ms`, update readers. |

### 3.4 Removing a table or column

1. Stop writing to the deprecated table/column (release N).
2. Remove all reader code (release N+1).
3. Optionally drop the table/column in a schema migration (release N+2).
   Dropping preserves `IF NOT EXISTS` safety for rollbacks.

---

## 4. Query API Migration

### Adding a new query type

1. Add a new method to `ObservabilityQuery`.
2. Use a unique `query_type` string (e.g., `"new_analytics"`).
3. Register the method in CLI or Admin UI consumers.

No `API_VERSION` bump needed — existing consumers ignore unknown
`query_type` values if they follow the forward-compatibility guidance above.

### Deprecating a field

1. Add the field to `warnings` list in the response envelope:

   ```python
   warnings=["field 'old_field' is deprecated, use 'new_field' instead"]
   ```

2. Keep the deprecated field in `data` for at least one release cycle.
3. Remove it on the next `API_VERSION` bump.

### Breaking a backward-incompatible change

1. Increment `API_VERSION`.
2. Update all first-party consumers (CLI, Admin UI, agent skills) to handle
   the new version.
3. Document the change in the CHANGELOG with the old/new shapes.

---

## 5. Migration when the gateway starts

On startup, the gateway:

1. Runs `GATEWAY_ADMIN_SQLITE_DDL` (idempotent — adds missing tables/indexes).
2. Seads the in-memory trace/audit ring buffers from SQLite (if the
   `admin-persist-sqlite` feature is enabled).
3. Writes fresh rows to both the in-memory ring and the SQLite lane.

Because DDL is `IF NOT EXISTS`, rolling upgrades are safe: old gateways that
do not know about new tables simply ignore them, and new gateways create
missing tables automatically on first startup.

---

## 6. Observability API Version History

| API version | Schema version | Release | Changes |
|-------------|---------------|---------|---------|
| `v1` | 1 | 0.19.x | Original: traces, audits, skill paths, deregistered instances, agent memory |
| `v1` | 2 | 0.20.x (PIP-2751) | Added `tool_calls`, `sessions`, `session_events` tables. Query API unchanged. |
