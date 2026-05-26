# Adapter Dispatcher Migration

Use this guide when moving a DCC adapter from local dispatcher copies to the
shared core primitives introduced by #1273, #1274, #1275, and #1276. The goal is
that adapters keep only host lifecycle glue while `dcc-mcp-core` owns request
validation, queueing, cancellation, timeout handling, pump lifecycle, and
standard result envelopes.

## Decision Table

| Adapter shape | Use | Adapter still owns |
|---------------|-----|--------------------|
| Qt-bearing DCC sidecar (Maya, Houdini, 3ds Max, Nuke, Cinema 4D, Substance Painter, Mari) | `dcc_mcp_core.qt_dispatcher.start_qt_server` and the `qtserver://` Rust client | Plugin startup/shutdown, session metadata, and the host action callback |
| Script-backed sidecar action dispatch | `SidecarActionDispatcher` as the Qt dispatch handler | `server_provider`, `action_resolver`, and an executor hook such as `maya_executor(...)` or `script_executor(...)` |
| Interactive UI host with main-thread affinity | `HostUiDispatcherBase` subclass plus `HostPumpController` | `poke_host_pump()` and a tiny `HostPumpTimerAdapter` for the host timer primitive |
| Non-Qt UI host with a native timer | `HostUiDispatcherBase` plus a custom `HostPumpTimerAdapter` | Timer install/uninstall/schedule mapping only |
| Headless or batch host (`mayapy`, `hython`, pytest) | `InProcessCallableDispatcher` or `DccServerBase.register_inprocess_executor(None)` | Verifying the host process is safe to call inline |
| Native host RPC that does not run skill scripts | Keep the `HostRpcClient` implementation | Native protocol framing and host-specific RPC errors |
| Adapter conformance tests | `ManualHostTimerAdapter` and fake sidecar/server fixtures | Only assertions for adapter-specific metadata and host callback wiring |

Do not vendor `_qt_dispatcher.py`, `qt_bridge.py`, queue implementations,
cancel flags, timeout loops, or sidecar payload validators in adapter
repositories after these primitives are available.

## Migration Checklist

1. Inventory local dispatcher files and classify each one with the decision
   table above.
2. Replace JSON-line Qt server copies with
   `dcc_mcp_core.qt_dispatcher.start_qt_server(...)`.
3. Compose `SidecarActionDispatcher` for script-backed skill actions. Keep
   action lookup in the adapter, but let core normalize payloads, missing-source
   errors, executor exceptions, and JSON-safe result envelopes.
4. Replace local UI-thread job queues with a `HostUiDispatcherBase` subclass.
   Implement only `poke_host_pump()` and optional diagnostics hooks such as
   `format_exception_error`, `format_timeout_error`, `on_job_queued`,
   `on_job_started`, and `on_job_finished`.
5. Move timer lifecycle into `HostPumpController`. Map the host's idle callback,
   .NET timer, Blender timer, or Qt `QTimer` to a `HostPumpTimerAdapter`.
6. Wire readiness only after the dispatcher can actually run main-thread work.
   Adapter smoke tests may require `host_execution_bridge` and
   `main_thread_executor` readiness bits.
7. Add or update adapter conformance tests using fake servers and
   `ManualHostTimerAdapter` before touching a live DCC. Use the core fixture in
   `tests/test_dispatcher_migration_conformance.py` as the minimum contract.
8. Run one live smoke in the adapter repository when the host is available. If
   not, document the gap in the PR and keep the fake conformance path runnable in
   CI.

## Core Conformance Fixture

`tests/test_dispatcher_migration_conformance.py` models two adapter families:

- A Maya-like Qt sidecar that uses `SidecarActionDispatcher`, resolves bundled
  skill scripts, executes through `HostUiDispatcherBase`, and drives the pump
  through `HostPumpController`.
- A 3ds Max-like script sidecar that passes an explicit `source_file` to
  `SidecarActionDispatcher.script_executor(...)`.

The fixture covers successful dispatch, malformed payloads, missing servers,
unknown actions, missing source files, executor failures, cancellation, timeout,
and shutdown cleanup. Adapter repositories should copy the shape of the tests,
not the core implementation code.

## Adapter Notes

### Maya

Use `start_qt_server(...)` for the sidecar endpoint and keep the Maya plugin
responsible for process lifecycle, action registration, and session metadata.
Route script-backed skills through `SidecarActionDispatcher.maya_executor(...)`.
When a skill requires the UI thread, use a `HostUiDispatcherBase` subclass whose
`poke_host_pump()` maps to Maya's idle or deferred execution primitive.

### 3ds Max

Replace local TCP/JSON bridge code with the shared Qt dispatcher when Qt is
available. Keep MaxScript or .NET timer glue in a `HostPumpTimerAdapter`, route
script-backed skills through `SidecarActionDispatcher.script_executor(...)`, and
put Max-specific diagnostics in dispatcher hooks instead of queue internals.

### Blender

Use `HostUiDispatcherBase` for UI-thread work and adapt `bpy.app.timers` to the
timer adapter contract. Batch Blender or pytest paths should use
`InProcessCallableDispatcher` instead of pretending a UI pump exists.

### Houdini

Qt-bearing Houdini sessions can use `QtHostTimerAdapter` and
`start_qt_server(...)`. Headless `hython` paths should stay inline only after the
adapter verifies the DCC APIs being called are safe without a UI loop.
