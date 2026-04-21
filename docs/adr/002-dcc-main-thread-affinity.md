# ADR 002 — DCC Main-Thread Affinity

- **Status**: Accepted
- **Date**: 2026-04-21
- **Related**: issue #315, #329 (`check_cancelled()`), #332 (`@chunked_job`)
- **Implements**: [`docs/guide/dcc-thread-safety.md`](../guide/dcc-thread-safety.md)

## Context

`dcc-mcp-core` must expose DCC (Digital Content Creation) scene APIs to
external AI agents over MCP Streamable HTTP. HTTP requests arrive on Tokio
worker threads, but every DCC host we target — Maya, Blender, Houdini,
3ds Max — enforces a hard contract that scene-mutating calls are only legal
on the host's **main thread**.

The contract is not advisory:

- Maya's `maya.cmds` / `OpenMaya` / PyMel are not thread-safe; the reference
  guide explicitly directs off-thread code through
  `maya.utils.executeDeferred`.
- Blender's `bpy` module asserts main-thread access and crashes the host
  otherwise; the supported escape hatch is `bpy.app.timers.register`.
- Houdini's `hou` module requires `hou.ui.addEventLoopCallback` for any
  scene mutation from a non-main thread.
- 3ds Max has no deferred primitive at all — all MAXScript / `pymxs` calls
  must originate on the main thread. Adapters synthesize a deferred
  primitive via a Qt single-shot timer.

Violating this contract produces one of three failure modes: silent
no-ops, memory corruption (the crash lands minutes later in unrelated
code), or hard segfaults. None is acceptable for a library that external
AI agents drive programmatically.

This ADR is non-negotiable because it is externally imposed by each DCC's
runtime. Our only design freedom is **how** we bridge worker threads to
the main thread, not whether we do.

## Decision

Adopt a single canonical bridge for all adapters:

1. **`DeferredExecutor`** owns a bounded `tokio::sync::mpsc::channel` of
   `DccTaskFn` closures. Source of truth:
   `crates/dcc-mcp-http/src/executor.rs`.
2. **Tokio workers submit** tasks via `DccExecutorHandle::execute(fn)` and
   `await` a `oneshot` reply channel. They never touch the scene API
   directly.
3. **The DCC main thread drains** the channel from its event loop by
   calling `poll_pending_bounded(max=N)` from whatever deferred primitive
   the host provides (`executeDeferred`, `bpy.app.timers`, etc.).
4. **Long-running jobs are chunked** into per-tick units. The expected
   chunking contract will be encoded by the `@chunked_job` decorator
   (#332) and the `check_cancelled()` primitive (#329).

The public Python surface is `DeferredExecutor(capacity=N)` with
`.execute(callable)` and `.poll_pending_bounded(max=N)`. `poll_pending()`
remains available but documented as a footgun (see consequences below).

Job scheduling above this bridge is expressed through
`crates/dcc-mcp-process/src/dispatcher.rs`:

- `ThreadAffinity::Main` → routed through `DeferredExecutor`.
- `ThreadAffinity::Any` → executed on Tokio workers directly.
- `ThreadAffinity::Named(_)` → host-managed worker pool (adapter-specific).

## Consequences

### Positive

- Exactly one thread-safety story across all adapters — easy to audit.
- Bounded channel provides natural back-pressure when the main thread
  falls behind.
- The `DccTaskFn` boundary is a natural sandbox seam: every main-thread
  call is wrapped and can be instrumented (tracing, telemetry, audit).

### Negative

- **Latency tax.** Every scene call pays at least one channel hop +
  one tick of event-loop wait time. A `polySphere()` that would be
  ~1 ms in-process becomes ~16 ms end-to-end at 60 FPS.
- **Per-DCC callback registration.** Each adapter must wire
  `poll_pending_bounded` into the host's deferred primitive. There is no
  universal abstraction; 3ds Max in particular has no native defer
  primitive and needs a Qt timer shim.
- **`poll_pending()` is a footgun.** Unbounded drains on a main thread
  freeze the UI under load. We keep the method because some legitimate
  teardown paths need it, but the guide directs all production code to
  `poll_pending_bounded(max=N)`.
- **Long-running jobs cannot be expressed as a single `DccTaskFn`.**
  They must be chunked and cooperatively cancellable — which forces
  skill authors to think about scheduling. This is a feature, not a bug,
  but it raises the floor for skill authoring.

## Alternatives considered

### A. Native OS threads with scene-API locks

*Rejected.* The DCC scene APIs are not merely unlocked — they are
affirmatively not thread-safe. Even a global mutex around every scene
call does not help, because internal DCC state (undo stacks, UI redraw,
dependency graphs) is keyed on thread-local context that only exists on
the main thread. Adding locks would give us data races on internal state
that we do not own and cannot fix.

### B. Pure event-loop polling (no channel)

*Rejected.* A `while True: poll()` loop on the main thread pegs a CPU
core and still cannot escape the deferred-primitive constraint on hosts
like Blender and Houdini where the host's own event loop is authoritative.
It also precludes bounded back-pressure — the Tokio side has no way to
signal "the main thread is overloaded, slow down."

### C. Coroutine-based scheduler (asyncio on the main thread)

*Rejected.* Maya, Blender, and Houdini do not expose their event loops
as asyncio-compatible. Embedding a nested asyncio loop that yields to the
host loop is technically possible but requires bespoke integration per
DCC (patching `selectors`, forwarding signal handlers, etc.), and none of
the DCC vendors support that configuration. The interop cost exceeds the
benefit of the ergonomic gain.

### D. Rendering through a separate out-of-process DCC worker

*Deferred, not rejected.* For truly heavyweight jobs (multi-hour batch
renders) we may eventually launch a headless DCC subprocess and proxy
results back. That is a feature of the process layer, not a replacement
for main-thread affinity in the interactive case, and is out of scope
for this ADR.

## References

- [`docs/guide/dcc-thread-safety.md`](../guide/dcc-thread-safety.md) —
  usage guide for adapter and skill authors.
- `crates/dcc-mcp-http/src/executor.rs` — `DeferredExecutor`
  implementation.
- `crates/dcc-mcp-process/src/dispatcher.rs` — `ThreadAffinity`,
  `JobRequest`, `HostDispatcher` trait.
- [`skills/integration-guide.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/skills/integration-guide.md)
  — per-DCC bridge patterns.
