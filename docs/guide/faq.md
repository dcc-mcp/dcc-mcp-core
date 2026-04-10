# FAQ

Frequently asked questions about DCC-MCP-Core.

## General

### What is DCC-MCP-Core?

DCC-MCP-Core is a foundational Rust library with Python bindings that provides:

- **ActionRegistry** — Thread-safe action registration and lookup
- **SkillCatalog** — Progressive skill discovery and loading; scripts auto-registered as MCP tools via SKILL.md
- **EventBus** — A publish-subscribe event system for DCC lifecycle hooks
- **MCP Protocol Types** — Type definitions for the Model Context Protocol (Tools, Resources, Prompts)
- **Transport Layer** — IPC and network communication for distributed DCC integration
- **MCP HTTP Server** — A streamable HTTP server exposing DCC tools to AI clients

### What DCC applications are supported?

dcc-mcp-core is DCC-agnostic — the core library provides the infrastructure. DCC-specific integrations are separate projects:

- **Maya** — via [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya)
- **Blender, Houdini, 3ds Max, Unreal** — community/third-party integrations using this library

The core library works with any Python 3.7+ environment.

### What Python versions are supported?

Python 3.7–3.13 are tested in CI. Wheels are built with `abi3-py38` for maximum compatibility.

### Does it have any Python runtime dependencies?

**No.** The library has zero Python runtime dependencies. Everything is compiled into the Rust core.

## Installation

### How do I install dcc-mcp-core?

**From PyPI:**
```bash
pip install dcc-mcp-core
```

**From source (requires Rust 1.85+ and maturin):**
```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install maturin
maturin develop
```

## Actions

### How do I register an action?

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher
import json

reg = ActionRegistry()

# Register action metadata with an optional JSON Schema
reg.register(
    name="create_sphere",
    description="Create a polygon sphere",
    category="geometry",
    tags=["create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {"radius": {"type": "number", "minimum": 0.0}},
    }),
)

# Attach a Python handler
dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"name": "sphere1"})
result = dispatcher.dispatch("create_sphere", '{"radius": 1.0}')
print(result["output"])  # {"name": "sphere1"}
```

### How do I return structured results from an action?

```python
from dcc_mcp_core import success_result, error_result, from_exception

# Success
result = success_result("Sphere created", context={"name": "sphere1"})
print(result.success)   # True
print(result.context)   # {"name": "sphere1"}

# Error
result = error_result("Failed to create sphere", error="No active scene")
print(result.success)   # False

# From exception
try:
    raise ValueError("radius must be > 0")
except Exception:
    result = from_exception("Invalid radius")
```

### How do I validate action input?

```python
from dcc_mcp_core import ActionValidator

validator = ActionValidator.from_schema_json('{"type":"object","required":["radius"],"properties":{"radius":{"type":"number"}}}')
ok, errors = validator.validate('{"radius": 1.0}')
assert ok

ok, errors = validator.validate('{}')
assert not ok
print(errors)  # ['missing required field: radius']
```

## Events

### How does the event system work?

```python
from dcc_mcp_core import EventBus

bus = EventBus()

# Subscribe — returns a subscription ID
def on_save(file_path: str):
    print(f"Saving to: {file_path}")

sub_id = bus.subscribe("dcc.save", on_save)

# Publish
bus.publish("dcc.save", file_path="/tmp/scene.usd")

# Unsubscribe
bus.unsubscribe("dcc.save", sub_id)
```

::: warning Async handlers
The EventBus does not natively support `async def` callbacks. Wrap async logic in a synchronous handler that schedules it with your event loop.
:::

## Skills

### What is the quickest way to expose scripts as MCP tools?

Use `create_skill_manager` (v0.12.12+) — one call does everything:

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/skills"
server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())  # http://127.0.0.1:8765/mcp
```

This automatically creates an `ActionRegistry`, `ActionDispatcher`, `SkillCatalog`, and `McpHttpServer`, and discovers skills from `DCC_MCP_MAYA_SKILL_PATHS` and `DCC_MCP_SKILL_PATHS`.

### What is the Skills system?

The Skills system allows zero-code script registration. Place scripts in a directory with a `SKILL.md` file and they are automatically discovered and registered as MCP tools:

```markdown
---
name: maya-geometry
description: "Geometry creation tools"
version: "1.0.0"
dcc: maya
tags: ["geometry"]
tools:
  - name: create_sphere
    description: "Create a sphere"
    source_file: scripts/create_sphere.py
---
```

### How do I discover and load skills?

```python
from dcc_mcp_core import SkillScanner, SkillCatalog
import os

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

scanner = SkillScanner()
catalog = SkillCatalog(scanner)

# Discover skills
catalog.discover(dcc_name="maya")

# Load a skill
ok = catalog.load_skill("maya-geometry")
print(ok)  # True
```

### What's the action naming convention for skill tools?

Actions from skills are named `{skill_name_underscored}__{tool_name}`, e.g.:
- skill `maya-geometry`, tool `create_sphere` → action `maya_geometry__create_sphere`

### How do I scan for skills without loading them?

```python
from dcc_mcp_core import scan_and_load_lenient

skills, skipped = scan_and_load_lenient(extra_paths=["/my/skills"])
for skill in skills:
    print(f"{skill.name} ({len(skill.tools)} tools)")
```

## Transport Layer

### What transport options are available?

- **TCP** — Network communication (`TransportAddress.tcp(host, port)`)
- **Named Pipes** — Low-latency local communication on Windows (`TransportAddress.named_pipe(name)`)
- **Unix Domain Sockets** — Low-latency local communication on Linux/macOS (`TransportAddress.unix_socket(path)`)

Use `TransportAddress.default_local(dcc_type, pid)` to automatically select the best IPC transport for the current platform.

### How do I register a DCC service and connect to it?

**DCC-side (server):**
```python
import os
from dcc_mcp_core import TransportManager, IpcListener, TransportAddress

mgr = TransportManager("/tmp/dcc-mcp")
instance_id, listener = mgr.bind_and_register("maya", version="2025")
channel = listener.accept()  # wait for agent to connect
```

**Agent-side (client):**
```python
from dcc_mcp_core import TransportManager, connect_ipc

mgr = TransportManager("/tmp/dcc-mcp")
entry = mgr.find_best_service("maya")
channel = connect_ipc(entry.effective_address())
rtt = channel.ping()
```

## MCP HTTP Server

### How do I expose DCC tools via HTTP for AI clients?

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
registry.register("get_scene_info", description="Get scene info", category="scene", dcc="maya")

server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())  # http://127.0.0.1:8765/mcp
# Connect your AI client to this URL
handle.shutdown()
```

## Troubleshooting

### My action registration is not working. What should I check?

1. Make sure the `ActionRegistry` instance used for registration is the same one used for lookup
2. Call `reg.list_actions()` to verify the action was registered
3. Use `reg.get_action("my_action")` to check the stored metadata
4. If using `ActionDispatcher`, verify `dispatcher.handler_count()` > 0

### How do I enable debug logging?

Set the `DCC_MCP_LOG` environment variable before importing:
```bash
export DCC_MCP_LOG=debug
```

Or configure via `TelemetryConfig`:
```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-service").with_stdout_exporter()
cfg.init()
```

### How do I report a bug or request a feature?

Please open an issue on [GitHub](https://github.com/loonghao/dcc-mcp-core/issues) with:
- DCC application and version
- Python version (`python --version`)
- dcc-mcp-core version (`python -c "import dcc_mcp_core; print(dcc_mcp_core.__version__)"`)
- Minimal reproduction code
- Expected vs actual behavior

## Contributing

### How do I contribute to the project?

See the [CONTRIBUTING.md](https://github.com/loonghao/dcc-mcp-core/blob/main/CONTRIBUTING.md) guide. Key steps:

1. Install Rust 1.85+ and Python 3.8+
2. Clone the repository
3. Run `vx just dev` to build and install in dev mode
4. Run `vx just test` to run the test suite

### Is there a community chat?

Join the discussion on [GitHub Discussions](https://github.com/loonghao/dcc-mcp-core/discussions).
