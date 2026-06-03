# Callable Dispatcher API

DCC-neutral protocols for **routing in-process Python skill scripts onto a host's
UI / main thread**, plus a declarative **minimal-mode skill loader** for embedded
DCCs (issues #520, #521, #525).

Every embedded DCC plugin (Maya, Houdini, Unreal, Blender Python …) re-implements
the same pattern: receive an MCP `tools/call`, route the script to the host's
event loop, return a JSON-serialisable result. This module lifts that pattern
into reusable contracts so each adapter only supplies host-specific glue.

**Exported symbols:** `BaseDccCallableDispatcher`, `InProcessExecutionContext`,
`BaseDccCallableDispatcherFull`, `BaseDccPump`, `AdaptivePumpPolicy`,
`AdaptivePumpStats`, `HostUiDispatcherBase`, `HostUiJobEntry`,
`DispatcherErrorCode`, `host_ui_outcome`, `InProcessCallableDispatcher`,
`JobEntry`, `JobOutcome`, `PendingEnvelope`, `DrainStats`, `PumpStats`,
`current_callable_job`, `current_host_ui_job`, `MinimalModeConfig`,
`build_inprocess_executor`, `run_skill_script`, `SidecarActionDispatcher`,
`SidecarDispatchRequest`, `HostPumpController`, `HostPumpSnapshot`,
`ManualHostTimerAdapter`, `ThreadedHostTimerAdapter`, `QtHostTimerAdapter`.

## When to use what

| Need | Use this |
|------|----------|
| Interactive DCC (Maya UI, Blender UI, Houdini desktop) | `HostUiDispatcherBase` — subclass + `poke_host_pump()` + host timer/`BaseDccPump` |
| Wire a single `dispatch_callable(func, *args, **kwargs)` shim | `BaseDccCallableDispatcher` (#521) |
| Full submit / cancel / shutdown contract | `BaseDccCallableDispatcherFull` (#520) |
| Sidecar-to-DCC transport for Qt-bearing hosts | `dcc_mcp_core.qt_dispatcher.start_qt_server` + `qtserver://` |
| Script-backed sidecar dispatch handler | `SidecarActionDispatcher` (#1274) |
| Batch / `mayapy` / pytest (no UI thread) | `InProcessCallableDispatcher` only — **do not** subclass `HostUiDispatcherBase` |
| Cooperative idle-tick that drains a queue (Maya `scriptJob(event=['idle', …])`) | `BaseDccPump` (#520) |
| Shared active/idle timer backoff for host pump callbacks | `AdaptivePumpPolicy` (#606) |
| Shared pump/timer lifecycle for UI hosts | `HostPumpController` + a `HostPumpTimerAdapter` (#1276) |
| Reference single-thread implementation for `mayapy` / pytest / batch | `InProcessCallableDispatcher` |
| Per-job cancellation handle reachable from skill scripts | `current_callable_job` ContextVar |
| Declarative progressive skill loading at startup | `MinimalModeConfig` (#525) |

## Universal Qt Dispatcher

Qt-bearing DCCs (Maya, Houdini, 3ds Max, Nuke, Cinema 4D, Substance Painter,
Mari, and similar hosts) should use the public package module instead of
vendoring a local copy of the JSON-line TCP server:

```python
from dcc_mcp_core.qt_dispatcher import start_qt_server

handle = start_qt_server(
    port=0,
    dispatch_handler=lambda payload: execute_action(
        payload["action"],
        payload.get("args") or {},
        request_id=payload.get("request_id"),
    ),
    session_info_provider=lambda: {"dcc": "maya"},
)

print(handle.url)      # qtserver://127.0.0.1:<port>
handle.close()
```

`dcc-mcp-host-rpc` embeds a Cargo-package mirror of this public module for the
lazy `commandPort` bootstrap, and CI asserts the mirror matches byte-for-byte.
Adapter imports and the Rust `qtserver://` client therefore share one
implementation contract. `start_qt_server()` returns a dict-compatible
`ServerHandle` for backwards compatibility with the bootstrap JSON path while
also exposing `handle.port`, `handle.url`, and `handle.close()`.

Built-in request methods are `ping`, `dispatch`, `execute`,
`get_session_info`, `install_stream_capture`, `get_buffered_output`, and
`create_module`. Adapters should pass a `dispatch_handler` for script-backed
actions and keep host-specific registration glue outside the dispatcher.

Migration notes:

- Maya should replace vendored `_qt_dispatcher.py` copies with
  `dcc_mcp_core.qt_dispatcher.start_qt_server(...)`; Maya-specific code should
  only resolve the running server, register the action dispatch callback, and
  provide session metadata.
- 3ds Max should replace custom `sidecar/qt_bridge.py` TCP request handling
  with the same dispatcher and keep .NET/MaxScript timer or executor glue in
  the adapter repository.
- Other Qt DCCs should follow the same rule: core owns the JSON-line server,
  request envelopes, stream capture, session info, and `qtserver://`
  compatibility; adapters own host lifecycle and action execution.

## SidecarActionDispatcher

Use `SidecarActionDispatcher` behind a Qt sidecar `dispatch_handler` when the
sidecar receives script-backed skill calls shaped like
`{"action": "...", "args": {...}, "request_id": "...", "source_file": "..."}`.
Core validates the payload, checks that an adapter server is running, resolves a
registered action or bundled skill `source_file`, executes through an adapter
hook, and normalizes the result envelope.

```python
from dcc_mcp_core.qt_dispatcher import start_qt_server
from dcc_mcp_core.sidecar import SidecarActionDispatcher

dispatcher = SidecarActionDispatcher(
    "maya",
    server_provider=get_running_server,
    action_resolver=resolve_registered_action,
    executor=SidecarActionDispatcher.maya_executor(execute_in_process),
    bundled_skill_roots=[bundled_skill_root],
)

handle = start_qt_server(
    port=0,
    dispatch_handler=dispatcher.dispatch_payload,
    session_info_provider=lambda: {"dcc": "maya"},
)
```

For adapters that already expose a direct host RPC method (for example a native
`HostRpcClient` call that does not execute skill scripts), keep using that
direct client. `SidecarActionDispatcher` is specifically for the script-backed
skill path where adapter implementations otherwise repeat the same validation,
action lookup, source-file resolution, error-code mapping, and JSON-safe result
wrapping.

Supported executor hooks:

- Maya-style:
  `SidecarActionDispatcher.maya_executor(execute_in_process)` adapts
  `execute_in_process(server, script_path, args, action_name)`.
- Script-runner style:
  `SidecarActionDispatcher.script_executor(run_skill_script)` adapts
  `run_skill_script(script_path, args)` for 3ds Max-like sidecars.
- Custom:
  pass `executor(request)` and read `SidecarDispatchRequest` fields such as
  `server`, `action`, `args`, `script_path`, `skill_name`, `thread_affinity`,
  `execution`, and `timeout_hint_secs`.

Standard adapter-facing error codes are `server-not-running`,
`payload-malformed`, `unknown-action`, `no-source-file`, and `dispatch-failed`.
They follow the existing result-envelope shape:
`{"success": false, "message": "...", "error": "<code>", "context": {...}}`.

For adapter migration decision tables, fake-adapter conformance fixtures, and
Maya / 3ds Max / Blender / Houdini checklists, see
[`docs/guide/adapter-dispatcher-migration.md`](../guide/adapter-dispatcher-migration.md).

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

## AdaptivePumpPolicy

`AdaptivePumpPolicy` gives embedded adapters the same active/idle timing rules
while leaving the host-specific timer install in adapter code:

```python
from dcc_mcp_core import AdaptivePumpPolicy

policy = AdaptivePumpPolicy(
    active_interval_secs=0.05,
    idle_interval_secs=1.0,
    idle_delay_secs=5.0,
    max_client_idle_secs=10.0,
)

def blender_timer_callback():
    stats = dispatcher.drain_queue(budget_ms=8)
    policy.record_tick(
        drained=stats.drained,
        elapsed_ms=stats.elapsed_ms,
        overrun=stats.overrun,
    )
    return policy.next_interval(
        has_pending=dispatcher.has_pending(),
        deferred_pending=render_job_is_running(),
    )
```

Use `mark_work_done(drained=N)` as shorthand when the adapter only knows that
work completed. Use `mark_client_activity()` when a new MCP request or plugin
event should keep the pump responsive for `max_client_idle_secs`.

`policy.stats` exposes cumulative `ticks`, `drained_jobs`, `overrun_cycles`,
`active_transitions`, `idle_transitions`, `mode`, and `last_interval_secs`.

## HostPumpController

`HostPumpController` composes a dispatcher/pump with the host's timer primitive.
Use it when an adapter already has a `HostUiDispatcherBase`, `BaseDccPump`, or
other object exposing `drain_queue(budget_ms)`, and you want core to own
install/uninstall idempotency, schedule-soon behavior, active/idle backoff,
budget accounting, and common stats.

```python
from dcc_mcp_core import HostPumpController, QtHostTimerAdapter

dispatcher = MyDccDispatcher()  # usually a HostUiDispatcherBase subclass
controller = HostPumpController(
    dispatcher,
    QtHostTimerAdapter(),
    budget_ms=8,
)
controller.start()

# Later, when the adapter shuts down:
controller.stop()
```

Built-in timer adapters:

- `ManualHostTimerAdapter` stores the tick callback and exposes `fire()` for
  deterministic unit tests and adapter conformance fixtures.
- `ThreadedHostTimerAdapter` uses `threading.Timer` for standalone/headless
  integration tests where no DCC UI loop exists.
- `QtHostTimerAdapter` uses a single-shot `QTimer`, probing PySide6, PyQt6,
  PySide2, then PyQt5 when a `qt_core` object is not supplied.

Migration mapping:

- Maya: implement a tiny timer adapter that registers `tick()` with
  `cmds.scriptJob(event=["idle", ...])` or `maya.utils.executeDeferred`, and
  call `controller.schedule_soon()` when new MCP work arrives.
- 3ds Max: map the existing .NET timer / rollout tick to
  `HostPumpTimerAdapter.install`, `.uninstall`, and `.schedule_soon`; keep job
  lifecycle in `HostUiDispatcherBase`.
- Blender: adapt `bpy.app.timers.register` / `unregister` to the timer adapter
  contract, returning the controller-provided next interval.
- Houdini, Nuke, Cinema 4D, Substance Painter, Mari: use `QtHostTimerAdapter`
  when the DCC exposes a compatible Qt event loop.

`HostPumpController.stats` returns `HostPumpSnapshot` with `queue_size`,
`active_jobs`, `interval_secs`, `overrun_count`, `last_tick_time`, `shutdown`,
`ticks`, `drained_jobs`, and `last_elapsed_ms`.

## InProcessCallableDispatcher (reference)

```python
from dcc_mcp_core import InProcessCallableDispatcher, build_inprocess_executor

dispatcher = InProcessCallableDispatcher()
executor = build_inprocess_executor(dispatcher)
# Pass `executor` to McpHttpServer.set_in_process_executor / DccServerBase.register_inprocess_executor.

# Core calls the executor with metadata from ToolMeta/tools.yaml:
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

## HostUiDispatcherBase Extension Hooks

Interactive adapters should prefer `HostUiDispatcherBase` over local queue/job
implementations. The base owns queueing, sync/async submission, cooperative
cancellation, timeout waits, shutdown cleanup, active-job tracking, and standard
dict outcomes. DCC-specific subclasses should implement `poke_host_pump()` and
override only the small extension hooks they need:

- `format_exception_error(exc)` converts task exceptions into adapter-specific
  error strings.
- `format_timeout_error(request_id, affinity, timeout_sec)` converts sync
  main-thread timeout failures.
- `on_job_queued(job)`, `on_job_started(job)`, and `on_job_finished(job)` attach
  logging, metrics, or host diagnostics without owning queue internals.
- `dispatcher_label` is the human-readable label used in shared logs; pass
  `label="3dsmax-ui"` or similar to the constructor.
- `queue_size()` and `active_count()` expose safe read-only visibility for
  pump controllers, health checks, and adapter tests.

3ds Max migration note: replace local job-entry / queue / shutdown code with a
`HostUiDispatcherBase` subclass, move .NET timer or rollout tick glue into a
`HostPumpTimerAdapter`, and keep only Max-specific error formatting or log
labels in the hooks above. Blender and Houdini follow the same split: base
dispatcher for job lifecycle, timer adapter for host loop integration.

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

## Host UI dispatcher checklist (adapter authors)

Use this path for **every embedded interactive host** so each adapter only
implements host-specific pump glue:

1. **Subclass** `HostUiDispatcherBase` and implement `poke_host_pump()` (Maya:
   `maya.utils.executeDeferred`; Blender: `bpy.app.timers.register`; …).
2. **Install a pump** that calls `drain_queue(budget_ms)` on idle ticks
   (`BaseDccPump` + `AdaptivePumpPolicy` optional but recommended).
3. **Register** the dispatcher on `HostExecutionBridge` /
   `DccServerBase.register_inprocess_executor` before `server.start()`.
4. **Skill scripts** call `check_dcc_cancelled()` in long loops — the base
   publishes `HostUiJobEntry` through `set_current_job` during `execute()`.
5. **Outcomes** use the dict envelope from `host_ui_outcome()` /
   `DispatcherErrorCode` (`Cancelled`, `Interrupted`, `host-busy`, …).
6. **Optional** `fail_fast_on_main_queue_busy=True` when the host must reject
   sync main-thread work while the queue is non-empty (orchestrator back-pressure).

Do **not** fork a second queue/cancel protocol per DCC — extend the base and
keep Maya-specific code in `poke_host_pump` and logging only.

Adapter repositories should carry a fake conformance test before live-host
smokes. The core reference is
[`tests/test_dispatcher_migration_conformance.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/tests/test_dispatcher_migration_conformance.py):
it exercises Maya-like and 3ds Max-like dispatch flows, malformed payloads,
missing servers, missing source files, executor errors, cancellation, timeout,
and shutdown cleanup without launching a real DCC.
