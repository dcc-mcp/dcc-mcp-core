# Cancellation API

Cooperative cancellation support for DCC-MCP skill scripts (issue #318, #332, #522).

Skill scripts executed inside a `tools/call` request run as regular Python code and cannot be interrupted by the dispatcher. The MCP spec's `notifications/cancelled` message only helps if the running code checks for cancellation at appropriate points.

**Exported symbols:** `CancellationProbe`, `CancelToken`, `CancelledError`, `JobHandle`, `check_cancelled`, `check_dcc_cancelled`, `current_cancel_token`, `current_job_id`, `current_job`, `reset_cancel_token`, `reset_current_job`, `set_cancel_token`, `set_current_job`

## CancellationProbe

Read-only `Protocol` exposed to running skill code. Both the pure-Python
`CancelToken` and the Rust-backed asynchronous MCP/REST probe implement it.

| Attribute | Type | Description |
|-----------|------|-------------|
| `cancelled` (property) | `bool` | Whether the owning dispatcher cancelled the operation. |
| `job_id` (property) | `str \| None` | Server-owned asynchronous job id, when available. |

## CancelToken

Thread-safe cancellation flag settable by the request dispatcher.

```python
from dcc_mcp_core import CancelToken

token = CancelToken("job-42")
token.cancelled  # False
token.job_id  # "job-42"
token.cancel()
token.cancelled  # True
```

### Constructor

| Parameter | Type | Description |
|-----------|------|-------------|
| `job_id` | `str \| None` | Optional server-owned job id associated with the token |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `cancel()` | `None` | Mark the token as cancelled. Idempotent. |
| `cancelled` (property) | `bool` | Whether `cancel()` has been invoked |
| `job_id` (property) | `str \| None` | Associated server-owned job id, when available |

## CancelledError

```python
from dcc_mcp_core import CancelledError
```

Raised by `check_cancelled()` when the active request was cancelled. Deliberately a plain `Exception` subclass — the `@skill_entry` decorator's generic `except Exception` branch will convert an unhandled `CancelledError` into a standard skill error dict.

## check_cancelled

```python
check_cancelled() -> None
```

Raise `CancelledError` if the active request has been cancelled. No-op when invoked outside of a request context (e.g. from a REPL or unit test).

| Parameter | Type | Description |
|-----------|------|-------------|
| (none) | | |

**Raises:** `CancelledError` — if a `CancelToken` is installed and its `cancelled` property is `True`.

```python
from dcc_mcp_core import check_cancelled, skill_success

def run(iterations: int = 100) -> dict:
    for _ in range(iterations):
        check_cancelled()  # raises CancelledError when cancelled
        do_one_unit_of_work()
    return skill_success("done")
```

## set_cancel_token

```python
set_cancel_token(token: CancellationProbe | None) -> contextvars.Token
```

Install a `CancelToken` as the active cancel token for the current context. For **dispatcher** use only — skill authors should call `check_cancelled()` instead.

| Parameter | Type | Description |
|-----------|------|-------------|
| `token` | `CancellationProbe \| None` | The probe to install, or `None` to clear |

**Returns:** A `contextvars.Token` that must be passed to `reset_cancel_token`.

## reset_cancel_token

```python
reset_cancel_token(reset: contextvars.Token) -> None
```

Restore the cancel-token contextvar to its previous value.

| Parameter | Type | Description |
|-----------|------|-------------|
| `reset` | `contextvars.Token` | The token returned by `set_cancel_token` |

## current_cancel_token

```python
current_cancel_token() -> CancellationProbe | None
```

Return the read-only cancellation probe installed in the current context, or
`None` when no dispatcher has installed one. Skill code must not assume the
probe exposes `cancel()`; the Rust-backed MCP/REST implementation is read-only.

::: tip
Use `current_cancel_token()` to poll the cancellation flag without raising, e.g. to flush partial progress before returning.
:::

## current_job_id

```python
current_job_id() -> str | None
```

Return the authoritative server-owned id for the asynchronous job currently
executing the skill. The in-process bridge installs it separately from tool
arguments, so client-supplied `_meta.dcc.jobId` values are never trusted.
Synchronous calls and calls outside a dispatcher context return `None`.

## Per-job cancellation (issue #522)

Skill scripts launched **outside** an MCP request context — queued batch renders, `scriptJob` callbacks, simulation runners — cannot rely on `check_cancelled()` because no `CancelToken` is installed. DCC plugins (Maya, Houdini, Unreal …) submit each callable to their own UI-thread dispatcher and need to flag in-flight jobs for cancellation through a per-job handle.

The four symbols below give the dispatcher a way to publish that handle and skill code a single probe (`check_dcc_cancelled`) that honours **both** layers.

### JobHandle

```python
from dcc_mcp_core import JobHandle
```

A `typing.Protocol` (runtime-checkable) describing the per-job handle a host dispatcher publishes through `set_current_job`. Only one attribute is contractual:

| Attribute | Type | Description |
|-----------|------|-------------|
| `cancelled` (property) | `bool` | `True` when the host dispatcher has signalled cancellation. |

Concrete implementations are free to expose additional fields (request id, progress token, `threading.Event`, …) for their own bookkeeping.

### check_dcc_cancelled

```python
check_dcc_cancelled() -> None
```

Cheap probe that raises `CancelledError` if **either** the active MCP `CancelToken` *or* the per-job `JobHandle` reports cancellation. Skill scripts that can run outside a request context should call this instead of `check_cancelled`.

```python
from dcc_mcp_core import check_dcc_cancelled, skill_success

def run(frames: list[int]) -> dict:
    for frame in frames:
        check_dcc_cancelled()  # honours both MCP token and dispatcher
        render_frame(frame)
    return skill_success("rendered", count=len(frames))
```

### set_current_job / reset_current_job / current_job

```python
set_current_job(job: JobHandle | None) -> contextvars.Token
reset_current_job(reset: contextvars.Token) -> None
current_job: contextvars.ContextVar[JobHandle | None]
```

Dispatcher-only API for installing the active `JobHandle`. Pair every `set_current_job` with a `reset_current_job` in a `finally` block; the contextvar is per-context, so threads spawned with `threading.Thread` start with the default `None` (use `contextvars.copy_context()` if propagation is desired).
