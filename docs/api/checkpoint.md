# Checkpoint API

Checkpoint/resume helpers for long-running tool executions (issue #436).

Implements the Checkpoint-and-Resume pattern: checkpoint progress at configurable intervals so interrupted jobs can resume from the last successful checkpoint rather than restarting from scratch.

**Exported symbols:** `CheckpointStore`, `checkpoint_every`, `clear_checkpoint`, `configure_checkpoint_store`, `get_checkpoint`, `list_checkpoints`, `register_checkpoint_tools`, `save_checkpoint`

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

Register `jobs.checkpoint_status` and `jobs.resume_context` MCP tools on `server`. Call **before** `server.start()`.
