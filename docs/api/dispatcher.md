# Callable Dispatcher API

DCC-neutral protocols for **routing in-process Python skill scripts onto a host's
UI / main thread**, plus a declarative **minimal-mode skill loader** for embedded
DCCs (issues #520, #521, #525).

Every embedded DCC plugin (Maya, Houdini, Unreal, Blender Python …) re-implements
the same pattern: receive an MCP `tools/call`, route the script to the host's
event loop, return a JSON-serialisable result. This module lifts that pattern
into reusable contracts so each adapter only supplies host-specific glue.

**Exported symbols:** `BaseDccCallableDispatcher`, `InProcessExecutionContext`,
`BaseDccCallableDispatcherFull`, `BaseDccPump`, `InProcessCallableDispatcher`,
`JobEntry`, `JobOutcome`, `PendingEnvelope`, `DrainStats`, `PumpStats`,
`current_callable_job`, `MinimalModeConfig`, `build_inprocess_executor`,
`run_skill_script`.

## When to use what

| Need | Use this |
|------|----------|
| Wire a single `dispatch_callable(func, *args, **kwargs)` shim | `BaseDccCallableDispatcher` (#521) |
| Full submit / cancel / shutdown contract | `BaseDccCallableDispatcherFull` (#520) |
| Cooperative idle-tick that drains a queue (Maya `scriptJob(event=['idle', …])`) | `BaseDccPump` (#520) |
| Reference single-thread implementation for `mayapy` / pytest / batch | `InProcessCallableDispatcher` |
| Per-job cancellation handle reachable from skill scripts | `current_callable_job` ContextVar |
| Declarative progressive skill loading at startup | `MinimalModeConfig` (#525) |

## BaseDccCallableDispatcher (minimal)

```python
from dcc_mcp_core import BaseDccCallableDispatcher

class MyDispatcher:  # duck-typing OK; explicit subclass enables isinstance() check
    def dispatch_callable(self, func, *args, **kwargs):
        # Push (func, args, kwargs) onto the host's UI-thread queue and
        # block until it returns. Maya example:
        #   from maya import utils
        #   return utils.executeInMainThreadWithResult(lambda: func(*args, **kwargs))
        ...

assert isinstance(MyDispatcher(), BaseDccCallableDispatcher)
```

The single `dispatch_callable(func, *args, **kwargs) -> Any` method is the **only**
contractual surface — kept narrow so the simplest hosts can satisfy it with one
line of glue. Use the *Full* variant below when you also need cancellation.

When skill tools execute in-process, core passes execution metadata through the
executor and into `dispatch_callable`:

```python
from dcc_mcp_core import InProcessExecutionContext

def dispatch_callable(self, func, *args, **kwargs):
    context: InProcessExecutionContext = kwargs["context"]
    if context.thread_affinity == "main":
        return run_on_ui_thread(lambda: func())
    return func()
```

The keyword arguments include `affinity`, `context`, `action_name`,
`skill_name`, `execution`, and `timeout_hint_secs`. Dispatchers may ignore the
extra fields, but should use `affinity` / `context.thread_affinity` to avoid
routing pure filesystem tools through the DCC UI thread.

## BaseDccCallableDispatcherFull (cancellable)

```python
from dcc_mcp_core import (
    BaseDccCallableDispatcherFull, JobOutcome, PendingEnvelope,
)

class HostDispatcher:
    def submit_callable(self, request_id, task, affinity="main", timeout_ms=None) -> JobOutcome: ...
    def submit_async_callable(self, request_id, task, *, affinity="main", timeout_ms=None,
                              progress_token=None, on_complete=None) -> PendingEnvelope: ...
    def cancel(self, request_id: str) -> bool: ...
    def shutdown(self, reason: str = "Interrupted") -> int: ...
```

| Method | Purpose |
|--------|---------|
| `submit_callable` | Synchronous submit; blocks for `JobOutcome` |
| `submit_async_callable` | Returns `PendingEnvelope` immediately; result delivered via `on_complete` callback |
| `cancel(request_id)` | Set the in-flight job's `cancel_flag`; returns `True` when found |
| `shutdown(reason)` | Cancel every in-flight job; returns the count cancelled |

`affinity` is `Literal["main", "any"]` — implementations are free to ignore
`"any"` and always run on the main thread. `timeout_ms=None` means no timeout
(host-defined behaviour for runaway scripts).

## BaseDccPump (cooperative drain)

Hosts that pump their own queue from an idle/tick callback should expose a
`drain_queue(budget_ms) -> DrainStats` method and a `stats` property:

```python
from dcc_mcp_core import BaseDccPump, DrainStats, PumpStats

class MyPump:
    def drain_queue(self, budget_ms: int) -> DrainStats: ...
    @property
    def stats(self) -> PumpStats: ...
```

`DrainStats(drained, elapsed_ms, overrun)` reports a single tick;
`PumpStats(ticks, drained, overrun_cycles)` is the cumulative counter.

## InProcessCallableDispatcher (reference)

```python
from dcc_mcp_core import InProcessCallableDispatcher, build_inprocess_executor

dispatcher = InProcessCallableDispatcher()
executor = build_inprocess_executor(dispatcher)
# Pass `executor` to McpHttpServer.set_in_process_executor / DccServerBase.register_inprocess_executor.

# Core calls the executor with metadata from ActionMeta/tools.yaml:
executor(
    "/path/to/script.py",
    {"root": "/show/shot010"},
    action_name="review__cache_manifest",
    skill_name="review",
    thread_affinity="any",
    execution="sync",
    timeout_hint_secs=None,
)

# Standalone fallback for mayapy / batch — runs scripts inline:
inline_executor = build_inprocess_executor(None)
```

Concrete production dispatchers (Maya UI thread, Houdini `hou.session` …)
typically subclass `InProcessCallableDispatcher` and override
`submit_callable` to enqueue onto the host's main-thread queue instead of
running inline. `cancel`, `shutdown`, and the per-job `JobEntry` bookkeeping
are inherited as-is.

## Per-job cancellation

`current_callable_job` is a `contextvars.ContextVar[JobEntry | None]` set by
`InProcessCallableDispatcher` for the duration of every submitted task.
Skill scripts can poll it without depending on an MCP request context:

```python
from dcc_mcp_core import current_callable_job, check_dcc_cancelled

def main(frames):
    for frame in frames:
        check_dcc_cancelled()      # honours both MCP token AND callable-job flag
        # equivalent manual probe:
        # job = current_callable_job.get()
        # if job is not None and job.cancelled: raise CancelledError()
        render_frame(frame)
```

`check_dcc_cancelled()` (cancellation API, #522) already routes through
`current_callable_job` in addition to the MCP `CancelToken` — **prefer it over
hand-rolled probes**.

## MinimalModeConfig (declarative startup)

```python
from dcc_mcp_core import MinimalModeConfig

CONFIG = MinimalModeConfig(
    skills=("scene_inspector", "render_queue"),     # full-load at startup
    deactivate_groups={"render_queue": ("submit",)}, # leave the `submit` group inactive
    env_var_minimal="DCC_MCP_MINIMAL",               # falsy → load every discovered skill
    env_var_default_tools="DCC_MCP_DEFAULT_TOOLS",   # comma/space-separated override
)
```

Resolution order, executed by `DccServerBase.register_builtin_actions`:

1. `env_var_default_tools` set & non-empty → load **only** those skills.
2. `env_var_minimal` set to `"0" / "false" / "no" / "off" / ""` → load **all** discovered skills.
3. Otherwise → load `skills` and apply `deactivate_groups`.

See [Server Factory API](./factory.md) for wiring `MinimalModeConfig` and the
in-process executor into `register_builtin_actions`.
