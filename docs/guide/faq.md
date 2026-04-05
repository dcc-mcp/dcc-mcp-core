# FAQ

Frequently asked questions about DCC-MCP-Core.

## General

### What is DCC-MCP-Core?

DCC-MCP-Core is a foundational Rust library with Python bindings that provides:
- **Action Registry**: A centralized system for registering and executing actions in DCC applications (Maya, Blender, Houdini, 3ds Max, etc.)
- **Event Bus**: A publish-subscribe event system for hook into DCC lifecycle
- **MCP Protocol Types**: Type definitions for the Model Context Protocol used by AI coding assistants
- **Transport Layer**: IPC and network communication for distributed DCC integration

### What DCC applications are supported?

Currently supported DCC integrations:
- **Maya**: Full action and event support
- **Blender**: Full action and event support
- **Houdini**: Full action and event support
- **3ds Max**: Full action and event support
- **Unreal Engine**: Transport layer support
- **Generic Python**: Works with any Python 3.8+ environment

### What Python versions are supported?

Python 3.8, 3.9, 3.10, 3.11, 3.12, and 3.13 are fully supported and tested in CI.

## Installation

### How do I install dcc-mcp-core?

**From PyPI:**
```bash
pip install dcc-mcp-core
```

**From source:**
```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### What are the dependencies?

The core library has **zero third-party dependencies**. All dependencies are optional:
- `pyo3 >= 0.23` for Python bindings
- `pytest`, `pytest-cov`, `pytest-mock`, `pyfakefs` for testing

### How do I install with a specific DCC integration?

```bash
# Maya
pip install dcc-mcp-core[maya]

# Blender
pip install dcc-mcp-core[blender]

# All DCCs
pip install dcc-mcp-core[all]
```

## Actions

### How do I register a custom action?

```python
from dcc_mcp_core import ActionRegistry, action

# Using decorator
registry = ActionRegistry()

@registry.action("my_custom_action")
def my_action(x: int, y: int) -> dict:
    """Add two numbers and return the result."""
    return {"result": x + y}

# Or register manually
def another_action(name: str) -> dict:
    return {"greeting": f"Hello, {name}!"}

registry.register("another_action", another_action)
```

### How do I execute an action?

```python
from dcc_mcp_core import ActionRegistry

registry = ActionRegistry()
result = registry.call("my_action", x=10, y=20)

print(result.success)    # True
print(result.message)    # "Action completed successfully"
print(result.context)    # {"result": 30}
```

### How do I validate action input?

```python
from dcc_mcp_core import action

@action(validator=lambda params: params.get("x", 0) > 0)
def positive_only(x: int):
    """Action that only accepts positive numbers."""
    return {"x": x}
```

## Events

### How does the event system work?

The EventBus provides a publish-subscribe pattern:

```python
from dcc_mcp_core import EventBus

bus = EventBus()

# Subscribe to an event
def on_save(file_path: str):
    print(f"Saving to: {file_path}")

bus.subscribe("dcc.save", on_save)

# Publish an event
bus.publish("dcc.save", file_path="/tmp/scene.usd")
```

### What events are available?

Standard DCC lifecycle events:
- `dcc.startup` - DCC application started
- `dcc.shutdown` - DCC application closing
- `dcc.save` - Before saving
- `dcc.save.complete` - After saving
- `dcc.open` - Before opening a file
- `dcc.open.complete` - After opening a file
- `dcc.undo` - Before undo operation
- `dcc.redo` - After redo operation

### Can I use async event handlers?

Yes, the EventBus supports async handlers:

```python
import asyncio
from dcc_mcp_core import EventBus

bus = EventBus()

@bus.on("network.request")
async def handle_request(endpoint: str):
    # Async operations
    await asyncio.sleep(0.1)
    return {"status": "ok"}
```

## Skills

### What is the Skills system?

The Skills system allows zero-code script registration through markdown files with YAML frontmatter:

```markdown
---
name: my-skill
version: 1.0.0
description: A useful skill
---

# My Skill

This skill does something useful.
```

### How do I scan for skills?

```python
from dcc_mcp_core.skills import SkillScanner

scanner = SkillScanner()
skills = scanner.scan(["/path/to/skills", "/another/path"])

for skill in skills:
    print(f"{skill.name} v{skill.version}: {skill.description}")
```

## Transport Layer

### What transport options are available?

- **IPC (Inter-Process Communication)**: Fast local communication via Unix sockets or named pipes
- **TCP**: Network-based communication for distributed systems
- **WebSocket**: Browser-based connections
- **HTTP**: REST-style communication

### How do I create a transport pool?

```python
from dcc_mcp_core.transport import TransportPool, TransportConfig

config = TransportConfig(
    max_connections=10,
    timeout=30.0,
)

pool = TransportPool(config)
```

## Troubleshooting

### My action registration is not working. What should I check?

1. Ensure the action function has a docstring
2. Check that parameter names match between registration and calls
3. Verify the ActionRegistry instance is the same used for both registration and calls
4. Enable debug logging to see registration messages

### How do I enable debug logging?

```python
import logging
logging.basicConfig(level=logging.DEBUG)

from dcc_mcp_core import ActionRegistry
# Now all ActionRegistry operations will print debug info
```

### How do I report a bug or request a feature?

Please open an issue on [GitHub](https://github.com/loonghao/dcc-mcp-core/issues) with:
- DCC application and version
- Python version
- Minimal reproduction code
- Expected vs actual behavior

## Contributing

### How do I contribute to the project?

See the [CONTRIBUTING.md](https://github.com/loonghao/dcc-mcp-core/blob/main/CONTRIBUTING.md) guide for:
1. Development environment setup
2. Coding standards
3. Testing requirements
4. Pull request process

### Is there a community chat?

Join the discussion on [GitHub Discussions](https://github.com/loonghao/dcc-mcp-core/discussions).
