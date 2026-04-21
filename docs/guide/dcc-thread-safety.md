# DCC Thread Safety

> **Audience**: adapter authors (`dcc-mcp-maya`, `dcc-mcp-blender`, ...) and
> skill authors who run long computations inside a DCC host.
>
> **TL;DR**: every scene-mutating call must run on the DCC's **main thread**.
> `DeferredExecutor` is the only supported bridge between Tokio HTTP workers
> and that main thread. Long-running jobs must be chunked into per-tick units
> and must use `poll_pending_bounded(max=N)`, never `poll_pending()`.

## Why main-thread affinity exists

Every major DCC host enforces main-thread-only access to its scene-mutating
API. The runtime does **not** protect you with locks — calling those APIs from
a worker thread corrupts scene state, segfaults the host, or silently does
nothing. Each DCC ships a canonical "defer to the main thread" primitive:

| DCC | Main-thread-only API | Canonical defer primitive |
|------|-----------------------|---------------------------|
| Maya | `maya.cmds`, `OpenMaya`, `pymel` | `maya.utils.executeDeferred(fn)` / `maya.cmds.evalDeferred("expr")` |
| Blender | `bpy.ops`, `bpy.data`, `bpy.context` | `bpy.app.timers.register(fn, first_interval=0.0)` |
| Houdini | `hou.*` (scene graph, SOPs, HDAs) | `hou.ui.addEventLoopCallback(fn)` |
| 3ds Max | `MaxPlus`, `pymxs.runtime`, MAXScript | `pymxs.runtime.execute("...")` only from main thread (no deferred primitive; use a Qt singleshot timer) |

These primitives all share the same contract:

1. The callable is queued.
2. The DCC event loop invokes it from the main thread at the next safe tick.
3. The callable runs to completion **synchronously** and **blocks the UI**.

Point (3) is why this guide exists: the moment your callable takes more than
~16 ms, the DCC UI stutters; beyond a few hundred ms, the host appears frozen.

## How `DeferredExecutor` bridges Tokio workers to the main thread

`McpHttpServer` accepts HTTP requests on Tokio worker threads. The worker
must **not** touch the scene API directly — instead it submits a task through
`DeferredExecutor`, which parks the task on an `mpsc::channel` and returns a
future. The DCC event loop drains that channel on the main thread.

```text
   Tokio worker (HTTP handler)                DCC main thread
   ───────────────────────────                ─────────────────
   handle.execute(task) ──── mpsc::channel ──► poll_pending_bounded(max=8)
           │                      (bounded)            │
           │                                           │ run task_fn()
           ▼                                           ▼
   await oneshot ◄──────────── oneshot::channel ── send(result)
```

The Rust source of truth for this bridge is small enough to read in one sit:

```rust
// crates/dcc-mcp-http/src/executor.rs (L23-L111)
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A boxed async-compatible task that runs on the DCC main thread.
pub type DccTaskFn = Box<dyn FnOnce() -> String + Send + 'static>;
// ...
impl DeferredExecutor {
    pub fn poll_pending(&mut self) -> usize { /* drain all */ }
    pub fn poll_pending_bounded(&mut self, max: usize) -> usize { /* drain <= max */ }
}
```

The job-dispatcher layer that higher-level adapters implement lives in:

```rust
// crates/dcc-mcp-process/src/dispatcher.rs (L1-L166)
pub enum ThreadAffinity { Main, Named(&'static str), Any }
pub struct JobRequest { /* request_id, affinity, timeout_ms, task */ }
pub trait HostDispatcher {
    fn submit(&self, req: JobRequest) -> oneshot::Receiver<ActionOutcome>;
    fn supported(&self) -> &[ThreadAffinity];
    fn capabilities(&self) -> HostCapabilities;
}
```

When a skill tool is marked `ThreadAffinity::Main`, the adapter routes it
through `DeferredExecutor`; `ThreadAffinity::Any` jobs run on Tokio workers
directly.

### Python usage

```python
from dcc_mcp_core._core import DeferredExecutor  # not yet in public __init__

executor = DeferredExecutor(capacity=16)

# From any thread (e.g. MCP HTTP handler):
executor.execute(lambda: maya.cmds.polySphere(radius=1.0))

# In the DCC idle callback:
executor.poll_pending_bounded(max=8)  # bounded per tick — see below
```

## Rules for long-running jobs

A "long-running job" is anything that cannot complete inside one DCC tick
(~16 ms at 60 FPS, ~33 ms at 30 FPS). Examples: playblast, batch render,
thousands of scene-graph edits, USD composition, heavy geometry generation.

Three non-negotiable rules:

### 1. Chunk work into per-tick units

Render one frame per timer tick, not all frames in one call. Process
geometry in batches of N primitives, not the whole mesh. The chunk size
should leave the DCC at least 50 % of each tick for the UI.

```python
# Good: one frame per Blender timer tick
frame_iter = iter(range(1, 241))

def render_next():
    try:
        frame = next(frame_iter)
    except StopIteration:
        return None  # unregister timer
    bpy.context.scene.frame_set(frame)
    bpy.ops.render.render(write_still=True)
    return 0.0  # reschedule immediately

bpy.app.timers.register(render_next)
```

### 2. Cooperative checkpoints

Between chunks, check a cancellation flag and yield control back to the
DCC. See [issue #329 — `check_cancelled()`](https://github.com/loonghao/dcc-mcp-core/issues/329)
for the planned cooperative-cancellation primitive.

```python
for batch in chunks(primitives, size=500):
    if job.check_cancelled():           # #329
        return skill_error("Cancelled by user")
    create_primitives(batch)
    # control returns to DCC between batches
```

### 3. Use `poll_pending_bounded(max=N)`, never `poll_pending()`

`poll_pending()` drains **every** queued task before returning — if 50 tasks
arrive simultaneously, the DCC freezes for the sum of their runtimes.
`poll_pending_bounded(max=N)` caps each pump to `N` tasks so the event loop
can redraw between batches.

```python
# ❌ bad — unbounded; a burst of tasks will freeze the UI
executor.poll_pending()

# ✅ good — bounded; up to 8 tasks per tick, worst-case latency is known
executor.poll_pending_bounded(max=8)
```

A reasonable starting value is `max=8` at 60 FPS; tune down if individual
tasks are expensive.

The chunked-job decorator described in
[issue #332 — `@chunked_job`](https://github.com/loonghao/dcc-mcp-core/issues/332)
will encode rules (1) and (2) automatically once it lands.

## Forbidden patterns

### `time.sleep()` inside a `DccTaskFn`

A `DccTaskFn` runs on the DCC main thread. `time.sleep(n)` blocks that
thread — the host freezes for `n` seconds.

```python
# ❌ freezes Maya for 5 seconds
executor.execute(lambda: (time.sleep(5), cmds.polySphere()))
```

If you need a delay, reschedule via the DCC's own timer primitive
(`maya.utils.executeDeferred`, `bpy.app.timers.register`, etc.) and return
control to the event loop.

### Native OS threads from a skill script for scene ops

Spawning `threading.Thread` and calling `maya.cmds` / `bpy.ops` from it
bypasses the main-thread contract entirely. Even if it appears to work in
testing, it will segfault or corrupt state under load.

```python
# ❌ Undefined behaviour — Maya API is not thread-safe
threading.Thread(target=lambda: cmds.polySphere()).start()
```

Use `DeferredExecutor.execute(...)` instead — it is the only thread-safe
path into the scene API.

### Blocking I/O on the main thread

`requests.get(url)`, `urllib.urlopen(...)`, synchronous database calls,
large file reads — none of these belong in a `DccTaskFn`. They block the
event loop exactly like `time.sleep`.

```python
# ❌ blocks the DCC UI for the duration of the HTTP round-trip
executor.execute(lambda: json.loads(requests.get(url).text))
```

Perform I/O on the Tokio worker (before submitting), then pass the
already-resolved payload into the `DccTaskFn`:

```python
# ✅ I/O on the worker; only the scene call is deferred
payload = requests.get(url).json()          # Tokio worker
executor.execute(lambda: apply_to_scene(payload))  # main thread
```

## See also

- [ADR 002 — DCC Main-Thread Affinity](../adr/002-dcc-main-thread-affinity.md)
  — the architectural rationale for this design.
- [Getting Started → DeferredExecutor](./getting-started.md#deferredexecutor-dcc-main-thread-safety)
  — minimal "hello world" example.
- [`skills/integration-guide.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/skills/integration-guide.md)
  — per-DCC bridge patterns (embedded Python / WebSocket / WebView).
- [Issue #329 — `check_cancelled()`](https://github.com/loonghao/dcc-mcp-core/issues/329)
  — cooperative cancellation for chunked jobs.
- [Issue #332 — `@chunked_job`](https://github.com/loonghao/dcc-mcp-core/issues/332)
  — decorator that encodes the chunking + checkpoint rules.
