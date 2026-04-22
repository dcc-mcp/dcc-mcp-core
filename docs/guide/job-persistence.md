# Job Persistence

`JobManager` tracks long-running async tool executions (see issues \#316, \#318,
\#326, \#371). By default it keeps all job state in-process, which means every
`Pending` or `Running` row is lost if the server crashes or restarts.

Issue #328 adds an **optional, feature-gated SQLite backend** so tracked jobs
survive a restart and so operators can prune completed history with a built-in
MCP tool.

## Design at a glance

```
┌──────────────┐      put/get/list/update_status/delete_older_than      ┌─────────────────┐
│  JobManager  │ ─────────────────────────────────────────────────────▶ │   JobStorage    │
│  (in-proc)   │                                                        │    (trait)      │
└──────────────┘                                                        └─────────────────┘
                                                                                 │
                                                     ┌───────────────────────────┼─────────────────────────┐
                                                     ▼                                                     ▼
                                             InMemoryStorage                                        SqliteStorage
                                             (default, zero-dep)                                    (feature job-persist-sqlite)
```

- `JobStorage` is `Send + Sync` and synchronous — write-through on every job
  transition. No background flush task, no batching.
- `InMemoryStorage` is the default and ships in every wheel. Same semantics as
  the trait, but obviously does not survive a restart.
- `SqliteStorage` is gated behind the Cargo feature `job-persist-sqlite` and
  pulls in `rusqlite` with `bundled` so no system SQLite is required.

## Enabling SQLite persistence

### Build

```bash
# default build — in-memory only, no SQLite code compiled in
vx just dev

# opt-in — adds SqliteStorage and the rusqlite dependency
cargo build --workspace --features job-persist-sqlite,python-bindings,ext-module
```

### Configure

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

config = McpHttpConfig(port=8765)
config.job_storage_path = "/var/lib/dcc-mcp/jobs.sqlite3"

registry = ToolRegistry()
# register tools...
server = McpHttpServer(registry, config)
handle = server.start()
```

If `job_storage_path` is set but the wheel was built **without**
`job-persist-sqlite`, `server.start()` fails fast with a descriptive error
rather than silently falling back to the in-memory store.

## Startup recovery

When `JobManager` boots with a storage backend, it scans for rows whose status
is `Pending` or `Running` — those are jobs that were in flight when the
previous process exited. Each one is rewritten to the new terminal
`JobStatus::Interrupted` variant with `error = "server restart"` and a fresh
`updated_at`, and a `$/dcc.jobUpdated` notification is emitted (if the
`JobNotifier` is wired). Clients that re-subscribe after reconnect therefore
see a clean terminal transition instead of a dangling `Running` job.

Recovery failures are logged at `error` level and do **not** abort startup — the
in-process map simply starts empty and the process continues to serve new
requests.

## `jobs.cleanup` built-in tool

A SEP-986-compliant built-in MCP tool prunes terminal jobs:

```jsonc
// tools/call
{
  "name": "jobs.cleanup",
  "arguments": { "older_than_hours": 24 }  // default: 24
}
// → { "removed": <count>, "older_than_hours": 24 }
```

- Annotations: `destructive_hint: true`, `idempotent_hint: true`,
  `read_only_hint: false`.
- Only terminal statuses (`Completed`, `Failed`, `Cancelled`, `Interrupted`)
  are eligible; `Pending` and `Running` rows are never removed regardless of
  age.
- Works against whichever backend is configured — in-memory or SQLite.

## Storage schema

```sql
CREATE TABLE IF NOT EXISTS jobs (
    job_id        TEXT PRIMARY KEY,
    parent_job_id TEXT,
    tool          TEXT NOT NULL,
    status        TEXT NOT NULL,
    progress_json TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    error         TEXT,
    result_json   TEXT
);
CREATE INDEX IF NOT EXISTS jobs_status_idx  ON jobs(status);
CREATE INDEX IF NOT EXISTS jobs_parent_idx  ON jobs(parent_job_id);
CREATE INDEX IF NOT EXISTS jobs_updated_idx ON jobs(updated_at);
```

Timestamps are stored as RFC 3339 UTC strings. Progress and result payloads are
JSON-serialized — the schema stays stable even if internal `Job` fields evolve.

## Operational guidance

- **Backup**: the SQLite file is a single path; standard file-level snapshotting
  works. There is no WAL-mode promise across versions — treat it as a durable
  cache, not a system of record.
- **Growth**: call `jobs.cleanup` on a schedule (cron, k8s CronJob, or from an
  orchestrator agent). Default 24h window works for most interactive use.
- **Migration**: there is no cross-version migration today. If the schema
  changes in a future release, delete the file and let `JobManager` recreate
  it — the file is meant to survive restarts, not upgrades.
- **Concurrency**: `SqliteStorage` serializes through a `parking_lot::Mutex` on
  the connection. Good enough for per-DCC servers; do not point multiple
  `McpHttpServer` instances at the same file.

## Related issues

- #316 — Async job execution with `Pending`/`Running`/`Completed` states
- #318 — `JobManager` core
- #326 — `$/dcc.jobUpdated` notifications
- #371 — `jobs.get_status` tool
- **#328** — this document
