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

When adapters use the default Python wiring, the database name includes the
DCC process instance key (`dcc-mcp-<dcc>-<key>-jobs.db`). This lets two GUI
instances of the same DCC own independent SQLite leases while preserving
history across stop/start in one process. Set `DCC_MCP_JOB_INSTANCE_KEY` to a
stable operator-provided key when the process identity is managed externally.
An explicit `job_storage_path` (or `DCC_MCP_JOB_STORAGE_PATH`) opts into a
shared pathname and therefore remains subject to single-owner arbitration;
ownership failures are reported as unavailable rather than falling back to
memory.

## Runtime write failures

Persistence is fail-soft after startup: jobs continue to run from the
in-process map if the configured backend becomes unwritable. A manager records
consecutive write failures and disables further persistence attempts after
three identical errors. Transient failures below that threshold are logged at
`DEBUG`; crossing the threshold emits one `WARN`, so a broken SQLite file does
not produce a warning for every later job transition.

`GET /health` exposes a payload-safe snapshot without filesystem paths or raw
backend messages:

```json
{
  "ok": true,
  "service": "dcc-mcp-http",
  "job_persistence": {
    "state": "disabled",
    "consecutive_failures": 3,
    "last_error_kind": "readonly"
  }
}
```

`state` is one of `not_configured`, `healthy`, `degraded`, `disabled`, or
`unavailable`. `unavailable` means persistence was requested but the selected
runtime cannot safely provide the storage contract (for example a lite
sidecar without the native SQLite backend); callers must not treat it as
durable persistence.
`last_error_kind` is a stable category suitable for diagnostics: `readonly`,
`wal`, `busy`, `disk_full`, `decode`, `feature_disabled`, or the generic
`backend`. It never contains a database path or backend-specific raw message.
Installing a storage backend on a new manager resets the circuit. A disabled
manager does not probe or automatically repair the database; replace or
restart it after correcting the backend fault.

The SQLite backend sets a bounded busy timeout and attempts WAL mode for
concurrent readers. WAL setup is best-effort: if a read-only database or stale
sidecar files prevent the pragma, the existing journal mode is preserved and
the write circuit remains authoritative. The backend never removes `-wal` or
`-shm` files automatically. Terminal-job pruning is explicit through
`jobs_cleanup`; non-terminal rows are never deleted.

The adapter startup check first acquires the same physical-file ownership
sidecar lease as the Rust backend (aliases converge on one lease), then uses a
storage-level `BEGIN IMMEDIATE` transaction with a temporary sentinel table and
rolls it back. It does not
start a server, scan job rows, or recover `Pending`/`Running` jobs. Read-only
ACL, ownership, and WAL/SHM failures are classified as unavailable and keep
the configured path so the real server fails closed. A failed retention prune records
`retention_prune_failed` in persistence health and disables further writes until
the next cleanup retry or process restart; persisted rows are left untouched.
The in-process terminal rows remain visible until a durable prune succeeds, so a
failed cleanup cannot make them disappear and then reappear after restart.

For deployments that want bounded history, set the opt-in
`job_retention_hours` configuration. Startup removes only terminal rows older
than the cutoff; a failed prune is logged and leaves all persisted rows
unchanged. The default is `None`, so existing deployments remain
non-destructive. A writable SQLite database is process-owned through a
physical-identity sidecar lock outside the database directory; a second process
fails closed instead of sharing a write connection. Shutdown drains accepted
 worker commands within the configured bound before closing the backend. If an
 arbitrary in-flight SQLite write is still blocked, both explicit shutdown and
 manager Drop call the backend force-close hook before detaching the worker;
 SQLite is interrupted, marked closed, and fenced so subsequent writes fail
 closed and a replacement can reacquire the sidecar lease.

```python
config.job_retention_hours = 24 * 30
```

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

### Recovery policy (issue #567)

`McpHttpConfig::job_recovery` selects how the recovery sweep treats in-flight
rows. Two values are accepted today:

| Value (Rust)                  | Wire string | Effect                                                                                                                                                                                                                                                                                                                                                                                                                                       |
|-------------------------------|-------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `JobRecoveryPolicy::Drop`     | `"drop"`    | **Default.** Every `Pending` / `Running` row is rewritten to `Interrupted`. Always safe — never re-runs a partially-applied tool. Matches the behaviour described above.                                                                                                                                                                                                                                                                     |
| `JobRecoveryPolicy::Requeue`  | `"requeue"` | **Reserved.** The contract is "re-submit idempotent in-flight jobs from the persisted spec". Today the `jobs` row stores only `tool_name`, not the original arguments, so true requeue is not yet possible. The variant is **accepted but degrades to `Drop`** — startup logs a `WARN` (`requested_policy=requeue effective_policy=drop`) and the recovery sweep behaves identically to `Drop`. The accepted-but-degraded contract lets DCC adapters (`dcc-mcp-maya`, `dcc-mcp-houdini`) plumb the knob today; the real implementation will land alongside tool-arg persistence without a config-shape break. |

```python
from dcc_mcp_core import McpHttpConfig

config = McpHttpConfig(port=8765)
config.job_storage_path = "/var/lib/dcc-mcp/jobs.sqlite3"
config.job_recovery = "requeue"   # accepted, currently behaves as "drop" + WARN
```

```rust
use dcc_mcp_http_types::config::{JobRecoveryPolicy, McpHttpConfig};

let cfg = McpHttpConfig::new(8765)
    .with_job_storage_path("/var/lib/dcc-mcp/jobs.sqlite3")
    .with_job_recovery(JobRecoveryPolicy::Requeue);
```

If you want to *guarantee* drop semantics regardless of future releases, leave
`job_recovery` at its default. If you want to opt in to true requeue when it
ships, set `job_recovery = "requeue"` today and the same configuration will
pick up the new behaviour automatically.

## `jobs_cleanup` built-in tool

A client-safe built-in MCP tool prunes terminal jobs:

```jsonc
// tools/call
{
  "name": "jobs_cleanup",
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
- The explicit tool waits for the persistence worker to confirm the durable
  delete before reporting `removed`; the wait runs on a blocking worker so
  synchronous storage I/O cannot starve the HTTP runtime. If the backend
  cannot complete the delete, no rows are removed from the in-process map.
- The Rust `JobManager::cleanup_older_than_hours` helper remains a
  non-blocking enqueue API for offloaded backends and therefore reports zero
  until confirmation; callers that need a confirmed count should use the
  blocking helper or this MCP tool.

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
- **Growth**: call `jobs_cleanup` on a schedule (cron, k8s CronJob, or from an
  orchestrator agent). Default 24h window works for most interactive use.
- **Migration**: there is no cross-version migration today. If the schema
  changes in a future release, delete the file and let `JobManager` recreate
  it — the file is meant to survive restarts, not upgrades.
- **Concurrency**: `SqliteStorage` serializes through a `parking_lot::Mutex` on
  the connection and claims a sidecar ownership lease. Pointing multiple
  `McpHttpServer` instances at the same file is rejected fail-closed; use a
  distinct database path per process.

## Related issues

- #316 — Async job execution with `Pending`/`Running`/`Completed` states
- #318 — `JobManager` core
- #326 — `$/dcc.jobUpdated` notifications
- #371 — `jobs_get_status` tool
- **#328** — this document
- **#567** — `job_recovery` policy contract (`drop` / `requeue`)
- **#2277** — bounded write-failure latching and health visibility
