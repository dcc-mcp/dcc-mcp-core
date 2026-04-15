# dcc-mcp-core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[Chinese](README_zh.md) | [English](README.md)

**Production-grade foundation for AI-assisted DCC workflows** combining the **Model Context Protocol (MCP)** and a **zero-code Skills system**. Provides a **Rust-powered core with Python bindings (PyO3)** delivering enterprise-grade performance, security, and scalability — all with **zero runtime Python dependencies**. Supports Python 3.7–3.13.

> **Note**: This project is in active development (v0.12+). APIs may evolve; see CHANGELOG.md for version history.

---

## The Problem & Our Solution

### Why Not Just Use CLI?
**CLI tools are blind to DCC state.** They can't see the active scene, selected objects, or viewport context. They execute in isolation, forcing the AI to:
- Make multiple roundtrips to gather context
- Rebuild state from CLI outputs (fragile, slow)
- Lack visual feedback from the viewport
- Scale poorly with context explosion as requests grow

### Why MCP (Model Context Protocol)?
**MCP is AI-native**, but stock MCP lacks two critical capabilities for DCC automation:
1. **Context Explosion** — MCP has no mechanism to scope tools to specific sessions or instances, causing request bloat with multi-DCC setups
2. **No Lifecycle Control** — Can't discover instance state (active scene, documents, process health) or control startup/shutdown

### Our Approach: MCP + Skills System

We **reuse and extend** the existing MCP ecosystem, adding:

| Capability | Benefit |
|-----------|---------|
| **Gateway Election & Version Awareness** | Multi-instance load balancing; automatic handoff when newer DCC launches |
| **Session Isolation** | Each AI session talks to its own DCC instance; prevents context bleeding |
| **Skills System (Zero-Code)** | Define tools as `SKILL.md` + scripts/; no Python glue code needed |
| **Progressive Discovery** | Scope tools by DCC type, instance, scene, product; prevents context explosion |
| **Instance Tracking** | Know active documents, PIDs, display names; enable smart routing |
| **Structured Results** | Every tool returns `(success, message, context, next_steps)` for AI reasoning |

This isn't reinventing MCP — it's **solving MCP's blind spots for desktop automation**.

---

## Why dcc-mcp-core Over Alternatives?

| Aspect | dcc-mcp-core | Generic MCP | CLI Tools | Browser Extensions |
|--------|-------------|------------|-----------|-------------------|
| **DCC State Awareness** | Scenes, docs, instance IDs | No | No | Partial |
| **Multi-Instance Support** | Gateway election + session isolation | Single endpoint | No | No |
| **Context Scoping** | By DCC/scene/product | Global tools | No | Limited |
| **Zero-Code Tools** | SKILL.md + scripts | Full Python required | Scripts only | No |
| **Performance** | Rust + zero-copy + IPC | Python overhead | Process overhead | Network overhead |
| **Security** | Sandbox + audit log | Manual | Manual | None |
| **Cross-Platform** | Windows/macOS/Linux | Yes | Limited | Browser only |

AI-friendly docs: [AGENTS.md](AGENTS.md) | [CLAUDE.md](CLAUDE.md) | [GEMINI.md](GEMINI.md) | [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

## Architecture: The Three-Layer Stack

```
+-----------------------------------------------------------------+
|  AI Agent (Claude, GPT, etc.)                                    |
|  Calls tools via MCP protocol (tools/list, tools/call)           |
+-------------------------------+---------------------------------+
                                |
                        MCP Streamable HTTP
                                |
+-------------------------------v---------------------------------+
|  Gateway Server (Rust/HTTP)                                      |
|  +-- Version-Aware Instance Election                             |
|  +-- Session Isolation & Routing                                 |
|  +-- Tool Discovery (skills-derived)                             |
|  +-- Cancellation & Notifications (SSE)                          |
+-------------------------------+---------------------------------+
                                |
                 IPC (Named Pipe / Unix Socket / TCP)
                                |
          +---------------------+---------------------+
          |                     |                     |
  +-------v-------+   +-------v-------+   +-------v-------+
  |  Maya Bridge   |   | Blender Bridge |   | Houdini Bridge|
  |  Plugin (Rust) |   | Plugin (Rust)  |   | Plugin (Rust)|
  +-------+--------+   +-------+--------+   +-------+-------+
          |                     |                     |
    Python 3.7+           Python 3.7+           Python 3.7+
    (zero deps)           (zero deps)           (zero deps)
```

**Layer 1: AI Agent** — Calls tools via standard MCP protocol (tools/list, tools/call, notifications).

**Layer 2: Gateway** — Orchestrates discovery, session isolation, and request routing. Maintains `__gateway__` sentinel for version-aware election.

**Layer 3: DCC Adapters** — Python bridge plugins (Maya, Blender, Photoshop) with embedded Skills system. Each registers documents, scene state, and active process info.

---

## Quick Start

### Installation

```bash
# From PyPI (pre-built wheels for Python 3.7+)
pip install dcc-mcp-core

# Or from source (requires Rust 1.85+)
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### Basic Usage — Load & Execute Skills

```python
import json
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, SkillsManager,
    EventBus, success_result, scan_and_load
)

# 1. Discover skills (scans SKILL.md + scripts/)
skills, skipped = scan_and_load(dcc_name="maya")
print(f"Loaded {len(skills)} skills, skipped {len(skipped)}")

# 2. Register all skill tools in registry
registry = ActionRegistry()
for skill in skills:
    for script_path in skill.scripts:
        skill_name = f"{skill.name.replace('-', '_')}__{Path(script_path).stem}"
        registry.register(
            name=skill_name,
            description=skill.description,
            dcc=skill.dcc
        )

# 3. Set up dispatcher with event lifecycle
dispatcher = ActionDispatcher(registry)
bus = EventBus()
bus.subscribe("action.after_execute", lambda **kw: print(f"Done: {kw['action_name']}"))

# 4. Call a skill (returns structured result)
result = dispatcher.dispatch(
    "maya_geometry__create_sphere",
    json.dumps({"radius": 2.0})
)
print(f"Success: {result['output']['success']}")
print(f"Message: {result['output']['message']}")
print(f"Context: {result['output']['context']}")
```

## Core Concepts

### ActionResultModel — Structured Results for AI

All skill execution results use `ActionResultModel`, designed to be AI-friendly with structured context and next-step suggestions:

```python
from dcc_mcp_core import ActionResultModel, success_result, error_result

# Factory functions (recommended)
ok = success_result(
    "Sphere created",
    prompt="Consider adding materials or adjusting UVs",
    object_name="sphere1", position=[0, 1, 0]
)
# ok.context == {"object_name": "sphere1", "position": [0, 1, 0]}

err = error_result(
    "Failed to create sphere",
    "Radius must be positive"
)

# Direct construction
result = ActionResultModel(
    success=True,
    message="Operation completed",
    context={"key": "value"}
)

# Access fields
result.success      # bool
result.message     # str
result.prompt       # Optional[str] -- AI next-step suggestion
result.error        # Optional[str] -- error details
result.context      # dict -- arbitrary structured data
```

### ActionRegistry & Dispatcher — The Skill Execution System

```python
import json
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, ActionValidator,
    EventBus, SemVer, VersionedRegistry
)

# Registry with search support
registry = ActionRegistry()
registry.register("my_skill", description="My skill", category="tools", version="1.0.0")

# Validated dispatcher (takes only registry; validate separately with ActionValidator)
dispatcher = ActionDispatcher(registry)
dispatcher.register_handler("my_skill", lambda params: {"done": True})
result = dispatcher.dispatch("my_skill", json.dumps({}))
# result == {"action": "my_skill", "output": {"done": True}, "validation_skipped": True}

# Event-driven architecture
bus = EventBus()
sub_id = bus.subscribe("action.before_execute", lambda **kw: print(f"before: {kw}"))
bus.publish("action.before_execute", action_name="test")
bus.unsubscribe("action.before_execute", sub_id)
```

## Skills System — Zero-Code MCP Tool Registration

The **Skills system** is dcc-mcp-core's core innovation: it lets you register any script (Python, MEL, MaxScript, Batch, Shell, JS) as an MCP tool with **zero Python code**. Reuses the existing [OpenClaw Skills](https://docs.openclaw.ai/tools) format.

### How It Works

```
+-----------------------------------+
|  Directory:  my-automation/       |
|  +-- SKILL.md  (metadata)         |
|  +-- scripts/                     |
|      +-- cleanup.py               |
|      +-- publish.sh               |
|      +-- validate.mel             |
+-----------------------------------+
          |  (discovery)
+-----------------------------------+
|  SkillsManager (with caching)     |
|  +-- Dual-cache: by-paths &       |
|  |   by-config (session-bound)    |
|  +-- Scoped: Repo/User/System     |
|  +-- Policies: implicit invoke,   |
|      product filters, deps        |
+-----------------------------------+
          |  (registration)
+-----------------------------------+
|  ActionRegistry + SkillCatalog    |
|  (callable by AI via MCP tools)   |
+-----------------------------------+
```

### Five Minutes to Your First Skill

**1. Create `my-tool/SKILL.md`:**

```yaml
---
name: maya-cleanup
description: "Scene optimization and cleanup tools"
version: "1.0.0"
dcc: maya
scope: repo  # Trust level: repo < user < system < admin
tags: ["maintenance", "quality"]
policy:
  allow_implicit_invocation: false  # Require explicit load_skill call
  products: ["maya", "houdini"]      # Only show these DCCs
external_deps:
  mcp:
    - name: "usd-tools"
      description: "USD validation"
      transport: "ipc"
---
# Maya Scene Cleanup

Automated tools for optimizing and validating Maya scenes.
```

**2. Create `my-tool/scripts/cleanup.py`:**

```python
#!/usr/bin/env python
"""Clean unused nodes from the scene."""
import sys
result = {"success": True, "message": "Cleaned up 42 unused nodes"}
print(json.dumps(result))
sys.exit(0)
```

**3. Register and call:**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-tool"

from dcc_mcp_core import scan_and_load, ActionRegistry

skills = scan_and_load(dcc_name="maya")
registry = ActionRegistry()

# Tools are now: maya_cleanup__cleanup, with structured results
result = dispatcher.dispatch("maya_cleanup__cleanup", json.dumps({}))
print(result["output"])  # {"success": True, "message": "...", "context": {...}}
```

**That's it.** No Python glue code. Just metadata + scripts.

### Supported Script Types

| Extension | Type | Execution |
|-----------|------|-----------|
| `.py` | Python | `subprocess` with system Python |
| `.mel` | MEL (Maya) | Via DCC adapter |
| `.ms` | MaxScript | Via DCC adapter |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |

See `examples/skills/` for **11 complete examples**: hello-world, maya-geometry, maya-pipeline, git-automation, ffmpeg-media, imagemagick-tools, usd-tools, clawhub-compat, multi-script, dcc-diagnostics, workflow.

### Bundled Skills — Zero Configuration Required

`dcc-mcp-core` ships **two core skills** directly inside the wheel.
They are available immediately after `pip install dcc-mcp-core` — no repository
clone or `DCC_MCP_SKILL_PATHS` configuration needed.

| Skill | Tools | Purpose |
|-------|-------|---------|
| `dcc-diagnostics` | `screenshot`, `audit_log`, `action_metrics`, `process_status` | Observability & debugging for any DCC |
| `workflow` | `run_chain` | Multi-step action chaining with context propagation |

```python
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths

# Get the bundled skills directory (inside the installed wheel)
print(get_bundled_skills_dir())
# /path/to/site-packages/dcc_mcp_core/skills

# Returns [bundled_dir] or [] -- ready to extend your search path
paths = get_bundled_skill_paths()                    # default ON
paths = get_bundled_skill_paths(include_bundled=False)  # opt-out
```

DCC adapters (e.g. `dcc-mcp-maya`) automatically include bundled skills by
default. To opt-out: `start_server(include_bundled=False)`.

## Solving MCP Context Explosion

**The Problem:** Stock MCP returns ALL tools in `tools/list`, even those irrelevant to the user's current task or DCC instance. With 3 DCC instances x 50 skills x 5 scripts = **750 tools**, the context window fills instantly.

**Our Solution — Progressive Discovery:**

1. **Instance Awareness** — Each DCC registers active documents, PID, display name, scope level
2. **Smart Tool Scoping** — Tools filtered by:
   - **DCC Type** — Only Maya tools if working with Maya
   - **Scope** — Repo skills < User skills < System skills
   - **Product** — Some tools only for Houdini, not Maya
   - **Policy** — Implicit-invocation skills grouped separately
3. **Session Isolation** — AI session pinned to one DCC instance; sees only its tools
4. **Gateway Election** — If a newer DCC version launches, automatically hand off to it

**Example:**

Without dcc-mcp-core (stock MCP):
```
tools/list response includes:
- 100 Maya tools
- 100 Houdini tools
- 100 Blender tools
+ 250 shared tools
= 550 tool definitions in context
```

With dcc-mcp-core:
```
tools/list filtered by session instance:
AI talking to Maya instance #1 -> sees 100 Maya tools + 50 shared tools
AI talking to Houdini instance #2 -> sees 100 Houdini tools + 50 shared tools
= 150 tools in context (71% reduction)
```

This is **session-bound tool discovery** — not a hack, but a fundamental rethinking of how MCP scales.

---

## Architecture Overview — 14 Rust Crates

dcc-mcp-core is organized as a **Rust workspace of 14 crates**, compiled into a single native Python extension (`_core`) via PyO3/maturin:

| Crate | Responsibility | Key Types |
|----------------------|-----------|
| `dcc-mcp-models` | Data models | `ActionResultModel`, `SkillMetadata` |
| `dcc-mcp-actions` | Skill execution lifecycle | `ActionRegistry`, `EventBus`, `ActionDispatcher`, `ActionValidator`, `ActionPipeline` |
| `dcc-mcp-skills` | Skills discovery & loading | `SkillScanner`, `SkillCatalog`, `SkillWatcher`, dependency resolver |
| `dcc-mcp-protocols` | MCP protocol types | `ToolDefinition`, `ResourceDefinition`, `PromptDefinition`, `DccAdapter`, `BridgeKind` |
| `dcc-mcp-transport` | IPC communication | `TransportManager`, `ConnectionPool`, `IpcListener`, `FramedChannel`, `CircuitBreaker`, `FileRegistry` |
| `dcc-mcp-process` | Process management | `PyDccLauncher`, `ProcessMonitor`, `ProcessWatcher`, `CrashRecoveryPolicy` |
| `dcc-mcp-sandbox` | Security | `SandboxPolicy`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | Shared memory | `SharedBuffer`, `BufferPool`, LZ4 compression |
| `dcc-mcp-capture` | Screen capture | `Capturer`, cross-platform backends |
| `dcc-mcp-telemetry` | Observability | `TelemetryConfig`, `RecordingGuard`, tracing |
| `dcc-mcp-usd` | USD integration | `UsdStage`, `UsdPrim`, scene info bridge |
| `dcc-mcp-http` | MCP Streamable HTTP server | `McpHttpServer`, `McpHttpConfig`, `ServerHandle`, Gateway (first-wins competition) |
| `dcc-mcp-server` | Binary entry point | `dcc-mcp-server` CLI, gateway runner |
| `dcc-mcp-utils` | Infrastructure | Filesystem helpers, type wrappers, constants, JSON |

## Key Features

- **Rust-powered performance**: Zero-copy serialization (rmp-serde), LZ4 shared memory, lock-free data structures
- **Zero runtime Python deps**: Everything compiled into native extension
- **Skills system**: Zero-code MCP tool registration via SKILL.md + scripts/
- **Validated dispatch**: Input validation pipeline before execution
- **Resilient IPC**: Connection pooling, circuit breaker, automatic retry
- **Process management**: Launch, monitor, auto-recover DCC processes
- **Sandbox security**: Policy-based access control with audit logging
- **Screen capture**: Cross-platform DCC viewport capture for AI visual feedback
- **USD integration**: Universal Scene Description read/write bridge
- **Structured telemetry**: Tracing & recording for observability
- **~140 public Python symbols** with full type stubs (`.pyi`)
- **OpenClaw Skills compatible**: Reuse existing ecosystem format

## Installation

```bash
# From PyPI (pre-built wheels)
pip install dcc-mcp-core

# Or from source (requires Rust 1.85+)
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

## Development Setup

```bash
# Clone the repository
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Recommended: use vx (universal dev tool manager)
# Install vx: https://github.com/loonghao/vx
vx just install     # Install all project dependencies
vx just dev         # Build + install dev wheel
vx just test       # Run Python tests
vx just lint       # Full lint check (Rust + Python)
```

### Without vx

```bash
# Manual setup
python -m venv venv
source venv/bin/activate   # Windows: venv\Scripts\activate
pip install maturin pytest pytest-cov ruff mypy
maturin develop --features python-bindings,ext-module
pytest tests/ -v
ruff check python/ tests/ examples/
cargo clippy --workspace -- -D warnings
```

## Running Tests

```bash
vx just test           # All Python tests
vx just test-rust       # All Rust unit tests
vx just test-cov        # With coverage report
vx just ci              # Full CI pipeline
vx just preflight       # Pre-commit checks only
```

### Transport Layer — Inter-Process Communication

dcc-mcp-core provides a production-ready IPC transport layer:

```python
from dcc_mcp_core import (
    TransportManager, TransportAddress, TransportScheme,
    RoutingStrategy, IpcListener, connect_ipc,
    FramedChannel
)

# Server side: listen for connections
listener = IpcListener.new("/tmp/dcc-mcp-server.sock")
handle = listener.start(handler_fn=my_message_handler)

# Client side: connect to server
channel = connect_ipc("/tmp/dcc-mcp-server.sock")
response = channel.call({"method": "ping", "params": {}})

# Advanced: connection pooling with resilience
mgr = TransportManager()
mgr.configure_pool(min_size=2, max_size=10)
mgr.set_circuit_breaker(threshold=5, reset_timeout=30)
```

### Process Management — DCC Lifecycle Control

```python
from dcc_mcp_core import (
    PyDccLauncher, PyProcessMonitor, PyProcessWatcher,
    PyCrashRecoveryPolicy
)

# Launch a DCC application
launcher = PyDccLauncher(dcc_type="maya", version="2025")
process = launcher.launch(
    script_path="/path/to/startup.py",
    working_dir="/project",
    env_vars={"MAYA_RENDER_THREADS": "4"}
)

# Monitor health
monitor = PyProcessMonitor()
monitor.track(process)
stats = monitor.stats(process)  # CPU, memory, uptime

# Auto-restart on crash
watcher = PyProcessWatcher(
    recovery_policy=PyCrashRecoveryPolicy(max_restarts=3, cooldown_sec=10)
)
watcher.watch(process)
```

### Sandbox Security — Policy-Based Access Control

```python
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator, AuditLog

# Define what's allowed
policy = (
    SandboxPolicy.builder()
    .allow_read(["/safe/paths/*"])
    .allow_write(["/temp/*"])
    .deny_pattern(["*.critical"])
    .require_approval_for("delete_*")
    .build()
)

ctx = SandboxContext(policy=policy)
validator = InputValidator(ctx)

# Validate before execution
if not validator.validate_action("delete_all_files"):
    print("Blocked by policy!")
else:
    print("Allowed -- executing...")

# Review audit trail
audit = AuditLog.load()
for entry in audit.entries:
    print(f"{entry.timestamp} [{entry.action}] {entry.decision} -> {entry.details}")
```

## More Examples

See the [`examples/skills/`](examples/skills/) directory for **11 complete skill packages**, and the [VitePress docs site](https://loonghao.github.io/dcc-mcp-core/) for comprehensive guides per module.

## Release Process

This project uses [Release Please](https://github.com/googleapis/release-please) to automate versioning and releases. The workflow is:

1. **Develop**: Create a branch from `main`, make changes using [Conventional Commits](https://www.conventionalcommits.org/)
2. **Merge**: Open a PR and merge to `main`
3. **Release PR**: Release Please automatically creates/updates a release PR that bumps the version and updates `CHANGELOG.md`
4. **Publish**: When the release PR is merged, a GitHub Release is created and the package is published to PyPI

### Commit Message Format

This project follows [Conventional Commits](https://www.conventionalcommits.org):

| Prefix | Description | Version Bump |
|--------|-------------|--------------|
| `feat:` | New feature | Minor (`0.x.0`) |
| `fix:` | Bug fix | Patch (`0.0.x`) |
| `feat!:` or `BREAKING CHANGE:` | Breaking change | Major (`x.0.0`) |
| `docs:` | Documentation only | No release |
| `chore:` | Maintenance | No release |
| `ci:` | CI/CD changes | No release |
| `refactor:` | Code refactoring | No release |
| `test:` | Adding tests | No release |

### Examples

```bash
# Feature (bumps minor version)
git commit -m "feat: add batch skill execution support"

# Bug fix (bumps patch version)
git commit -m "fix: resolve middleware chain ordering issue"

# Breaking change (bumps major version)
git commit -m "feat!: redesign skill registry API"

# Scoped commit
git commit -m "feat(skills): add PowerShell script support"

# No release trigger
git commit -m "docs: update API reference"
git commit -m "ci: add Python 3.14 to test matrix"
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

### Development Workflow

1. Fork the repository and clone your fork
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make your changes following the coding standards below
4. Run tests and linting:
   ```bash
   vx just lint       # Check code style
   vx just test       # Run tests
   vx just prek-all   # Run all pre-commit hooks
   ```
5. Commit using [Conventional Commits](https://www.conventionalcommits.org/) format
6. Push and open a Pull Request against `main`

### Coding Standards

- **Style**: Code is formatted with `ruff` and `isort` (line length: 120)
- **Type hints**: All public APIs must have type annotations
- **Docstrings**: Google-style docstrings for all public modules, classes, and functions
- **Testing**: New features must include tests; maintain or improve coverage
- **Imports**: Use section headers (`Import built-in modules`, `Import third-party modules`, `Import local modules`)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## AI Agent Resources

If you're an AI coding agent, also see:
- **[AGENTS.md](AGENTS.md)** — Comprehensive guide for all AI agents (architecture, commands, API reference, pitfalls)
- **[CLAUDE.md](CLAUDE.md)** — Claude-specific instructions and workflows
- **[GEMINI.md](GEMINI.md)** — Gemini-specific instructions and workflows
- **[.agents/skills/dcc-mcp-core/SKILL.md](.agents/skills/dcc-mcp-core/SKILL.md)** — Complete API skill definition for learning and using this library
- **[python/dcc_mcp_core/__init__.py](python/dcc_mcp_core/__init__.py)** — Full public API surface (~140 symbols)
- **[llms.txt](llms.txt)** — Concise API reference optimized for LLMs
- **[llms-full.txt](llms-full.txt)** — Complete API reference optimized for LLMs
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — Development workflow and coding standards
