# Adapter And Service Workflow

Use this reference to build a new adapter, expose an internal standalone
service, or simplify an existing integration.

## 1. Choose the Runtime Shape

Use the smallest shape that can honestly run the host API:

| Host shape | Typical DCCs | Recommended path |
|---|---|---|
| Embedded Python, GUI | Blender, Houdini, Maya, 3ds Max Python | `DccServerBase` with `HostExecutionBridge` and a host dispatcher |
| Embedded Python, headless | mayapy, Blender background, Houdini hython | `DccServerBase` with inline or blocking dispatcher |
| External bridge | ZBrush, Photoshop, Unity, proprietary tools | `DccServerBase` plus IPC/WebSocket/HTTP bridge helpers |
| Editor/game engine | Unreal, Unity | Adapter-owned plugin bridge plus typed skill tools; keep Python optional |
| Standalone internal service | Asset/review/render APIs, private CLI tools | `DccServerBase` with `instance_type="standalone"`, no DCC PID, and inline typed tools |

When an external bridge uses the public Python `DccBridge` WebSocket server,
declare `dcc-mcp-core[bridge]`; the base install intentionally does not pull in
the optional `websockets` transport.

## 2. Build the Composition Root

Adapter server modules should be composition roots, not utility bins. Keep them
responsible for wiring only:

- Resolve options with `DccServerOptions.from_env(...)`.
- Pass the adapter's bundled `skills/` directory.
- Attach `HostExecutionBridge` before skill discovery.
- Register resources, project tools, diagnostics, prompts, and adapter
  instruction resources before `start()`.
- Keep `start_server()` and `stop_server()` thin wrappers.

Minimal skeleton:

```python
from pathlib import Path

from dcc_mcp_core import DccServerBase, DccServerOptions, HostExecutionBridge


class MyDccServer(DccServerBase):
    def __init__(self, port: int | None = None, dispatcher=None, **kwargs):
        bridge = HostExecutionBridge(dispatcher=dispatcher) if dispatcher else None
        options = DccServerOptions.from_env(
            "mydcc",
            Path(__file__).parent / "skills",
            port=port,
            execution_bridge=bridge,
            **kwargs,
        )
        super().__init__(options=options)

    def _version_string(self) -> str:
        return "unknown"
```

Keep `port=None` as the adapter default. Core resolves an explicit argument
(including `0`) first, then `DCC_MCP_<DCC>_PORT`, then `0` so the OS assigns a
free loopback port. Clients discover the bound endpoint through FileRegistry
or the gateway; use an explicit argument or environment value only when a
fixed direct endpoint is required.

For `HostUiDispatcherBase` subclasses, the bridge creates and attaches the
native HTTP main-affinity queue automatically. Keep the host timer calling the
subclass's `drain_queue()`; do not wire a second queue in the adapter.

Guard the adapter's outer startup import with
`capture_bootstrap_errors(dcc_name, adapter_version=..., min_core_version=...)`.
It records pre-server failures and re-raises them for the host's native error
UI. `DccServerBase` captures Python logging and uncaught runtime exceptions
into the shared log plus `output://` / `events://`; forward host-native console
callbacks with `server.report_host_error(...)`. Do not install a second sink or
replace process-wide stdout/stderr.

For a non-DCC standalone service, keep the same composition root but omit
`HostExecutionBridge`, pass `instance_type="standalone"`, and leave `dcc_pid`
unset. Follow [INTERNAL_SERVICE_WORKFLOW.md](INTERNAL_SERVICE_WORKFLOW.md); a
public repository and public catalog entry are not required.

## 3. Add Progressive Skills

Use `MinimalModeConfig` for startup policy:

```python
from dcc_mcp_core import MinimalModeConfig

minimal = MinimalModeConfig(
    skills=("mydcc-scripting", "mydcc-scene"),
    deactivate_groups={"mydcc-scene": ("heavy",)},
    env_var_minimal="DCC_MCP_MYDCC_MINIMAL",
    env_var_default_tools="DCC_MCP_MYDCC_DEFAULT_TOOLS",
)
server.register_builtin_actions(minimal_mode=minimal)
```

Only eager-load the skills needed for discovery, diagnostics, and a first useful
scene query. Leave authoring, render, export, and pipeline skills loadable on
demand.

For ordinary standalone Python adapters installed in a virtual environment,
leave `DCC_MCP_PYTHON_EXECUTABLE` unset. The subprocess executor resolves an
explicit override first, then the active PyO3-attached `sys.executable` when it
is a real Python CLI, and finally `python` on `PATH` for pure-Rust callers. This
keeps adapter and Core packages visible to Skill scripts even when the virtual
environment is not first on `PATH`. Core does not auto-select host GUI binaries
such as Blender, Maya, FreeCAD, or OpenSCAD. Embedded hosts should keep using an
in-process `HostExecutionBridge`, or set an explicit vendor CLI such as
`mayapy`, `hython`, or `c4dpy` only when subprocess execution is intentional.

## 4. Publish Adapter Context

Prefer core-owned surfaces:

- `set_context_snapshot_provider(...)` for post-tool scene/document context.
- `register_adapter_instructions(...)` for adapter instruction resources.
- `register_project_tools(...)` for resumable project state.
- `server.resources()` or a public resource binder when available.
- `plugin_manifest(...)` for machine-readable install metadata.

If a needed adapter context requires private inner-server access, keep the shim
in one adapter-local module and open a core issue.

## 5. Decide What Belongs in Core

Escalate to core when more than one adapter would need the same helper:

- skill metadata lifecycle hooks;
- host-thread readiness bits;
- resource registration patterns;
- sidecar/install lifecycle;
- gateway search/describe/call response shape;
- UI Control automation contracts;
- file/artifact handoff;
- diagnostics, agent trace packets, compact debug negotiation, and issue-report exports.

Adapter repositories should contain host facts and host API calls. Core should
own reusable MCP, gateway, catalog, lifecycle, and wire contracts.

Admin issue reports are public-safe by default. They should expose request
status, DCC type, tool family, timing, sanitized error kind, token accounting,
redaction status, and relative debug links without raw payload previews or local
machine details. Full debug bundles remain a core-owned explicit raw export
(`?mode=raw`) for reviewed local evidence only.

Gateway/admin token accounting has two distinct concepts. Payload token fields
(`payload_token_usage`, `payload_token_accounting`, trace input/output tokens)
estimate captured request/response previews and must report missing coverage
explicitly. Response token accounting (`token_usage`, `response_token_accounting`,
original/returned/saved tokens) describes JSON/TOON compaction savings and must
not be used as a substitute for missing payload estimates.
