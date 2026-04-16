# Getting Started

## Installation

### From PyPI

```bash
pip install dcc-mcp-core
```

### From Source (requires Rust toolchain)

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

::: tip
Building from source requires the Rust toolchain. Install it from [rustup.rs](https://rustup.rs/).
The build is handled by [maturin](https://www.maturin.rs/) which compiles the Rust core and installs the Python package.
:::

## Requirements

- **Python**: >= 3.7 (CI tests 3.7, 3.8, 3.9, 3.10, 3.11, 3.12, 3.13)
- **Rust**: >= 1.85 (for building from source)
- **License**: MIT
- **Python Dependencies**: Zero — everything is in the compiled Rust extension

## Quick Start

### Skills-First: `create_skill_server` (recommended since v0.12.12)

The fastest way to expose scripts as MCP tools. Create a `SKILL.md` in your script folder, then use `create_skill_server` to wire everything in one call:

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

# Point to your skill directories (per-app env var)
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my-skills"

# One call: discover skills + start MCP HTTP server
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server at {handle.mcp_url()}")
# AI clients (Claude Desktop, etc.) connect to http://127.0.0.1:8765/mcp
```

Or use `SkillCatalog` directly for more control:

```python
import os
from dcc_mcp_core import SkillCatalog, ToolRegistry

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-skills"

registry = ToolRegistry()
catalog = SkillCatalog(registry)

discovered = catalog.discover(dcc_name="maya")
print(f"Discovered {discovered} skills")

# Load a skill and inspect the registered tool names
actions = catalog.load_skill("maya-geometry")
print(actions)
```

See the [Skills System guide](/guide/skills) for writing `SKILL.md` files and advanced options.

### Tool Registry

```python
from dcc_mcp_core import ToolRegistry

registry = ToolRegistry()
registry.register(
    name="create_sphere",
    description="Creates a sphere in the scene",
    category="geometry",
    tags=["geometry", "creation"],
    dcc="maya",
)

tool = registry.get_action("create_sphere")
print(tool)  # dict with tool metadata

maya_tools = registry.list_actions(dcc_name="maya")
```

### Action Results

```python
from dcc_mcp_core import success_result, error_result

result = success_result("Created 5 spheres", prompt="Use modify next", count=5)
print(result.success)  # True
print(result.message)  # "Created 5 spheres"
print(result.context)  # {"count": 5}

err = error_result("Failed", "File not found", prompt="Check path")
print(err.success)  # False
```

### Event Bus

```python
from dcc_mcp_core import EventBus

bus = EventBus()
sid = bus.subscribe("scene.changed", lambda: print("Scene updated!"))
bus.publish("scene.changed")
bus.unsubscribe("scene.changed", sid)
```

### MCP HTTP Server

Expose your registry to AI clients (Claude Desktop, etc.) over HTTP in one call:

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
# ... register tools or load skills ...

config = McpHttpConfig(port=8765)
server = McpHttpServer(registry, config)
handle = server.start()

print(f"MCP server running at {handle.mcp_url()}")
# handle.shutdown() to shut down
```

## Development Setup

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Install with vx (recommended)
vx just install

# Or manual setup
pip install maturin
maturin develop
```

## Running Tests

```bash
vx just test
vx just lint
```

## Next Steps

- Learn about [Actions & Registry](/guide/actions) — the tool registration layer
- Explore the [Event System](/guide/events) for lifecycle hooks
- Check out the [Skills System](/guide/skills) for zero-code script registration
- Expose tools with [MCP HTTP Server](/api/http)
- See the [Transport Layer](/guide/transport) for DCC communication
- Understand the [Architecture](/guide/architecture) of the 14-crate Rust workspace
