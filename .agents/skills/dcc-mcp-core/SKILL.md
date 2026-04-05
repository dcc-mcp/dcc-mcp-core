---
name: dcc-mcp-core
description: "Foundation library for the DCC Model Context Protocol (MCP) ecosystem. Provides Rust-powered action management, skills system, transport layer, sandbox security, shared memory, screen capture, USD scene support, and telemetry for AI-assisted DCC workflows."
tools: ["Bash", "Read", "Write", "Edit"]
tags: ["mcp", "dcc", "rust", "pyo3", "maya", "blender", "houdini", "ai", "skills", "actions"]
version: "0.12.4"
---

# dcc-mcp-core — DCC MCP Ecosystem Foundation

The foundational library enabling AI assistants to interact with Digital Content Creation (DCC) software through the Model Context Protocol (MCP).

## What This Library Does

dcc-mcp-core solves the core infrastructure challenges of building AI-DCC integrations:

| Capability | Description |
|------------|-------------|
| **Action Management** | Register, validate, dispatch, and execute actions with typed inputs/outputs |
| **Skills System** | Zero-code script registration (Python/MEL/Batch/Shell/JS) as MCP tools via `SKILL.md` |
| **Transport Layer** | High-performance IPC with connection pooling, circuit breaker, retry policies |
| **Process Management** | Launch, monitor, auto-recover DCC processes (Maya, Blender, Houdini, etc.) |
| **Sandbox Security** | Policy-based access control, input validation, audit logging |
| **Shared Memory** | LZ4-compressed inter-process data exchange for large scenes |
| **Screen Capture** | Cross-platform DCC viewport capture for visual feedback |
| **USD Support** | Read/write Universal Scene Description for pipeline integration |
| **Telemetry** | Structured tracing and recording for observability |
| **MCP Protocol Types** | Complete Tool/Resource/Prompt schema implementations |

## Quick Start for AI Agents

### Installation

```bash
pip install dcc-mcp-core
```

### Basic Usage Pattern

```python
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, ActionValidator,
    EventBus, ActionResultModel, success_result, error_result,
    SkillScanner, scan_and_load, parse_skill_md,
    ToolDefinition, TransportManager,
)

# 1. Create registry and discover actions
registry = ActionRegistry()

# 2. Load skills from environment paths
skills = scan_and_load(dcc_name="maya")
for skill_meta in skills:
    print(f"Loaded skill: {skill_meta.name}")

# 3. Dispatch validated actions
dispatcher = ActionDispatcher(registry)
result = dispatcher.call("maya_geometry__create_sphere", radius=2.0)

if result.success:
    print(f"Created: {result.context.get('object_name')}")
else:
    print(f"Error: {result.error}")
```

### Creating a Custom Skill (Zero Python Code)

```bash
# 1. Create directory structure
mkdir -p my-tool/scripts/

# 2. Write SKILL.md
cat > my-tool/SKILL.md << 'EOF'
---
name: my-tool
description: "My custom DCC automation tools"
tools: ["Bash"]
tags: ["automation", "custom"]
dcc: maya
---

# My Tool

Automation scripts for Maya workflow optimization.
EOF

# 3. Add scripts/
cat > my-tool/scripts/list_selected.py << 'PYEOF'
#!/usr/bin/env python3
"""List selected objects in the Maya scene."""
import json
import sys

# Simulated output — in real usage, context provides cmds
result = {
    "selected": ["pSphere1", "pCube1"],
    "count": 2
}
print(json.dumps(result))
PYEOF

# 4. Set environment and use
export DCC_MCP_SKILL_PATHS="$(pwd)/my-tool"
python -c "
from dcc_mcp_core import scan_and_load, ActionRegistry
registry = ActionRegistry()
skills = scan_and_load(dcc_name='maya')
print(f'Discovered {len(skills)} skills')
"
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                   Python Layer                       │
│  dcc_mcp_core/__init__.py  →  _core (PyO3 cdyll)   │
│  ~120 public symbols re-exported from Rust core      │
└──────────────────────┬──────────────────────────────┘
                       │ PyO3 bindings
┌──────────────────────▼──────────────────────────────┐
│                   Rust Core (11 Crates)               │
│                                                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │ models   │←─│ actions  │←─│ skills   │          │
│  │(data)    │  │(pipeline)│  │(scanner) │          │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘          │
│       │              │              │               │
│  ┌────▼─────┐  ┌────▼─────┐  ┌────▼─────┐         │
│  │protocols │  │ transport│  │ process  │         │
│  │(MCP)     │  │(IPC/pool)│  │(launcher)│         │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘         │
│       │              │              │               │
│  ┌────▼─────┐  ┌────▼─────┐  ┌────▼─────┐         │
│  │ sandbox  │  │   shm    │  │ telemetry│         │
│  │(security)│  │(shared-m)│  │(tracing) │         │
│  └────┬─────┘  └────┬─────┘  └──────────┘         │
│       │              │                                │
│  ┌────▼─────┐  ┌────▼──────────────▼─────┐        │
│  │ capture  │  │         usd             │        │
│  │(screen)  │  │  (scene description)    │        │
│  └──────────┘  └─────────────────────────┘        │
│                                                       │
│  utils (filesystem, wrappers, constants, JSON)      │
└─────────────────────────────────────────────────────┘
```

## Key API Domains

### Actions & Dispatch

```python
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, ActionValidator,
    EventBus, SemVer, VersionedRegistry
)

registry = ActionRegistry()
validator = ActionValidator()
dispatcher = ActionDispatcher(registry, validator)

# Subscribe to events
bus = EventBus()
bus.subscribe("action.before_execute", lambda e: print(f"Running: {e.action_name}"))
bus.subscribe("action.after_execute", lambda e: print(f"Result: {e.result.success}"))

# Call with validation
result = dispatcher.call("action_name", param1="value")
```

### Skills Discovery

```python
from dcc_mcp_core import (
    SkillScanner, SkillWatcher, SkillMetadata,
    parse_skill_md, scan_skill_paths, scan_and_load,
    resolve_dependencies, get_skill_paths_from_env
)

# Option 1: Scan and load everything from env
all_skills = scan_and_load(dcc_name="maya")

# Option 2: Manual scan with custom paths
scanner = SkillScanner()
found = scanner.scan(extra_paths=["./my-skills"], dcc_name="blender")

# Option 3: Parse individual SKILL.md
meta = parse_skill_md("path/to/SKILL.md")
# Returns SkillMetadata(name, description, tags, scripts, ...)

# Option 4: Watch for changes (auto-reload)
watcher = SkillWatcher(callback=lambda skill: print(f"Reloaded: {skill}"))
watcher.watch(["./skills-dir"])
```

### Transport (IPC Communication)

```python
from dcc_mcp_core import (
    TransportManager, TransportAddress, TransportScheme,
    RoutingStrategy, IpcListener, connect_ipc, FramedChannel
)

# Start listening
listener = IpcListener.new("/tmp/dcc-mcp.sock")
handle = listener.start(lambda msg: print(f"Got: {msg}"))

# Connect as client
channel = connect_ipc("/tmp/dcc-mcp.sock")
response = channel.call({"method": "ping"})
```

### Process Management

```python
from dcc_mcp_core import (
    PyDccLauncher, PyProcessMonitor, PyProcessWatcher,
    PyCrashRecoveryPolicy, ScriptResult, ScriptLanguage
)

# Launch a DCC process
launcher = PyDccLauncher(dcc_type="maya")
process = launcher.launch(
    script_path="/path/to/startup.py",
    working_dir="/project/root"
)

# Monitor it
monitor = PyProcessMonitor()
monitor.track(process)

# Auto-restart on crash
watcher = PyProcessWatcher(recovery_policy=PyCrashRecoveryPolicy(max_restarts=3))
watcher.watch(process)
```

### Sandbox (Security)

```python
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator, AuditEntry

policy = SandboxPolicy.default()  # Allow safe operations only
context = SandboxContext(policy=policy)

# Validate before execution
validator = InputValidator(context)
if validator.validate_action("delete_all_files"):
    # Blocked by policy!
    print("Action denied")
```

### Result Models

```python
from dcc_mcp_core import ActionResultModel, success_result, error_result, validate_action_result

# Factory functions (preferred)
ok = success_result(message="Done", context={"key": "value"}, prompt="Try X next")
err = error_result(message="Failed", error="Reason here")

# Direct construction
result = ActionResultModel(
    success=True,
    message="Object created",
    prompt="Consider adding materials next",
    context={"object_name": "sphere1", "position": [0, 1, 0]}
)
```

## Testing Your Integration

```bash
# After installing dcc-mcp-core, run the test suite
pip install pytest pytest-cov
pytest tests/ -v

# Test skills discovery specifically
pytest tests/test_skills.py tests/test_skills_e2e.py -v

# Test transport
pytest tests/test_transport.py -v
```

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_SKILL_PATHS` | Empty | Colon-separated paths to scan for `SKILL.md` dirs |
| `DCC_MCP_LOG_LEVEL` | `INFO` | Log level (`TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`) |

## Supported DCC Software

- **Autodesk Maya** — MEL/Python scripting
- **Blender** — Python API
- **SideFX Houdini** — HScript/Python
- **Cinema 4D** — Python/CoffeeScript
- **Any DCC with scripting support** — via generic adapter

## Learning Resources

- **Full docs site**: https://loonghao.github.io/dcc-mcp-core/
- **Examples**: See `examples/skills/` for 9 complete skill packages
- **Type stubs**: `python/dcc_mcp_core/_core.pyi` (complete API signature reference)
- **CHANGELOG**: `CHANGELOG.md` for version history
- **Contributing**: `CONTRIBUTING.md` for development workflow
