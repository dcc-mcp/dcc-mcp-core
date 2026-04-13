# dcc-mcp-core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[中文文档](README_zh.md) | [English](README.md)

Foundational library for the DCC Model Context Protocol (MCP) ecosystem. It provides a **Rust-powered core with Python bindings (PyO3)** that delivers high-performance skill management, skills discovery, transport, sandbox security, shared memory, screen capture, USD support, and telemetry — all with **zero runtime Python dependencies**. Supports Python 3.7–3.13.

> **Note**: This project is in active development (v0.12+). APIs may evolve; see CHANGELOG.md for version history.

## Why dcc-mcp-core?

| Feature | Description |
|---------|-------------|
| **Performance** | Rust core with zero-copy serialization via rmp-serde & LZ4 compression |
| **Type Safety** | Full PyO3 bindings with comprehensive `.pyi` type stubs (~120 public symbols) |
| **Skills System** | Zero-code script registration as MCP tools (SKILL.md + scripts/) |
| **Resilient Transport** | IPC with connection pooling, circuit breaker, retry policies |
| **Process Management** | Launch, monitor, auto-recover DCC processes |
| **Sandbox Security** | Policy-based access control with audit logging |
| **Cross-Platform** | Windows, macOS, Linux — tested on all three |

AI-friendly docs: [AGENTS.md](AGENTS.md) | [CLAUDE.md](CLAUDE.md) | [GEMINI.md](GEMINI.md) | [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

## Quick Start

### Installation

```bash
# From PyPI (pre-built wheels for Python 3.7+)
pip install dcc-mcp-core

# Or from source (requires Rust toolchain)
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### Basic Usage

```python
import json
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher,
    EventBus, success_result, scan_and_load
)

# 1. Load skills; scan_and_load returns a 2-tuple (skills, skipped_dirs)
skills, skipped = scan_and_load(dcc_name="maya")
print(f"Loaded {len(skills)} skills")

# 2. Register skills from discovered skill packages
registry = ActionRegistry()
from pathlib import Path
for skill in skills:
    for script_path in skill.scripts:
        stem = Path(script_path).stem
        skill_name = f"{skill.name.replace('-', '_')}__{stem}"
        registry.register(name=skill_name, description=skill.description, dcc=skill.dcc)

# 3. Set up dispatcher and register a handler
dispatcher = ActionDispatcher(registry)
dispatcher.register_handler(
    "maya_geometry__create_sphere",
    lambda params: {"object_name": "pSphere1", "radius": params.get("radius", 1.0)},
)

# 4. Subscribe to lifecycle events
bus = EventBus()
bus.subscribe("action.after_execute", lambda **kw: print(f"event: {kw}"))

# 5. Dispatch a skill
result = dispatcher.dispatch(
    "maya_geometry__create_sphere",
    json.dumps({"radius": 2.0}),
)
output = result["output"]
print(f"Created: {output.get('object_name')}")
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
result.prompt       # Optional[str] — AI next-step suggestion
result.error        # Optional[str] — error details
result.context      # dict — arbitrary structured data
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

The **Skills system** is dcc-mcp-core's most unique feature: it lets you register any script (Python, MEL, MaxScript, Batch, Shell, JS) as an MCP-discoverable tool with **zero Python code**. It reuses the [OpenClaw Skills](https://docs.openclaw.ai/tools) ecosystem format.

### How It Works

```
SKILL.md (metadata) + scripts/ directory
       ↓  SkillScanner discovers & parses
SkillMetadata per skill (name, description, tags, script list)
       ↓  Skills registered in ActionRegistry → callable by AI via MCP
```

### Quick Example

**1. Create a Skill directory:**

```
my-tool/
├── SKILL.md          # Metadata + description
└── scripts/
    └── list.py      # Your script
```

**2. Write `SKILL.md`:**

```yaml
---
name: my-tool
description: "My custom DCC automation tools"
allowed-tools: ["Bash"]
tags: ["automation", "custom"]
dcc: maya
version: "1.0.0"
---
# My Tool

Automation scripts for Maya workflow optimization.
```

**3. Add `scripts/list.py`**

**4. Set environment and use:**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-tool"

from dcc_mcp_core import scan_and_load, ActionRegistry

registry = ActionRegistry()
skills = scan_and_load(dcc_name="maya")
for s in skills:
    print(f"✓ {s.name}: {len(s.scripts)} scripts")

# Call a skill: {skill_name}__{script_name}
result = registry.call("my_tool__list", some_param="value")
```

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

# Returns [bundled_dir] or [] — ready to extend your search path
paths = get_bundled_skill_paths()                    # default ON
paths = get_bundled_skill_paths(include_bundled=False)  # opt-out
```

DCC adapters (e.g. `dcc-mcp-maya`) automatically include bundled skills by
default. To opt-out: `start_server(include_bundled=False)`.

## Architecture Overview

dcc-mcp-core is organized as a **Rust workspace of 11 crates**, compiled into a single native Python extension (`_core`) via PyO3/maturin:

| Crate | Responsibility | Key Types |
|----------------------|-----------|
| `dcc-mcp-models` | Data models | `ActionResultModel`, `SkillMetadata` |
| `dcc-mcp-actions` | Skill execution lifecycle | `ActionRegistry`, `EventBus`, `ActionDispatcher`, `ActionValidator`, `ActionPipeline` |
| `dcc-mcp-skills` | Skills discovery | `SkillScanner`, `SkillLoader`, `SkillWatcher`, dependency resolver |
| `dcc-mcp-protocols` | MCP protocol types | `ToolDefinition`, `ResourceDefinition`, `PromptDefinition`, `DccAdapter` types |
| `dcc-mcp-transport` | IPC communication | `TransportManager`, `ConnectionPool`, `IpcListener`, `FramedChannel`, `CircuitBreaker` |
| `dcc-mcp-process` | Process management | `PyDccLauncher`, `ProcessMonitor`, `ProcessWatcher`, `CrashRecoveryPolicy` |
| `dcc-mcp-sandbox` | Security | `SandboxPolicy`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | Shared memory | `SharedBuffer`, `BufferPool`, LZ4 compression |
| `dcc-mcp-capture` | Screen capture | `Capturer`, cross-platform backends |
| `dcc-mcp-telemetry` | Observability | `TelemetryConfig`, `RecordingGuard`, tracing |
| `dcc-mcp-usd` | USD integration | `UsdStage`, `UsdPrim`, scene info bridge |
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
- **~120 public Python symbols** with full type stubs (`.pyi`)
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
    print("Allowed — executing...")

# Review audit trail
audit = AuditLog.load()
for entry in audit.entries:
    print(f"{entry.timestamp} [{entry.action}] {entry.decision} → {entry.details}")
```

## More Examples

See the [`examples/skills/`](examples/skills/) directory for **9 complete skill packages**, and the [VitePress docs site](https://loonghao.github.io/dcc-mcp-core/) for comprehensive guides per module.

## Release Process

This project uses [Release Please](https://github.com/googleapis/release-please) to automate versioning and releases. The workflow is:

1. **Develop**: Create a branch from `main`, make changes using [Conventional Commits](https://www.conventionalcommits.org/)
2. **Merge**: Open a PR and merge to `main`
3. **Release PR**: Release Please automatically creates/updates a release PR that bumps the version and updates `CHANGELOG.md`
4. **Publish**: When the release PR is merged, a GitHub Release is created and the package is published to PyPI

### Commit Message Format

This project follows [Conventional Commits](https://www.conventionalcommits.org/):

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
- **[python/dcc_mcp_core/__init__.py](python/dcc_mcp_core/__init__.py)** — Full public API surface (~120 symbols)
- **[llms.txt](llms.txt)** — Concise API reference optimized for LLMs
- **[llms-full.txt](llms-full.txt)** — Complete API reference optimized for LLMs
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — Development workflow and coding standards
