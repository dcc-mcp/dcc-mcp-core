# dcc-mcp-core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[中文](README_zh.md) | English

**Production-grade foundation for AI-assisted DCC workflows** — combining the **Model Context Protocol (MCP 2025-03-26 Streamable HTTP)** with a **zero-code Skills system** built on [agentskills.io 1.0](https://agentskills.io/specification). A **Rust-powered core with Python bindings (PyO3)** delivering enterprise-grade performance, security, and scalability — all with **zero runtime Python dependencies**. Supports Python 3.7–3.13.

> **Note**: This project is in active development (v0.14+). APIs may evolve; see `CHANGELOG.md` for version history.

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

1. **Context Explosion** — MCP has no mechanism to scope tools to specific sessions or instances, causing request bloat with multi-DCC setups.
2. **No Lifecycle Control** — Can't discover instance state (active scene, documents, process health) or control startup/shutdown.

### Our Approach: MCP + Skills System

We **reuse and extend** the existing MCP ecosystem, adding:

| Capability | Benefit |
|---|---|
| **Gateway Election & Version Awareness** | Multi-instance load balancing; automatic handoff when a newer DCC launches |
| **Session Isolation** | Each AI session talks to its own DCC instance; prevents context bleeding |
| **Skills System (Zero-Code)** | Define tools as `SKILL.md` + sibling YAML/scripts — no Python glue code needed |
| **Progressive Discovery** | Scope tools by DCC type, instance, scene, product; prevents context explosion |
| **Instance Tracking** | Know active documents, PIDs, display names; enable smart routing |
| **Structured Results** | Every tool returns `(success, message, context, prompt)` for AI reasoning |
| **Workflow Primitive** | Declarative multi-step workflows with retry / timeout / idempotency / approval gates |
| **Artefact Hand-off** | Content-addressed (SHA-256) file passing between tools and workflow steps |
| **Job Lifecycle + SSE** | `tools/call` opt-in async dispatch, `$/dcc.jobUpdated` notifications, SQLite persistence |

This isn't reinventing MCP — it's **solving MCP's blind spots for desktop automation**.

---

## Why dcc-mcp-core Over Alternatives?

| Aspect | dcc-mcp-core | Generic MCP | CLI Tools | Browser Extensions |
|---|---|---|---|---|
| **DCC State Awareness** | Scenes, docs, instance IDs | No | No | Partial |
| **Multi-Instance Support** | Gateway election + session isolation | Single endpoint | No | No |
| **Context Scoping** | By DCC / scene / product | Global tools | No | Limited |
| **Zero-Code Tools** | `SKILL.md` + sibling files | Full Python required | Scripts only | No |
| **Performance** | Rust + zero-copy + IPC | Python overhead | Process overhead | Network overhead |
| **Security** | Sandbox + audit log | Manual | Manual | None |
| **Cross-Platform** | Windows / macOS / Linux | Yes | Limited | Browser only |

AI-friendly docs: [AGENTS.md](AGENTS.md) · [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) · [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

---

## Architecture: The Three-Layer Stack

```
+-----------------------------------------------------------------+
|  AI Agent (Claude, GPT, etc.)                                   |
|  Calls tools via MCP protocol (tools/list, tools/call)          |
+-------------------------------+---------------------------------+
                                |
                        MCP Streamable HTTP
                                |
+-------------------------------v---------------------------------+
|  Gateway Server (Rust / HTTP)                                   |
|  +-- Version-aware instance election                            |
|  +-- Session isolation & routing                                |
|  +-- Tool discovery (skills-derived)                            |
|  +-- Job lifecycle + SSE notifications                          |
|  +-- Workflow execution engine                                  |
+-------------------------------+---------------------------------+
                                |
                 IPC (Named Pipe / Unix Socket) via DccLink
                                |
          +---------------------+---------------------+
          |                     |                     |
  +-------v-------+     +-------v-------+     +-------v-------+
  |  Maya Adapter  |     | Blender Adapter|     | Houdini Adapter|
  |  (_core.pyd)   |     |  (_core.so)    |     |  (_core.so)   |
  +-------+--------+     +-------+--------+     +-------+-------+
          |                      |                      |
    Python 3.7+             Python 3.7+            Python 3.7+
    (zero deps)             (zero deps)            (zero deps)
```

- **Layer 1 — AI Agent**: Calls tools via standard MCP protocol (`tools/list`, `tools/call`, notifications).
- **Layer 2 — Gateway**: Orchestrates discovery, session isolation, request routing, job lifecycle, and workflow execution. Maintains a `__gateway__` sentinel for version-aware election.
- **Layer 3 — DCC Adapters**: DCC-side Python packages (Maya, Blender, Photoshop, Houdini…) that embed the `_core` native extension plus the Skills system. WebView-host adapters (AuroraView, Electron panels) and WebSocket bridges (Photoshop, ZBrush) use narrower capability surfaces.

---

## Quick Start

### Installation

```bash
# From PyPI (pre-built wheels for Python 3.7+)
pip install dcc-mcp-core

# Or from source (requires Rust 1.85+)
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
vx just dev           # recommended — uses the project's canonical feature set
# or: pip install -e .
```

### Serve a DCC over MCP — Skills-First (recommended)

`create_skill_server` wires up the full Skills-First entry point: `tools/list` returns six core tools plus one stub per unloaded skill. Agents call `search_skills` → `load_skill` to activate the tools they need, keeping the context window small.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server(
    "maya",
    McpHttpConfig(port=8765),
)
handle = server.start()
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"
# ... later ...
handle.shutdown()
```

### Low-level: register tools manually

```python
import json
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher, EventBus,
    McpHttpServer, McpHttpConfig,
    success_result, scan_and_load,
)

skills, skipped = scan_and_load(dcc_name="maya")
print(f"Loaded {len(skills)} skills, skipped {len(skipped)}")

registry = ToolRegistry()
registry.register(
    name="get_scene",
    description="Return the active Maya scene path",
    category="scene",
    dcc="maya",
    version="1.0.0",
)

dispatcher = ToolDispatcher(registry)
dispatcher.register_handler(
    "get_scene",
    lambda params: success_result("OK", path="/proj/shots/sh010.ma").to_dict(),
)

# Optional: observe lifecycle events
bus = EventBus()
bus.subscribe("action.after_execute", lambda **kw: print(f"done: {kw['action_name']}"))

result = dispatcher.dispatch("get_scene", json.dumps({}))
print(result["output"])   # {"success": True, "message": "OK", "context": {"path": ...}}

# Expose registry over MCP (register ALL handlers before .start())
server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()
```

---

## Core Concepts

### ToolResult — Structured Results for AI

All skill execution results use `ToolResult`, designed to be AI-friendly with structured context and follow-up guidance.

```python
from dcc_mcp_core import ToolResult, success_result, error_result

# Factory functions (recommended). Extra kwargs land in `context`.
ok = success_result(
    "Sphere created",
    prompt="Consider adding materials or adjusting UVs",
    object_name="sphere1",
    position=[0, 1, 0],
)
# ok.context == {"object_name": "sphere1", "position": [0, 1, 0]}

err = error_result(
    "Failed to create sphere",
    "Radius must be positive",
)

# Direct construction
result = ToolResult(
    success=True,
    message="Operation completed",
    context={"key": "value"},
)

result.success   # bool
result.message   # str
result.prompt    # Optional[str] — AI next-step suggestion
result.error     # Optional[str] — error details
result.context   # dict — arbitrary structured data
result.to_json() # JSON-safe serialization for transport
```

### ToolRegistry & Dispatcher

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher, EventBus

registry = ToolRegistry()
registry.register(name="my_tool", description="My tool", category="tools", version="1.0.0")

dispatcher = ToolDispatcher(registry)
dispatcher.register_handler("my_tool", lambda params: {"done": True})

result = dispatcher.dispatch("my_tool", json.dumps({}))
# result == {"action": "my_tool", "output": {"done": True}, "validation_skipped": True}

bus = EventBus()
sub_id = bus.subscribe("action.before_execute", lambda **kw: print(f"before: {kw}"))
bus.publish("action.before_execute", action_name="test")
bus.unsubscribe("action.before_execute", sub_id)
```

---

## Skills System — Zero-Code MCP Tool Registration

The **Skills system** lets you register any script (Python, MEL, MaxScript, Batch, Shell, PowerShell, JavaScript, TypeScript) as an MCP tool with **zero Python glue code**. Aligned with the [agentskills.io 1.0](https://agentskills.io/specification) specification.

### Architectural Rule — Sibling-File Pattern (v0.15+)

Every dcc-mcp-core extension — `tools`, `groups`, `workflows`, `prompts`, `next-tools`, etc. — lives in a **sibling file** pointed at by a `metadata.dcc-mcp.<feature>` key. The `SKILL.md` frontmatter itself only carries the six standard agentskills.io fields (`name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`).

```
my-automation/
├── SKILL.md                      # frontmatter + human-readable body
├── tools.yaml                    # tool definitions + annotations + groups
├── workflows/
│   └── vendor_intake.workflow.yaml
├── prompts/
│   └── review_scene.prompt.yaml
└── scripts/
    ├── cleanup.py
    └── publish.sh
```

### Five Minutes to Your First Skill

**1. Create `maya-cleanup/SKILL.md`:**

```yaml
---
name: maya-cleanup
description: >-
  Domain skill — Scene optimisation and cleanup tools for Maya.
  Not for authoring new geometry — use maya-geometry for that.
license: MIT
compatibility: "Maya 2024+, Python 3.7+"
metadata:
  dcc-mcp:
    layer: domain
    dcc: maya
    tools: tools.yaml
    search-hint: "cleanup, optimise, unused nodes"
    depends: [dcc-diagnostics]
---
# Maya Scene Cleanup

Automated tools for optimising and validating Maya scenes.
```

**2. Create `maya-cleanup/tools.yaml`:**

```yaml
tools:
  - name: cleanup
    description: "Remove unused nodes from the active scene."
    script: scripts/cleanup.py
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: true
    next-tools:
      on-success: [maya_cleanup__validate]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]

  - name: validate
    description: "Validate scene integrity after cleanup."
    script: scripts/validate.mel
    annotations:
      read_only_hint: true
```

**3. Create `maya-cleanup/scripts/cleanup.py`:**

```python
#!/usr/bin/env python
"""Clean unused nodes from the scene."""
from __future__ import annotations

import json
import sys


def main() -> int:
    result = {"success": True, "message": "Cleaned up 42 unused nodes"}
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

**4. Register and call:**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/maya-cleanup/.."

from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
# Agent calls search_skills("cleanup") → load_skill("maya-cleanup") → maya_cleanup__cleanup
```

That's it — no Python glue code, just `SKILL.md` + `tools.yaml` + scripts.

### Supported Script Types

| Extension | Type | Execution |
|---|---|---|
| `.py` | Python | `subprocess` with system Python |
| `.mel` | MEL (Maya) | Via DCC adapter |
| `.ms` | MaxScript | Via DCC adapter |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |
| `.ts` | TypeScript | `node` (via ts-node or tsx) |

See [`examples/skills/`](examples/skills/) for complete reference packages.

### Bundled Skills — Zero Configuration Required

`dcc-mcp-core` ships **two core skills** directly inside the wheel. They are available immediately after `pip install dcc-mcp-core` — no repository clone or `DCC_MCP_SKILL_PATHS` configuration needed.

| Skill | Tools | Purpose |
|---|---|---|
| `dcc-diagnostics` | `screenshot`, `audit_log`, `tool_metrics`, `process_status` | Observability & debugging for any DCC |
| `workflow` | `run_chain` | Multi-step action chaining with context propagation |

```python
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths

print(get_bundled_skills_dir())
# /path/to/site-packages/dcc_mcp_core/skills

paths = get_bundled_skill_paths()                       # default ON
paths = get_bundled_skill_paths(include_bundled=False)  # opt-out
```

DCC adapters (e.g. `dcc-mcp-maya`) include the bundled skills by default. To opt out: `start_server(include_bundled=False)`.

---

## Solving MCP Context Explosion

**The problem**: Stock MCP returns *all* tools in `tools/list`, even those irrelevant to the current task or DCC instance. With 3 DCC instances × 50 skills × 5 scripts = **750 tools**, the context window fills instantly.

**Progressive discovery** — dcc-mcp-core shrinks this to what the agent actually needs:

1. **Skill stubs** — `tools/list` returns six meta-tools plus one stub per unloaded skill (`__skill__<name>`). Agents call `search_skills(query)` → `load_skill(name)` to activate the real tools.
2. **Instance awareness** — Each DCC registers its active documents, PID, display name, scope level.
3. **Smart tool scoping** — Tools filter by DCC type, trust scope (Repo < User < System < Admin), product whitelist, and policy.
4. **Session isolation** — An AI session is pinned to one DCC instance; it sees only that instance's tools.
5. **Gateway election** — When a newer DCC version launches, traffic automatically hands off to it.

Stock MCP:

```
tools/list response:
  100 Maya + 100 Houdini + 100 Blender + 250 shared = 550 tool definitions
```

With dcc-mcp-core (Skills-First):

```
tools/list response (Maya session, nothing loaded yet):
  6 core tools + 22 skill stubs = 28 entries
→ agent loads only the 3 skills it needs → ~30 tools in context
```

---

## Highlights

- **Rust-powered performance** — Zero-copy serialisation (`rmp-serde`), LZ4 shared memory, lock-free data structures.
- **Zero runtime Python deps** — Everything compiled into the native extension.
- **Skills-First MCP server** — `create_skill_server()` gives a ready-to-use MCP 2025-03-26 Streamable HTTP endpoint with progressive discovery.
- **Workflow primitive** — `WorkflowSpec` / `WorkflowExecutor`: declarative multi-step workflows with retry, timeout, idempotency keys, approval gates, foreach / parallel / branch steps, SQLite-backed recovery.
- **Scheduler** — Cron + webhook (HMAC-SHA256) triggered workflows via sibling `schedules.yaml` (opt-in feature).
- **Artefact hand-off** — Content-addressed (SHA-256) `FileRef` + `ArtefactStore` for passing files between tools and workflow steps.
- **Job lifecycle & notifications** — Opt-in async `tools/call`, SSE channels (`notifications/progress`, `$/dcc.jobUpdated`, `$/dcc.workflowUpdated`), optional SQLite persistence surviving restarts.
- **Resources & Prompts primitives** — Live DCC state (`scene://current`, `capture://current_window`, `audit://recent`, `artefact://sha256/<hex>`) and reusable prompt templates from sibling YAML.
- **Thread affinity** — `DeferredExecutor` routes main-thread-only tools to the DCC's event loop safely; Tokio workers handle the rest.
- **Gateway & multi-instance** — Version-aware first-wins election, SSE multiplex across sessions, async dispatch + wait-for-terminal passthrough.
- **Resilient IPC** — DccLink framing over `ipckit` (Named Pipe / Unix Socket): `IpcChannelAdapter`, `GracefulIpcChannelAdapter`, `SocketServerAdapter`.
- **Process management** — Launch, monitor, auto-recover DCC processes.
- **Sandbox security** — Policy-based access control with audit logging; `ToolAnnotations` safety hints; `ToolValidator` schema validation.
- **Screen capture** — Full-screen or per-window (HWND `PrintWindow`) viewport capture for AI visual feedback.
- **USD integration** — Universal Scene Description read/write bridge.
- **Structured telemetry** — Tracing, recording, optional Prometheus `/metrics` exporter.
- **~180 public Python symbols** with full type stubs (`python/dcc_mcp_core/_core.pyi`).

---

## Architecture Overview — 18 Rust Crates

`dcc-mcp-core` is organised as a **Rust workspace of 18 crates**, compiled into a single native Python extension (`_core`) via PyO3 / maturin:

| Crate | Responsibility | Key Types |
|---|---|---|
| `dcc-mcp-naming` | SEP-986 naming validators | `validate_tool_name`, `validate_action_id`, `TOOL_NAME_RE` |
| `dcc-mcp-models` | Data models | `ToolResult`, `SkillMetadata`, `ToolDeclaration` |
| `dcc-mcp-actions` | Tool execution lifecycle | `ToolRegistry`, `ToolDispatcher`, `ToolValidator`, `ToolPipeline`, `EventBus` |
| `dcc-mcp-skills` | Skills discovery & loading | `SkillScanner`, `SkillCatalog`, `SkillWatcher`, dependency resolver |
| `dcc-mcp-protocols` | MCP protocol types | `ToolDefinition`, `ResourceDefinition`, `PromptDefinition`, `ToolAnnotations`, `BridgeKind` |
| `dcc-mcp-transport` | IPC communication | `DccLinkFrame`, `IpcChannelAdapter`, `GracefulIpcChannelAdapter`, `SocketServerAdapter`, `FileRegistry` |
| `dcc-mcp-process` | Process management | `PyDccLauncher`, `PyProcessMonitor`, `PyProcessWatcher`, `PyCrashRecoveryPolicy`, `HostDispatcher` |
| `dcc-mcp-sandbox` | Security | `SandboxPolicy`, `SandboxContext`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | Shared memory | `PySharedBuffer`, `PySharedSceneBuffer`, LZ4 compression |
| `dcc-mcp-capture` | Screen capture | `Capturer`, `WindowFinder`, HWND / DXGI / X11 / Mock backends |
| `dcc-mcp-telemetry` | Observability | `TelemetryConfig`, `ToolRecorder`, `ToolMetrics`, optional Prometheus |
| `dcc-mcp-usd` | USD integration | `UsdStage`, `UsdPrim`, `scene_info_json_to_stage` |
| `dcc-mcp-http` | MCP Streamable HTTP server | `McpHttpServer`, `McpHttpConfig`, `McpServerHandle`, gateway, job manager |
| `dcc-mcp-server` | Binary entry point | `dcc-mcp-server` CLI, gateway runner |
| `dcc-mcp-workflow` | Workflow engine (opt-in) | `WorkflowSpec`, `WorkflowExecutor`, `WorkflowHost`, `StepPolicy`, `RetryPolicy` |
| `dcc-mcp-scheduler` | Cron + webhook scheduler (opt-in) | `ScheduleSpec`, `TriggerSpec`, `SchedulerService`, HMAC verification |
| `dcc-mcp-artefact` | Content-addressed artefact store | `FileRef`, `FilesystemArtefactStore`, `InMemoryArtefactStore` |
| `dcc-mcp-utils` | Infrastructure | Filesystem helpers, type wrappers, constants, JSON |

---

## Selected APIs

### Transport Layer — Inter-Process Communication

```python
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter, SocketServerAdapter

# Server: create channel and wait for client
server = IpcChannelAdapter.create("dcc-mcp-maya")
server.wait_for_client()

# Client: connect to server
client = IpcChannelAdapter.connect("dcc-mcp-maya")
client.send_frame(DccLinkFrame(msg_type="Call", seq=1, body=b'{"method":"ping"}'))
reply = client.recv_frame()      # DccLinkFrame(msg_type, seq, body)

# Multi-client socket server (for bridge-mode DCCs)
sock_server = SocketServerAdapter("/tmp/dcc-mcp.sock",
                                  max_connections=10,
                                  connection_timeout_secs=30)
```

### Process Management — DCC Lifecycle Control

```python
from dcc_mcp_core import (
    PyDccLauncher, PyProcessMonitor, PyProcessWatcher, PyCrashRecoveryPolicy,
)

launcher = PyDccLauncher(dcc_type="maya", version="2025")
process = launcher.launch(
    script_path="/path/to/startup.py",
    working_dir="/project",
    env_vars={"MAYA_RENDER_THREADS": "4"},
)

monitor = PyProcessMonitor()
monitor.track(process)
stats = monitor.stats(process)     # CPU, memory, uptime

watcher = PyProcessWatcher(
    recovery_policy=PyCrashRecoveryPolicy(max_restarts=3, cooldown_sec=10),
)
watcher.watch(process)
```

### Sandbox Security — Policy-Based Access Control

```python
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator

policy = SandboxPolicy()
ctx = SandboxContext(policy)
validator = InputValidator(ctx)

allowed, reason = validator.validate("delete_all_files")
if not allowed:
    print(f"Blocked by policy: {reason}")

# Audit trail
for entry in ctx.audit_log.entries():
    print(f"{entry.action} -> {entry.outcome}")
```

### Workflow & Artefact Hand-off (v0.14+)

```python
from dcc_mcp_core import (
    WorkflowSpec, BackoffKind,
    artefact_put_bytes, artefact_get_bytes,
)

spec = WorkflowSpec.from_yaml_str(yaml_text)
spec.validate()                    # static idempotency_key + template check
print(spec.steps[0].policy.retry.next_delay_ms(2))

ref = artefact_put_bytes(b"hello", mime="text/plain")
print(ref.uri)                     # "artefact://sha256/<hex>"
assert artefact_get_bytes(ref.uri) == b"hello"
```

See [AGENTS.md](AGENTS.md) for the full feature matrix and decision tree.

---

## Development Setup

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Recommended: use vx (universal dev tool manager) — https://github.com/loonghao/vx
vx just dev            # build + install dev wheel (uses canonical feature set)
vx just test           # run Python tests
vx just test-rust      # run Rust unit/integration tests
vx just lint           # full lint check (Rust + Python)
vx just preflight      # pre-commit checks (cargo check + clippy + fmt + test-rust)
vx just ci             # full local CI pipeline
```

### Without `vx`

```bash
python -m venv venv
source venv/bin/activate   # Windows: venv\Scripts\activate
pip install maturin pytest pytest-cov ruff mypy

# The canonical feature list lives in the root justfile — see `just print-dev-features`.
maturin develop --features "$(just print-dev-features)"
pytest tests/ -v
ruff check python/ tests/ examples/
cargo clippy --workspace -- -D warnings
```

The feature list is the **single source of truth in `justfile`** (`OPT_FEATURES`, `DEV_FEATURES`, `WHEEL_FEATURES`, `WHEEL_FEATURES_PY37`). CI, local dev, and release wheels all read from the same place.

---

## Release Process

This project uses [Release Please](https://github.com/googleapis/release-please) to automate versioning and releases:

1. **Develop**: Create a branch from `main` and commit using [Conventional Commits](https://www.conventionalcommits.org/).
2. **Merge**: Open a PR and merge to `main`.
3. **Release PR**: Release Please automatically creates / updates a release PR that bumps the version and updates `CHANGELOG.md`.
4. **Publish**: When the release PR merges, a GitHub Release is created and the wheel is published to PyPI.

### Commit Message Format

| Prefix | Description | Version Bump |
|---|---|---|
| `feat:` | New feature | Minor (`0.x.0`) |
| `fix:` | Bug fix | Patch (`0.0.x`) |
| `feat!:` or `BREAKING CHANGE:` | Breaking change | Major (`x.0.0`) |
| `docs:` | Documentation only | No release |
| `chore:` | Maintenance | No release |
| `ci:` | CI/CD changes | No release |
| `refactor:` | Code refactoring | No release |
| `test:` | Adding tests | No release |
| `build:` | Build system / dependency changes | No release |

```bash
git commit -m "feat: add batch skill execution support"
git commit -m "fix: resolve middleware chain ordering issue"
git commit -m "feat!: redesign skill registry API"
git commit -m "feat(skills): add PowerShell script support"
git commit -m "docs: update API reference"
```

---

## Contributing

Contributions are welcome — please open a Pull Request.

1. Fork the repository and clone your fork.
2. Create a feature branch: `git checkout -b feat/my-feature`.
3. Make your changes following the coding standards below.
4. Run tests and linting:
   ```bash
   vx just lint        # check code style
   vx just test        # run tests
   vx just preflight   # run all pre-commit checks
   ```
5. Commit using [Conventional Commits](https://www.conventionalcommits.org/).
6. Push and open a Pull Request against `main`.

### Coding Standards

- **Style**: Rust via `cargo fmt`, Python via `ruff format` (line length 120, double quotes).
- **Type hints**: All public Python APIs must have type annotations; Rust uses `thiserror` for errors and `tracing` for logging.
- **Docstrings**: Google-style docstrings for all public modules, classes, and functions.
- **Testing**: New features must include tests; maintain or improve coverage.
- **Imports (Python)**: `from __future__ import annotations` first, then stdlib → third-party → local with section comments.

---

## License

MIT — see the [LICENSE](LICENSE) file.

---

## AI Agent Resources

If you're an AI coding agent, also read:

- [AGENTS.md](AGENTS.md) — Navigation map for AI agents (entry point, decision tables, top traps).
- [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) — Detailed agent rules, traps, code style, and project-specific architecture constraints.
- [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md) — Complete API skill definition.
- [`python/dcc_mcp_core/__init__.py`](python/dcc_mcp_core/__init__.py) — Full public API surface (~180 symbols).
- [`python/dcc_mcp_core/_core.pyi`](python/dcc_mcp_core/_core.pyi) — Ground-truth type stubs (parameter names, types, signatures).
- [`llms.txt`](llms.txt) — Concise API reference optimised for LLMs.
- [`llms-full.txt`](llms-full.txt) — Complete API reference optimised for LLMs.
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development workflow and coding standards.
