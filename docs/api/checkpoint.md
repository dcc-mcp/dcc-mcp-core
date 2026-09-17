# Checkpoint API

Checkpoint/resume helpers for long-running tool executions (issue #436).

Implements the Checkpoint-and-Resume pattern: checkpoint progress at configurable intervals so interrupted jobs can resume from the last successful checkpoint rather than restarting from scratch.

**Exported symbols:** `CHECKPOINT_FILE_NAME`, `CheckpointStore`, `checkpoint_every`, `clear_checkpoint`, `configure_checkpoint_store`, `default_checkpoint_dir`, `default_checkpoint_path`, `get_checkpoint`, `list_checkpoints`, `register_checkpoint_tools`, `resolve_checkpoint_path`, `save_checkpoint`

## Durable default (issue #2300)

Iteration state must survive restarts, so a server's checkpoint store is
**durable by default** — no adapter opt-in required:

- `DccServerBase` builds its store from `resolve_checkpoint_path(dcc_name)`,
  which resolves to `<base>/<dcc>/checkpoints.json`.
- `<base>` is `DCC_MCP_CHECKPOINT_DIR` when set, otherwise `~/.dcc-mcp`.
- `jobs_checkpoint_status` / `jobs_resume_context` are registered on the
  adapter server at startup, so an agent can read resume state after a
  restart.
- Writes are atomic (temp file + rename), so a crash mid-write cannot
  truncate the file.

Opt out with any of:

| Opt-out | Scope |
|---------|-------|
| `DCC_MCP_CHECKPOINT_IN_MEMORY=1` | Process-wide env var |
| `enable_checkpoint_persistence=False` | Per-server option |
| `checkpoint_path=...` | Per-server explicit file (wins over both) |

The module-level compatibility store used by `save_checkpoint` /
`get_checkpoint` stays in memory; only the per-server store is durable.

```python
from dcc_mcp_core.checkpoint import default_checkpoint_path, resolve_checkpoint_path

default_checkpoint_path("maya")            # ~/.dcc-mcp/maya/checkpoints.json
resolve_checkpoint_path("maya")            # same, unless opted out
resolve_checkpoint_path("maya", in_memory=True)   # None
```

## CheckpointStore

Thread-safe checkpoint storage backend. Default is in-memory; pass `path` to persist to a JSON file.

### Constructor

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | `str \| Path \| None` | `None` | Filesystem path for durable storage. `None` = in-memory only |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `save(job_id, state, progress_hint="")` | `None` | Save or overwrite the checkpoint for `job_id` |
| `get(job_id)` | `dict \| None` | Return the checkpoint dict, or `None` if not found |
| `clear(job_id)` | `bool` | Delete the checkpoint; returns `True` if it existed |
| `list_ids()` | `list[str]` | Return all job IDs that have checkpoints |
| `clear_all()` | `int` | Delete all checkpoints; returns count deleted |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `path` | `Path \| None` | Backing file, or `None` for in-memory stores |
| `is_durable` | `bool` | `True` when checkpoints survive a process restart |

## Path helpers

```python
default_checkpoint_dir() -> Path
default_checkpoint_path(dcc_name: str = "dcc", instance_id: str | None = None) -> Path
resolve_checkpoint_path(
    dcc_name: str = "dcc",
    instance_id: str | None = None,
    *,
    path: str | Path | None = None,
    in_memory: bool | None = None,
    env: Mapping[str, str] | None = None,
) -> Path | None
```

| Function | Description |
|----------|-------------|
| `default_checkpoint_dir()` | `DCC_MCP_CHECKPOINT_DIR` or `~/.dcc-mcp` |
| `default_checkpoint_path(dcc_name, instance_id=None)` | `<base>/<dcc>/[instance/]checkpoints.json`; names are sanitised |
| `resolve_checkpoint_path(...)` | Explicit `path` → in-memory opt-out → durable default; `None` means in-memory |

`CHECKPOINT_FILE_NAME` is the `"checkpoints.json"` constant used by
`default_checkpoint_path`.

## configure_checkpoint_store

```python
configure_checkpoint_store(path: str | Path | None = None) -> CheckpointStore
```

Replace the module-level default store and return it. Call once at startup to enable durable storage.

## save_checkpoint

```python
save_checkpoint(job_id: str, state: dict, *, progress_hint: str = "", store: CheckpointStore | None = None) -> None
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `job_id` | `str` | The job identifier |
| `state` | `dict[str, Any]` | Serialisable dict (replaces any previous checkpoint) |
| `progress_hint` | `str` | Human-readable summary (e.g. "Processed 180/200 files") |
| `store` | `CheckpointStore \| None` | Custom store; defaults to module-level store |

## get_checkpoint

```python
get_checkpoint(job_id: str, *, store: CheckpointStore | None = None) -> dict | None
```

Returns dict with keys: `job_id`, `saved_at` (float epoch), `progress_hint`, `context` (the state dict), or `None` if no checkpoint exists.

## clear_checkpoint

```python
clear_checkpoint(job_id: str, *, store: CheckpointStore | None = None) -> bool
```

## list_checkpoints

```python
list_checkpoints(*, store: CheckpointStore | None = None) -> list[str]
```

## checkpoint_every

```python
checkpoint_every(n: int, job_id: str, state_fn: Any, *, progress_fn: Any = None, store: CheckpointStore | None = None) -> None
```

Call inside a loop to auto-checkpoint every `n` iterations.

| Parameter | Type | Description |
|-----------|------|-------------|
| `n` | `int` | Checkpoint interval |
| `job_id` | `str` | Job identifier |
| `state_fn` | `callable` | Zero-arg callable returning the current state dict |
| `progress_fn` | `callable \| None` | Zero-arg callable returning a progress hint string |

```python
for i, item in enumerate(items):
    process(item)
    checkpoint_every(
        50, job_id,
        state_fn=lambda: {"index": i, "last": item},
        progress_fn=lambda: f"Processed {i+1}/{len(items)}",
    )
```

## register_checkpoint_tools

```python
register_checkpoint_tools(server, *, dcc_name="dcc", store=None) -> None
```

Register `jobs_checkpoint_status` and `jobs_resume_context` MCP tools on `server`. Call **before** `server.start()`.
