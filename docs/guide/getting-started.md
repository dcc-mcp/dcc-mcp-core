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

- **Python**: >= 3.11 (CI tests 3.11, 3.12, 3.13)
- **Rust**: >= 1.85 (for building from source)
- **License**: MIT
- **Python Dependencies**: Zero — everything is in the compiled Rust extension

## Quick Start

### Action Registry

```python
from dcc_mcp_core import ActionRegistry

registry = ActionRegistry()
registry.register(
    name="create_sphere",
    description="Creates a sphere in the scene",
    category="geometry",
    tags=["geometry", "creation"],
    dcc="maya",
)

action = registry.get_action("create_sphere")
print(action)  # dict with action metadata

maya_actions = registry.list_actions(dcc_name="maya")
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

- Learn about [Actions & Registry](/guide/actions) — the core building block
- Explore the [Event System](/guide/events) for lifecycle hooks
- Check out the [Skills System](/guide/skills) for zero-code script registration
- See the [Transport Layer](/guide/transport) for DCC communication
