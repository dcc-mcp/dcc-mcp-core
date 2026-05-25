# Host Pattern Matrix

Use this table when choosing adapter runtime wiring for a new DCC.

| DCC family | Host API | Dispatcher approach | Notes |
|---|---|---|---|
| Blender | `bpy` | GUI timers or background blocking dispatcher | Keep `bpy` imports lazy so discovery works outside Blender. |
| 3ds Max | `pymxs` / MaxPlus | Main-thread dispatcher; Python entry from startup scripts or plugin bootstrap | Treat scene mutations as main-thread-only unless proven safe. |
| Unreal | Python, C++ plugin, or remote control | Prefer an editor plugin bridge; use Python only where deployed | Long operations should become async jobs with progress/cancellation. |
| ZBrush | ZScript, GoZ, HTTP/IPC helper | External bridge; no embedded Python assumption | Keep bridge commands typed and bounded; avoid generic remote execution. |
| Houdini | `hou` | Event-loop callback or headless hython dispatcher | Node graph writes are main-thread-sensitive. |
| Maya | `maya.cmds` / OpenMaya | UI dispatcher in GUI; standalone serialized dispatcher in mayapy | Do not special-case Maya patterns into core without parameterizing host identity. |
| Photoshop / Adobe | UXP/CEP/ExtendScript | External bridge or app UI contract | Use structured bridge calls; do not depend on a Python-in-host runtime. |
| Custom studio tool | Python, socket, HTTP, or CLI | Start with the least-powerful bridge that can satisfy typed tools | Document auth, scope, and shutdown behavior up front. |

## Host API Rules

- Import host modules inside callables or skill script entry points, never at
  package import time.
- Mark scene-touching tools `affinity: main`.
- Use `affinity: any` only for pure file, validation, serialization, or metadata
  operations.
- If a tool can exceed two seconds, declare `execution: async` and a realistic
  `timeout_hint_secs`.
- Long host loops must check core cancellation where available.
- Every bridge path must normalize arguments and return structured envelopes.

## Dispatcher Smoke Tests

Each adapter should have at least one smoke that proves:

- the server starts without a GUI import at discovery time;
- a main-affinity tool runs through the host dispatcher, not the HTTP worker;
- a pure `any` tool can run without blocking the host UI;
- cancellation or timeout produces a structured error;
- gateway REST or direct MCP can search, load, describe, and call one typed tool.
