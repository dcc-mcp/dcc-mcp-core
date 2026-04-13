# AGENTS.md — dcc-mcp-core AI Agent Guide

> **Purpose**: This file helps AI coding agents (Claude, Copilot, Cursor, Codex, Gemini, Devin, etc.) understand, navigate, and contribute to this project effectively. Read this before writing any code.

## Quick Decision Guide — Use the Right API

> **AI agents**: When implementing features, ALWAYS check if dcc-mcp-core already provides what you need before writing custom code.

| Task | Prefer this API (not custom code) |
|------|----------------------------------|
| Return action result | `success_result()` / `error_result()` — never return raw dicts |
| Register a callable as DCC tool | `ActionRegistry.register()` + `ActionDispatcher` |
| Start a DCC skill server (Skills-First) | `create_skill_manager("maya")` — one call, auto-discovers from `DCC_MCP_MAYA_SKILL_PATHS` |
| Discover/load skill packages | `create_skill_manager()` or `scan_and_load()` — not manual file scanning |
| Set app-specific skill paths | `DCC_MCP_{APP}_SKILL_PATHS` env var (e.g. `DCC_MCP_MAYA_SKILL_PATHS`) |
| Get bundled skills (zero config) | `get_bundled_skill_paths()` — included in wheel, no path config needed |
| Validate JSON input params | `ActionValidator.from_schema_json()` — not manual isinstance checks |
| Connect to running DCC process | `connect_ipc(TransportAddress.default_local(...))` |
| Define MCP tool for LLM | `ToolDefinition` + `ToolAnnotations` — not raw JSON dicts |
| Share large data zero-copy | `PySharedSceneBuffer.write()` — not pickle/JSON for large data |
| Monitor process health | `PyProcessWatcher` + `PyCrashRecoveryPolicy` |
| Enforce security on AI actions | `SandboxPolicy` + `SandboxContext` |
| Measure action performance | `ActionRecorder` + `ActionMetrics` |
| Capture DCC viewport | `Capturer.new_auto().capture()` |
| Exchange scene as USD | `UsdStage` + `scene_info_json_to_stage()` |
| Safe RPyC value transport | `wrap_value()` / `unwrap_value()` |
| Multi-version action lookup | `VersionedRegistry` + `VersionConstraint.parse()` |
| Expose DCC tools over HTTP/MCP | `create_skill_manager("maya", McpHttpConfig(port=8765))` — one-call Skills-First setup |

## Project Overview

**dcc-mcp-core** is the foundational library for the DCC (Digital Content Creation) Model Context Protocol (MCP) ecosystem. It provides a **Rust-powered core with Python bindings (PyO3/maturin)** that enables AI assistants to interact with DCC software (Maya, Blender, Houdini, 3ds Max, etc.).

### Key Architecture Facts

- **Language**: Rust core (12 crates workspace) + Python bindings via PyO3
- **Build system**: `cargo` (Rust) + `maturin` (Python wheels)
- **Python package**: `dcc_mcp_core` with ~130 public symbols re-exported from `_core` native extension (see `python/dcc_mcp_core/__init__.py`)
- **Zero runtime Python dependencies** — everything is compiled into the Rust core
- **Version**: 0.12.9 (use Release Please for versioning — never manually bump)
- **Python support**: 3.7–3.13 (CI tests 3.7–3.13; abi3-py38 wheel for 3.8+)

## Repository Structure

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 entry point (_core module)
├── Cargo.toml                  # Workspace definition (12 crates)
├── pyproject.toml              # Python package metadata
├── justfile                    # Development commands (use: vx just <recipe>)
│
├── crates/                     # Rust workspace crates
│   ├── dcc-mcp-models/         # ActionResultModel, SkillMetadata
│   ├── dcc-mcp-actions/        # ActionRegistry, EventBus, Pipeline, Dispatcher, Validator
│   ├── dcc-mcp-skills/         # SkillScanner, SkillLoader, SkillWatcher, Resolver
│   ├── dcc-mcp-protocols/      # MCP types: ToolDefinition, ResourceDefinition, Prompt, DccAdapter
│   ├── dcc-mcp-transport/      # IPC, ConnectionPool, SessionManager, CircuitBreaker, FramedChannel
│   ├── dcc-mcp-process/        # PyDccLauncher, ProcessMonitor, ProcessWatcher, CrashRecovery
│   ├── dcc-mcp-telemetry/      # Tracing/recording infrastructure
│   ├── dcc-mcp-sandbox/        # Security policy, input validation, audit logging
│   ├── dcc-mcp-shm/            # Shared memory buffers (LZ4 compressed)
│   ├── dcc-mcp-capture/        # Screen/window capture backend
│   ├── dcc-mcp-usd/            # USD scene description bridge
│   ├── dcc-mcp-http/           # MCP Streamable HTTP server (2025-03-26 spec, McpHttpServer)
│   └── dcc-mcp-utils/          # Filesystem, type wrappers, constants, JSON helpers
│
├── python/dcc_mcp_core/
│   ├── __init__.py             # Public API re-exports (~120 symbols) — ALWAYS read this first
│   ├── _core.pyi               # Type stubs (auto-generated-ish) — ground truth for parameter names
│   └── py.typed                # PEP 561 marker
│
├── tests/                      # Python integration tests (26 files)
├── examples/skills/            # 9 example SKILL.md packages (hello-world, maya-*, git-*, etc.)
├── docs/                       # VitePress documentation site (EN + ZH)
│   ├── api/                    # API reference per module
│   └── guide/                  # User guides & tutorials
├── llms.txt                    # Concise API reference for LLMs
├── llms-full.txt               # Complete API reference for LLMs
└── .agents/skills/             # VX toolchain skills (IDE-agnostic)
```

## Build & Test Commands

### Prerequisites

```bash
# Install vx (recommended universal tool manager): https://github.com/loonghao/vx
# Or ensure you have: Rust 1.85+, Python 3.8+, maturin
```

### Core Commands (via `vx just` or `just`)

| Command | Purpose |
|---------|---------|
| `vx just check` | Cargo check all workspace crates |
| `vx just clippy` | Clippy lint with `-D warnings` (CI strict) |
| `vx just fmt` | Format Rust + Python |
| `vx just fmt-check` | Check format only (CI mode) |
| `vx just test-rust` | Run all Rust unit tests (`cargo test --workspace`) |
| `vx just dev` | Build + install wheel in dev mode (`maturin develop`) |
| `vx just install` | Build release wheel + pip install |
| `vx just test` | Run pytest on `tests/` |
| `vx just test-cov` | Run tests with coverage report |
| `vx just lint-py` | Ruff check Python code |
| `vx just lint-py-fix` | Fix + format Python |
| `vx just lint` | Full lint: clippy + fmt-check + lint-py |
| `vx just lint-fix` | Auto-fix all lint issues |
| `vx just preflight` | Pre-commit: check + clippy + fmt-check + test-rust |
| `vx just ci` | Full CI pipeline: preflight + install + test + lint-py |
| `vx just build` | Build release wheel |
| `vx just clean` | Remove all build artifacts |

### Running Specific Tests

```bash
# Run a single test file
vx just test -- tests/test_skills.py -v

# Run with coverage for a specific module
vx just test-cov -- tests/test_actions.py -v

# Run Rust tests for a specific crate
cargo test -p dcc-mcp-actions --workspace
```

## Code Style & Conventions

### Rust

- Edition 2024, MSRV 1.85
- Use `tracing` for logging (never `println!` / `eprintln!`)
- Use `thiserror` for error types
- Use `parking_lot` instead of `std::sync::Mutex`
- Keep files under 1000 lines (split into submodules if needed)

### Python

- Formatter: `ruff format` (line length: 120, double quotes)
- Linter: `ruff check` (includes isort via `I` rules)
- Target: Python 3.7+ (CI tests 3.7–3.13)
- Docstrings: Google-style
- All public APIs must have type annotations

### Import Organization (Strict)

```python
# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import ActionResultModel
```

## API Reference (What AI Agents Should Know)

> **IMPORTANT**: Always prefer `python/dcc_mcp_core/__init__.py` over guessing imports.
> That file lists every public symbol. Never import from internal paths like `dcc_mcp_core._core.*` directly.

### Current Public API (Rust-backed, v0.12+)

The public API is in `python/dcc_mcp_core/__init__.py`. Key domains:

#### Actions System

```python
from dcc_mcp_core import (
    ActionRegistry,       # Thread-safe registry: register/get/list actions
    ActionDispatcher,     # Typed dispatch with validation
    ActionValidator,      # Input parameter validation before execution
    ActionPipeline,       # Middleware-style processing pipeline
    ActionMetrics,        # Performance/execution metrics
    ActionRecorder,       # Record/replay action executions
    EventBus,             # Pub/sub lifecycle events
    ActionResultModel,    # Structured result model
    success_result,       # Factory: ActionResultModel(success=True, ...)
    error_result,         # Factory: ActionResultModel(success=False, ...)
    from_exception,       # Factory: wrap Python exception as result
    validate_action_result,  # Normalize dict/str/None → ActionResultModel
)
```

**ActionRegistry patterns:**

```python
reg = ActionRegistry()

# Register an action
reg.register(
    name="create_sphere",
    description="Create a polygon sphere",
    category="geometry",
    tags=["geo", "create"],
    dcc="maya",
    version="1.0.0",
    input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}',
)

# Look up
meta = reg.get_action("create_sphere")
meta = reg.get_action("create_sphere", dcc_name="maya")  # scoped
names = reg.list_actions_for_dcc("maya")
all_actions = reg.list_actions()
dccs = reg.get_all_dccs()

# Batch registration (preferred over repeated register() calls for large sets)
reg.register_batch([
    {"name": "create_sphere", "category": "geometry", "dcc": "maya", "tags": ["create"]},
    {"name": "delete_mesh",   "category": "edit",     "dcc": "maya", "tags": ["delete"]},
    {"name": "create_cube",   "category": "geometry", "dcc": "blender"},
])

# Unregister (global removes from all DCCs; scoped removes only one DCC's entry)
removed = reg.unregister("create_sphere")                  # global: True if found
removed = reg.unregister("create_sphere", dcc_name="maya") # scoped to maya only

# Search & discovery (all filters AND-ed; None / [] = no filter)
results = reg.search_actions(category="geometry")                  # by category
results = reg.search_actions(tags=["create", "mesh"])              # must have ALL tags
results = reg.search_actions(category="geometry", dcc_name="maya") # category + DCC
categories = reg.get_categories()                                  # sorted unique categories
tags = reg.get_tags(dcc_name="maya")                               # sorted unique tags

# Version-aware registry
from dcc_mcp_core import SemVer, VersionedRegistry, VersionConstraint
vreg = VersionedRegistry()
vreg.register_versioned("my_action", dcc="maya", version="1.2.0")
vreg.register_versioned("my_action", dcc="maya", version="2.0.0")
result = vreg.resolve("my_action", dcc="maya", constraint=">=1.0.0")   # → version "2.0.0"
result = vreg.resolve("my_action", dcc="maya", constraint="^1.0.0")    # → version "1.2.0"
all_results = vreg.resolve_all("my_action", dcc="maya", constraint="*")  # all versions sorted
latest = vreg.latest_version("my_action", dcc="maya")                  # → "2.0.0"
versions = vreg.versions("my_action", dcc="maya")                      # → ["1.2.0", "2.0.0"]
keys = vreg.keys()                                                      # → [("my_action", "maya")]
removed = vreg.remove("my_action", dcc="maya", constraint="^1.0.0")    # → 1 (versions removed)
```

**ActionPipeline patterns:**

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
reg.register("create_sphere", description="Create sphere", category="geometry")

dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"name": "sphere1"})

pipeline = ActionPipeline(dispatcher)

# Built-in middleware (add in desired order)
pipeline.add_logging(log_params=True)         # tracing log before/after each action
timing = pipeline.add_timing()                # measure per-action latency
audit = pipeline.add_audit(record_params=True) # in-memory audit log
rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)  # fixed-window rate limiter

# Python callable hooks (flexible custom middleware)
pipeline.add_callable(
    before_fn=lambda action: print(f"before: {action}"),
    after_fn=lambda action, success: print(f"after: {action} ok={success}"),
)

# Dispatch
result = pipeline.dispatch("create_sphere", '{"radius": 1.0}')
result["output"]          # {"name": "sphere1"}
result["action"]          # "create_sphere"
result["validation_skipped"]  # bool

# Register handler directly on pipeline (mirrors ActionDispatcher)
pipeline.register_handler("delete_sphere", lambda params: True)

# Introspect middleware
pipeline.middleware_count()   # int
pipeline.middleware_names()   # ["logging", "timing", "audit", "rate_limit", "python_callable"]
pipeline.handler_count()      # int

# Query middleware state
timing.last_elapsed_ms("create_sphere")  # int | None (milliseconds)
audit.records()                           # list[dict] with action/success/error/timestamp_ms
audit.records_for_action("create_sphere") # filtered records
audit.record_count()                      # int
audit.clear()                             # reset
rl.call_count("create_sphere")            # int (calls in current window)
rl.max_calls                              # int
rl.window_ms                              # int
```

**ActionPipeline patterns:**

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
reg.register("create_sphere", description="Create sphere", category="geometry")

dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"name": "sphere1"})

pipeline = ActionPipeline(dispatcher)

# Built-in middleware (add in desired order)
pipeline.add_logging(log_params=True)         # tracing log before/after each action
timing = pipeline.add_timing()                # measure per-action latency
audit = pipeline.add_audit(record_params=True) # in-memory audit log
rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)  # fixed-window rate limiter

# Python callable hooks (flexible custom middleware)
pipeline.add_callable(
    before_fn=lambda action: print(f"before: {action}"),
    after_fn=lambda action, success: print(f"after: {action} ok={success}"),
)

# Dispatch
result = pipeline.dispatch("create_sphere", '{"radius": 1.0}')
result["output"]          # {"name": "sphere1"}
result["action"]          # "create_sphere"
result["validation_skipped"]  # bool

# Register handler directly on pipeline (mirrors ActionDispatcher)
pipeline.register_handler("delete_sphere", lambda params: True)

# Introspect middleware
pipeline.middleware_count()   # int
pipeline.middleware_names()   # ["logging", "timing", "audit", "rate_limit", "python_callable"]
pipeline.handler_count()      # int

# Query middleware state
timing.last_elapsed_ms("create_sphere")  # int | None (milliseconds)
audit.records()                           # list[dict] with action/success/error/timestamp_ms
audit.records_for_action("create_sphere") # filtered records
audit.record_count()                      # int
audit.clear()                             # reset
rl.call_count("create_sphere")            # int (calls in current window)
rl.max_calls                              # int
rl.window_ms                              # int
```

**ActionResultModel fields:**

```python
result = success_result(
    message="Sphere created",
    prompt="Consider adding materials or adjusting UVs",  # AI next-step hint
    context={"object_name": "sphere1", "position": [0, 0, 0]}
)
result.success   # bool
result.message   # str
result.prompt    # Optional[str] — guidance for AI's next step
result.error     # Optional[str] — error details (set when success=False)
result.context   # dict — arbitrary structured data

# Copy variants
result.with_error("something failed")   # new result with success=False
result.with_context(count=5, done=True) # new result with updated context
result.to_dict()                        # -> dict
```

#### Skills System

```python
from dcc_mcp_core import (
    SkillScanner,                    # Discovers SKILL.md directories (mtime-cached)
    SkillWatcher,                    # File-watching auto-reload for live development
    SkillMetadata,                   # Parsed skill metadata model
    parse_skill_md,                  # Parse one skill directory → SkillMetadata
    scan_skill_paths,                # Scan + return discovered paths
    scan_and_load,                   # Scan + parse all → List[SkillMetadata]
    scan_and_load_lenient,           # Same, but silently skip errors
    resolve_dependencies,            # Resolve skill dependency graph
    expand_transitive_dependencies,  # Full transitive closure
    validate_dependencies,           # Validate dependency graph is acyclic
)
```

```python
from dcc_mcp_core import (
    # Progressive loading (Skills-First)
    SkillCatalog,                    # Manage discovered vs loaded skills
    SkillSummary,                    # Lightweight skill summary for search results
    create_skill_manager,            # One-call factory: env vars → server
    get_app_skill_paths_from_env,    # Read DCC_MCP_{APP}_SKILL_PATHS + global
    # Bundled skills (zero-config)
    get_bundled_skills_dir,          # Absolute path to dcc_mcp_core/skills/ in wheel
    get_bundled_skill_paths,         # Returns [bundled_dir] or [] (include_bundled=False)
    # SkillMetadata new fields
    ToolDeclaration,                 # Tool declaration with input_schema, annotations
)
```

**Full skills pipeline:**

```python
# ─────────────────────────────────────────────────────────────
# Skills-First (recommended): one-call setup
# Bundled skills (dcc-diagnostics, workflow, git-automation,
# ffmpeg-media, imagemagick-tools) are loaded automatically.
# ─────────────────────────────────────────────────────────────
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"  # or DCC_MCP_SKILL_PATHS

from dcc_mcp_core import create_skill_manager, McpHttpConfig

server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
# Agents connect → search_skills → load_skill → tools/call

# On-demand skill discovery (agent-driven):
# 1. tools/list → 6 core tools + __skill__<name> stubs for every unloaded skill
#    Core: find_skills, list_skills, get_skill_info, load_skill, unload_skill, search_skills
# 2. search_skills(query="modeling") → compact summary: name [status] (N tools: ...) — desc
# 3. load_skill("maya-bevel") → registers tools + handlers, sends tools/list_changed
# 4. tools/list → new skill tools visible with full input schemas
# 5. tools/call maya_bevel__bevel {offset: 0.1} → runs scripts/bevel.py

# ─────────────────────────────────────────────────────────────
# Bundled skills — zero-config, shipped inside the wheel
# ─────────────────────────────────────────────────────────────
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths

print(get_bundled_skills_dir())   # .../site-packages/dcc_mcp_core/skills
paths = get_bundled_skill_paths() # [".../dcc_mcp_core/skills"]  — include in search path
# Opt-out: get_bundled_skill_paths(include_bundled=False) → []

# DCC adapters call this automatically; DCC adapter search-path priority:
# extra_paths > builtin DCC skills > DCC_MCP_{APP}_SKILL_PATHS > DCC_MCP_SKILL_PATHS
# > bundled core skills > platform default skills dir

# ─────────────────────────────────────────────────────────────
# SkillMetadata fields (v0.12.10+)
# ─────────────────────────────────────────────────────────────
# s.name, s.description, s.dcc, s.version, s.tags
# s.search_hint                 # keyword hint for search_skills (SKILL.md search-hint:)
# s.license, s.compatibility    # agentskills.io standard
# s.allowed_tools               # agent permission list (e.g. ["Bash", "Read"])
# s.metadata                    # arbitrary KV + ClawHub openclaw.*
# s.tools (List[ToolDeclaration]) # MCP tool declarations with schemas
# s.scripts (List[str])         # auto-discovered script paths
# s.skill_path, s.depends, s.metadata_files

# SkillSummary fields (from find_skills / list_skills):
# s.name, s.description, s.search_hint, s.version, s.dcc, s.tags
# s.tool_count, s.tool_names, s.loaded

# ─────────────────────────────────────────────────────────────
# Manual setup (advanced / custom handlers)
# ─────────────────────────────────────────────────────────────
paths = get_app_skill_paths_from_env("maya")  # DCC_MCP_MAYA_SKILL_PATHS + DCC_MCP_SKILL_PATHS
skills, _ = scan_and_load_lenient(extra_paths=paths, dcc_name="maya")
```

#### MCP Protocol Types

```python
from dcc_mcp_core import (
    ToolDefinition, ToolAnnotations,
    ResourceDefinition, ResourceAnnotations, ResourceTemplateDefinition,
    PromptDefinition, PromptArgument,
    DccInfo, DccCapabilities, DccError, DccErrorCode,
)

# Build MCP tool definition
tool = ToolDefinition(
    name="create_sphere",
    description="Create a polygon sphere in the DCC scene",
    input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]}',
    annotations=ToolAnnotations(
        title="Create Sphere",
        read_only_hint=False,
        destructive_hint=False,
        idempotent_hint=False,
    ),
)
```

#### Transport Layer

```python
from dcc_mcp_core import (
    TransportManager,    # Connection pool manager
    TransportAddress, TransportScheme, RoutingStrategy,
    IpcListener,         # Server-side IPC listener
    ListenerHandle,      # Handle returned by IpcListener.into_handle()
    FramedChannel,       # Message-framed IPC channel
    connect_ipc,         # Client: connect to IPC server
)

# Server — bind + accept pattern
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)
local_addr = listener.local_address()   # get assigned port
channel = listener.accept()             # blocks until client connects

# Client
channel = connect_ipc(local_addr, timeout_ms=10000)

# FramedChannel RPC — use .call() for synchronous request/reply
result = channel.call("execute_python", b'cmds.sphere()')
# result keys: "id", "success" (bool), "payload" (bytes), "error" (str|None)
if result["success"]:
    print(result["payload"].decode())
else:
    raise RuntimeError(result["error"])

# Low-level: send_request + recv for async/multiplexed patterns
req_id = channel.send_request("execute_python", params=b'cmds.sphere()')
msg = channel.recv(timeout_ms=10000)   # {"type": "response", "id": req_id, ...}

# One-way notifications
channel.send_notify("scene_changed", data=b'{"scene": "shot01.ma"}')

# Heartbeat check
rtt_ms = channel.ping()                 # round-trip time in ms
channel.shutdown()                      # graceful close

# Production: pooled + circuit breaker + auto-registration
mgr = TransportManager("/tmp/dcc-mcp")
instance_id, listener = mgr.bind_and_register("maya", version="2025")
entry = mgr.find_best_service("maya")   # best available Maya instance
session_id = mgr.get_or_create_session("maya", entry.instance_id)
```

#### MCP HTTP Server

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig, McpServerHandle

# Build an action registry
registry = ActionRegistry()
registry.register(
    "get_scene_info",
    description="Get information about the current DCC scene",
    category="scene", tags=["query"], dcc="maya", version="1.0.0",
)

# Start MCP Streamable HTTP server (2025-03-26 spec)
# Runs in background thread — safe to call from DCC main thread
config = McpHttpConfig(
    port=8765,              # use 0 for random available port
    server_name="maya-mcp",
    server_version="1.0.0",
    enable_cors=False,      # set True for browser-based MCP clients
    request_timeout_ms=30000,
)
server = McpHttpServer(registry, config)
handle = server.start()

# MCP host (e.g. Claude Desktop) connects to:
print(handle.mcp_url())    # "http://127.0.0.1:8765/mcp"
print(handle.port)         # actual port (useful when port=0)
print(handle.bind_addr)    # "127.0.0.1:8765"

# Shutdown
handle.shutdown()          # blocks until stopped
# handle.signal_shutdown() # non-blocking alternative

# McpServerHandle is an alias for ServerHandle in __init__.py
from dcc_mcp_core import McpServerHandle  # same type
```

#### Process Management

```python
from dcc_mcp_core import (
    PyDccLauncher,           # Launch DCC processes
    PyProcessMonitor,        # Track running DCC processes
    PyProcessWatcher,        # Auto-restart on crash
    PyCrashRecoveryPolicy,   # Crash recovery configuration
    ScriptResult,            # Result of a script execution
    ScriptLanguage,          # Enum: Python, MEL, MaxScript, etc.
)

launcher = PyDccLauncher(dcc_type="maya", version="2025")
process = launcher.launch(script_path="/startup.py", working_dir="/project")

watcher = PyProcessWatcher(
    recovery_policy=PyCrashRecoveryPolicy(max_restarts=3, cooldown_sec=10)
)
watcher.watch(process)
```

#### Other Modules

```python
# Sandbox security
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator, AuditEntry, AuditLog

# Shared memory (LZ4 compressed)
from dcc_mcp_core import PyBufferPool, PySharedBuffer, PySharedSceneBuffer, PySceneDataKind

# Screen capture
from dcc_mcp_core import Capturer, CaptureFrame, CaptureResult

# Telemetry
from dcc_mcp_core import TelemetryConfig, RecordingGuard, ActionMetrics, ActionRecorder
from dcc_mcp_core import is_telemetry_initialized, shutdown_telemetry

# USD bridge
from dcc_mcp_core import UsdStage, UsdPrim, SdfPath, VtValue, SceneInfo, SceneStatistics
from dcc_mcp_core import scene_info_json_to_stage, stage_to_scene_info_json

# Type wrappers (for RPyC safe serialization)
from dcc_mcp_core import wrap_value, unwrap_value, unwrap_parameters
from dcc_mcp_core import BooleanWrapper, FloatWrapper, IntWrapper, StringWrapper

# Platform utilities
from dcc_mcp_core import (
    get_config_dir, get_data_dir, get_log_dir, get_platform_dir,
    get_actions_dir, get_skills_dir, get_skill_paths_from_env, mpu_to_units, units_to_mpu,
)

# Service registry
from dcc_mcp_core import ServiceEntry, ServiceStatus
```

#### Constants

```python
from dcc_mcp_core import (
    APP_NAME,            # "dcc-mcp"
    APP_AUTHOR,          # "dcc-mcp"
    DEFAULT_DCC,         # "python"
    DEFAULT_LOG_LEVEL,   # "DEBUG"
    DEFAULT_MIME_TYPE,   # "text/plain"
    DEFAULT_VERSION,     # "1.0.0"
    ENV_SKILL_PATHS,     # "DCC_MCP_SKILL_PATHS"
    ENV_LOG_LEVEL,       # "MCP_LOG_LEVEL"
    SKILL_METADATA_FILE, # "SKILL.md"
    SKILL_METADATA_DIR,  # "metadata"
    SKILL_SCRIPTS_DIR,   # "scripts"
)
```

### Legacy APIs (DO NOT USE in new code)

These APIs were removed in v0.12+:
- ~~`ActionManager`~~ → Use `ActionRegistry` + `ActionDispatcher`
- ~~`Action` base class~~ → Actions are registered via `ActionRegistry`
- ~~`Middleware` / `MiddlewareChain`~~ → Use `ActionPipeline` with middleware context
- ~~`create_action_manager()`~~ → Use `ActionRegistry()` directly
- ~~`create_action_manager()`~~ → Use `create_skill_manager(app_name)` (Skills-First) or `ActionRegistry()` directly
- ~~`LoggingMiddleware` / `PerformanceMiddleware`~~ → Use `ActionMetrics` + `EventBus`

## DCC Ecosystem Architecture

> **Key insight**: `dcc-mcp-core` is the **only** dependency a DCC-specific package needs.
> There is no need for `dcc-mcp-ipc` or any other separate IPC library.

### Full Stack (from DCC process to AI agent)

```
┌─────────────────────────────────────────────────────────────────┐
│  MCP Host (Claude Desktop / OpenClaw / any MCP-compatible agent) │
│  Connects via:  http://localhost:8765/mcp  (Streamable HTTP)     │
└────────────────────────────┬────────────────────────────────────┘
                             │ MCP 2025-03-26 Streamable HTTP
┌────────────────────────────▼────────────────────────────────────┐
│  DCC application process (Maya / Blender / Houdini / 3ds Max)   │
│                                                                  │
│  # Python code running inside DCC:                              │
│  from dcc_mcp_core import ActionRegistry, McpHttpServer          │
│  from dcc_mcp_core import McpHttpConfig, DeferredExecutor        │
│                                                                  │
│  registry = ActionRegistry()                                     │
│  # register skills/actions ...                                   │
│                                                                  │
│  server = McpHttpServer(registry, McpHttpConfig(port=8765))      │
│  handle = server.start()   # non-blocking                       │
│  # → handle.mcp_url() == "http://127.0.0.1:8765/mcp"           │
└─────────────────────────────────────────────────────────────────┘
```

### Why `dcc-mcp-ipc` is no longer needed

`dcc-mcp-ipc` was a separate Python project that provided IPC transport between a
DCC process and an external MCP gateway. **Everything it provided is now in `dcc-mcp-core`:**

| dcc-mcp-ipc feature | dcc-mcp-core equivalent |
|---------------------|------------------------|
| IPC Named Pipe / Unix Socket | `IpcListener` + `FramedChannel` in `dcc-mcp-transport` |
| TCP transport | `TransportAddress.tcp()` + `connect_ipc()` |
| Instance routing / load balancing | `InstanceRouter` + `RoutingStrategy` in `dcc-mcp-transport` |
| Service discovery | `ServiceRegistry` + `TransportManager` |
| MCP HTTP gateway | `McpHttpServer` in `dcc-mcp-http` ← **new in v0.12.7** |

With `McpHttpServer`, the DCC process **is** the MCP server — no external gateway needed.

### DCC-specific packages (`dcc-mcp-maya`, `dcc-mcp-blender`, etc.)

Upper-layer DCC packages only need to:

1. **Import `dcc_mcp_core`** — no other dependency
2. **Register DCC-specific actions** via `ActionRegistry.register()` or SKILL.md
3. **Start `McpHttpServer`** — MCP host connects directly
4. **Optionally use `DeferredExecutor`** for main-thread safety (see below)

```python
# dcc-mcp-maya minimal example
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig
import maya.utils  # DCC-specific

registry = ActionRegistry()
# Load skills via DCC_MCP_SKILL_PATHS env var or register manually

server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()
# Claude Desktop connects to http://127.0.0.1:8765/mcp
```

### DCC Main-Thread Safety (`DeferredExecutor`)

All major DCC applications require scene API calls on their **main thread**.
HTTP requests arrive on Tokio worker threads. Use `DeferredExecutor` to bridge:

```python
from dcc_mcp_core._core import DeferredExecutor  # Rust-backed
import maya.utils

# Create executor (on DCC main thread at startup)
executor = DeferredExecutor(queue_depth=64)
dcc_handle = executor.handle()  # cloneable, Send+Sync

# In your DCC event loop / timer callback:
def maya_tick():
    executor.poll_pending()   # runs queued tasks on main thread
    maya.utils.executeDeferred(maya_tick)  # reschedule

maya.utils.executeDeferred(maya_tick)

# Pass dcc_handle to McpHttpServer for thread-safe dispatch:
server = McpHttpServer(registry, config).with_executor(dcc_handle)
handle = server.start()
```

> **When is `DeferredExecutor` needed?**
> - Maya: always (cmds, OpenMaya require main thread)
> - Blender: always (bpy requires main thread)
> - Houdini: most API calls require main thread
> - 3ds Max: most API calls require main thread
> - Testing / non-DCC Python: not needed (omit `.with_executor()`)

## Skills System (Deep Dive)

Skills allow zero-code registration of scripts as MCP tools.

### Directory Layout

```
my-skill/
├── SKILL.md          # Required: YAML frontmatter + markdown description
├── scripts/          # Required: one file per action
│   ├── create_sphere.py
│   ├── batch_rename.mel
│   └── export_fbx.bat
└── metadata/         # Optional
    ├── help.md
    ├── install.md
    └── depends.md    # Dependency declarations (YAML list of skill names)
```

### SKILL.md Format

```yaml
---
name: maya-geometry           # Required: unique identifier (used in action names)
description: "Maya geometry creation and modification tools"
tools: ["Bash", "Read"]       # OpenClaw tool permissions
tags: ["maya", "geometry"]    # Classification tags
dcc: maya                     # Target DCC application
version: "1.0.0"              # Semantic version
depends: ["other-skill"]      # Names of required skills
---
# Human-readable description (markdown body)
```

### Action Naming

Each script in `scripts/` becomes an action named `{skill_name}__{script_stem}`:
- `maya-geometry/scripts/create_sphere.py` → `maya_geometry__create_sphere`
- `maya-geometry/scripts/batch_rename.mel` → `maya_geometry__batch_rename`

Note: hyphens in skill names are replaced by underscores.

### Environment Variable

```bash
# Unix/macOS
export DCC_MCP_SKILL_PATHS="/path/to/skills1:/path/to/skills2"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\skills1;C:\path\skills2
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

See `examples/skills/` for **9 complete examples**: hello-world, maya-geometry, maya-pipeline, git-automation, ffmpeg-media, imagemagick-tools, usd-tools, clawhub-compat, multi-script.

## Adding New Python-Accessible Functions/Classes

When adding a new public API (function or class exposed to Python):

1. **Implement in Rust**: Add to the appropriate `crates/dcc-mcp-*/src/` crate
2. **Add PyO3 bindings**: Create/update the crate's `python.rs` module with `#[pymethods]` / `#[pyclass]`
3. **Register in entry point**: Add to `src/lib.rs` in the corresponding `register_*()` function
4. **Re-export in Python**: Add to `python/dcc_mcp_core/__init__.py` and `__all__`
5. **Update type stubs**: If needed, update `python/dcc_mcp_core/_core.pyi`
6. **Add tests**: Create/update `tests/test_<module>.py`

Example PyO3 binding pattern:

```rust
// crates/dcc-mcp-actions/src/python.rs
use pyo3::prelude::*;

#[pyclass]
pub struct ActionRegistry { /* ... */ }

#[pymethods]
impl ActionRegistry {
    #[new]
    fn new() -> Self { ActionRegistry { /* ... */ } }

    fn register(&self, name: String, description: String) -> PyResult<()> {
        // ...
    }
}

pub fn register_actions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ActionRegistry>()?;
    Ok(())
}
```

## Commit & PR Guidelines

- Use [Conventional Commits](https://www.conventionalcommits.org/)
- Prefixes: `feat:`, `fix:`, `docs:`, `chore:`, `ci:`, `refactor:`, `test:`
- Breaking changes: `feat!:` or add `BREAKING CHANGE:` footer
- **Never manually bump version or edit CHANGELOG.md** — Release Please handles this
- Always run `vx just preflight` before committing
- PR title format: follow Conventional Commits (e.g., `docs: enhance AI agent guidance`)

## Common Pitfalls for AI Agents

1. **Don't import from internal paths**: `dcc_mcp_core.actions.base`, `dcc_mcp_core._core.*` directly — these are implementation details. Always use the public API from `dcc_mcp_core`.

2. **Don't manually bump version numbers** — handled exclusively by Release Please via `release-please-config.json`.

3. **Don't add runtime Python dependencies** — this project has zero runtime deps by design. Everything is in Rust.

4. **Rust changes need Python binding updates**: If you modify a Rust struct exposed via PyO3, update the crate's `python.rs`, register it in `src/lib.rs`, re-export in `__init__.py`, and update `_core.pyi` stubs.

5. **Test with Python bindings feature flag**: `cargo test --workspace --features python-bindings`

6. **Always use `vx` prefix**: `vx just test` not `pytest`, `vx just lint` not `ruff check`.

7. **Don't use legacy APIs**: `ActionManager`, `Action` base class, `create_action_manager()`, `MiddlewareChain` — all removed in v0.12+.

8. **Skills directory convention**: The env var is `DCC_MCP_SKILL_PATHS` (not `SKILL_PATHS`, not `DCC_SKILL_PATHS`).

9. **Action naming**: When building tools on top of this library, use the `{skill_name}__{script_stem}` naming pattern (double underscore separator).

10. **`_core.pyi` is the authoritative stub**: When unsure of parameter names or types, read `python/dcc_mcp_core/_core.pyi` rather than guessing.

11. **`IpcListener.bind(addr)`** creates the listener (not `.new()`); `.accept()` returns a `FramedChannel`. Use `into_handle()` for `ListenerHandle` with tracking.

12. **`FramedChannel.call()` is the primary RPC helper**: `channel.call(method, params_bytes, timeout_ms)` sends a request and waits for the matching response atomically. Use `send_request()` + `recv()` only for multiplexed async patterns.

13. **`McpServerHandle` vs `ServerHandle`**: `server.start()` returns a `ServerHandle`; it is re-exported as `McpServerHandle` in `__init__.py`. Both refer to the same class.

14. **`McpHttpServer` requires an `ActionRegistry`**: The HTTP server reads tool names/descriptions from the registry. Register all actions before calling `server.start()`.

15. **DCC main-thread safety with `McpHttpServer`**: By default, tool handlers run on Tokio worker threads. If your DCC requires main-thread execution (Maya, Blender, Houdini), attach a `DeferredExecutor` via `McpHttpServer.with_executor(handle)` and call `executor.poll_pending()` from your DCC event loop. Omitting this in Maya/Blender **will crash** the DCC.

16. **Do NOT use `dcc-mcp-ipc` in new code**: That project is superseded by `dcc-mcp-core`. All IPC transport, routing, and HTTP serving is provided by this library. New DCC integrations should only depend on `dcc-mcp-core`.

17. **MCP specification version awareness**: `McpHttpServer` implements the 2025-03-26 MCP spec (Streamable HTTP). The 2025-11-05 draft spec introduces JSON-RPC batching, resource links in tool results, and event streams — watch for these capabilities in future `dcc-mcp-core` releases. Do NOT implement these from scratch; wait for API additions to `McpHttpServer`.

18. **`scan_and_load` with `extra_paths`**: When calling `scan_and_load(extra_paths=[...], dcc_name="maya")`, both arguments are keyword-only. Do not pass `extra_paths` as a positional argument — use `extra_paths=["/path"]` explicitly.

## Debugging & Diagnostics

### Build Issues

```bash
# Check all crates compile
vx just check

# Verbose Rust build
cargo build --workspace --features python-bindings 2>&1 | head -50

# Python import check after build
vx just dev
python -c "import dcc_mcp_core; print(dcc_mcp_core.__version__)"
```

### Test Failures

```bash
# Verbose test output
vx just test -- -v -s

# Run a specific test by name
vx just test -- -k "test_scan_and_load" -v

# Check test coverage gaps
vx just test-cov
```

### Type Stub Issues

If Python type checkers report errors about `_core`:
```bash
# Stubs are in python/dcc_mcp_core/_core.pyi
# They're manually maintained — check if your new symbol is listed
grep "SkillMetadata" python/dcc_mcp_core/_core.pyi
```

## CI/CD

- Main branch is protected; all PRs must pass CI
- CI runs: `preflight` → `install` → `test` → `lint-py`
- CI matrix: Python 3.7, 3.9, 3.11, 3.13 on Linux/macOS/Windows
- PyPI publishing uses Trusted Publishing (no tokens needed)
- Documentation deploys to GitHub Pages via VitePress
- `.agents/` directory is gitignored — use `git add -f` for skill files there

## AI Integration Patterns

These patterns show how AI agents should use this library. Copy and adapt them.

### Pattern 1: Skills-First — one call setup (recommended)

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

# Set per-app skill paths (or use global DCC_MCP_SKILL_PATHS)
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/opt/maya-skills"

# One call: creates registry + dispatcher + catalog + discovers skills + server
server = create_skill_manager(
    "maya",                           # app name (used for env var, server name)
    McpHttpConfig(port=8765),         # optional config
    extra_paths=["/extra/skills"],    # optional extra paths
)
handle = server.start()
print(f"Maya MCP server: {handle.mcp_url()}")

# Agents connect and use on-demand skill discovery:
# → search_skills(query="maya") to find relevant skills
# → load_skill("maya-bevel") to activate
# → tools/call maya_bevel__bevel to execute
handle.shutdown()
```

### Pattern 2: Call a DCC action and return structured result

```python
from dcc_mcp_core import (
    connect_ipc, TransportAddress,
    success_result, error_result, from_exception,
)

def call_maya_command(pid: int, python_code: str):
    addr = TransportAddress.default_local("maya", pid)
    channel = connect_ipc(addr)
    try:
        # Primary RPC pattern: .call() handles request/response atomically
        result = channel.call("execute_python", python_code.encode(), timeout_ms=10000)
        if result["success"]:
            payload = result.get("payload", b"")
            decoded = payload.decode() if isinstance(payload, bytes) else str(payload)
            return success_result(
                decoded,
                prompt="Command executed. Check result and decide next step.",
            )
        else:
            return error_result(
                "DCC script failed",
                result.get("error", "Unknown error"),
                possible_solutions=["Check Maya is running", "Verify syntax"],
            )
    except Exception as e:
        return from_exception(str(e), message="IPC call failed", include_traceback=True)
    finally:
        channel.shutdown()
```

### Pattern 3: Validate action inputs before dispatch

```python
import json
from dcc_mcp_core import ActionRegistry, ActionValidator, ActionDispatcher, error_result

schema = json.dumps({
    "type": "object",
    "required": ["name", "radius"],
    "properties": {
        "name": {"type": "string", "maxLength": 64},
        "radius": {"type": "number", "minimum": 0.001, "maximum": 1000.0},
    },
})
validator = ActionValidator.from_schema_json(schema)
ok, errors = validator.validate(json.dumps({"name": "sphere1", "radius": 1.0}))
if not ok:
    return error_result("Invalid parameters", "; ".join(errors))
```

### Pattern 4: Watch skills directory for live development

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/my/dev/skills")  # immediate load + start watching

# In your main loop or callback:
current_skills = watcher.skills()  # always up-to-date snapshot
```

### Pattern 5: Sandbox AI-generated actions

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext, InputValidator

policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "create_sphere"])
policy.deny_actions(["delete_all", "save_to"])
policy.set_timeout_ms(10000)
policy.set_max_actions(20)

ctx = SandboxContext(policy)
ctx.set_actor("ai-assistant")

try:
    result_json = ctx.execute_json("create_sphere", '{"radius": 1.5}')
except RuntimeError as e:
    print(f"Denied: {e}")

# Inspect audit trail
for entry in ctx.audit_log.entries():
    print(f"{entry.action}: {entry.outcome}")
```

### Pattern 6: Expose DCC actions over MCP Streamable HTTP

```python
# ── Approach A: Skills-First (recommended) ──────────────────────────────────
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/opt/maya-skills"

server = create_skill_manager("maya", McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()
print(f"MCP server ready at {handle.mcp_url()}")
# Agents use search_skills/load_skill to discover and activate tools on-demand.
# handle.shutdown() when done


# ── Approach B: Manual setup (custom handlers) ───────────────────────────────
import os
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, McpHttpServer, McpHttpConfig,
    success_result, error_result,
)
from pathlib import Path

os.environ["DCC_MCP_SKILL_PATHS"] = "/opt/maya-skills"

registry = ActionRegistry()
server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))

# Register custom handlers (bypasses script auto-execution)
registry.register("get_scene_info", description="Return current scene info", dcc="maya")
server.register_handler("get_scene_info", lambda params: {"scene": "untitled", "objects": 0})

handle = server.start()
print(f"MCP server ready at {handle.mcp_url()}")
# handle.shutdown() when done
```

### Pattern 7: Thread-safe DCC tool execution (Maya / Blender / Houdini)

```python
"""
DCC main-thread dispatch pattern.

Applicable when the DCC restricts scene API calls to its main thread.
McpHttpServer runs on a Tokio worker thread; DeferredExecutor bridges the gap.
"""
import threading
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig
# DeferredExecutor is a Rust type; import via _core directly for now
from dcc_mcp_core._core import DeferredExecutor

# ── 1. Create executor on main thread ───────────────────────────────────────
executor = DeferredExecutor(queue_depth=64)
dcc_handle = executor.handle()  # cloneable handle for worker threads

# ── 2. Register your DCC actions ───────────────────────────────────────────
registry = ActionRegistry()
registry.register(
    "create_sphere",
    description="Create a polygon sphere on the active layer",
    category="geometry", tags=["create", "mesh"],
    dcc="maya", version="1.0.0",
    input_schema='{"type":"object","properties":{"radius":{"type":"number"}}}'
)

# ── 3. Start HTTP server (non-blocking) ────────────────────────────────────
server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="maya-mcp"))
# NOTE: .with_executor() is implemented in Rust — available when McpHttpServer
# gains executor support. Until then, run tools without DCC-specific APIs.
handle = server.start()
print(f"MCP server at {handle.mcp_url()}")

# ── 4. Poll executor from DCC main thread ──────────────────────────────────
# Maya: maya.utils.executeDeferred(poll)
# Blender: bpy.app.timers.register(poll, persistent=True)
# Houdini: hou.ui.addEventLoopCallback(poll)
def poll():
    executor.poll_pending()

# ── 5. Shutdown ────────────────────────────────────────────────────────────
# handle.shutdown()
```

### Pattern 8: Per-app skill path configuration

```python
import os
from dcc_mcp_core import get_app_skill_paths_from_env, create_skill_manager, McpHttpConfig

# Set paths per DCC — different studios/users can have different skill sets
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya:/home/user/.skills/maya"
os.environ["DCC_MCP_BLENDER_SKILL_PATHS"] = "/studio/blender"
os.environ["DCC_MCP_SKILL_PATHS"] = "/shared/common-skills"  # global fallback

# Query resolved paths for a specific app (per-app + global, deduplicated)
maya_paths = get_app_skill_paths_from_env("maya")
# → ["/studio/maya", "/home/user/.skills/maya", "/shared/common-skills"]

# create_skill_manager automatically picks up the right env vars
maya_server = create_skill_manager("maya", McpHttpConfig(port=8765))
blender_server = create_skill_manager("blender", McpHttpConfig(port=8766))
```

## External References

- [AGENTS.md specification](https://agents.md/) — open standard for AI agent guidance files (Linux Foundation / Agentic AI Foundation)
- [llms.txt specification](https://llmstxt.org/) — AI-optimized documentation format by Answer.AI
- [Model Context Protocol](https://modelcontextprotocol.io/) — the underlying MCP standard (Anthropic)
- [MCP Specification 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26) — current stable spec (Streamable HTTP, OAuth 2.1, Tool Annotations)
- [MCP Specification 2025-11-05 (draft)](https://modelcontextprotocol.io/specification/draft) — upcoming spec (JSON-RPC batching, resource links in tool results, event streams, improved error taxonomy)
- [MCP Agent Skills](https://modelcontextprotocol.io/docs/develop/build-with-agent-skills) — SKILL.md ecosystem and agent skills spec
- [Agent Skills Specification](https://agentskills.io/specification) — official SKILL.md frontmatter format and best practices
- [PyO3 documentation](https://pyo3.rs/) — Rust-Python bindings used in this project
- [maturin documentation](https://www.maturin.rs/) — Python wheel builder used for release
- [OpenClaw Skills format](https://docs.openclaw.ai/tools) — SKILL.md ecosystem compatibility
- [vx tool manager](https://github.com/loonghao/vx) — universal dev tool manager used in this project

## When Stuck

If you are uncertain about how to proceed, follow these steps **in order** — do NOT make large speculative changes:

1. **Read the type stubs first**: `python/dcc_mcp_core/_core.pyi` has authoritative parameter names, types, and docstrings with inline examples.
2. **Read the public API**: `python/dcc_mcp_core/__init__.py` lists every public symbol — if it's not there, it doesn't exist.
3. **Check the tests**: `tests/` directory contains executable usage examples for every major API. Run `vx just test -- -k "test_your_keyword" -v` to see a specific example.
4. **Ask clarifying questions** rather than guessing: e.g., "Does `ActionDispatcher` have a `.call()` method?" — answer: No, use `.dispatch(action_name, params_json)`.
5. **Run `cargo check --workspace`** early when adding Rust code to catch errors before a full build.
6. **Do not** hallucinate method names — every public method is documented in `_core.pyi`.

### Quick Lookup: Common Method Signatures

```python
# ActionDispatcher — only .dispatch(), never .call()
dispatcher = ActionDispatcher(registry)   # takes ONE arg (no validator)
result = dispatcher.dispatch("action_name", json.dumps({"key": "value"}))

# scan_and_load — always returns a 2-TUPLE
skills, skipped = scan_and_load(dcc_name="maya")   # unpack both

# success_result — kwargs become context, NOT "context=" keyword
result = success_result("message", prompt="hint", count=5, name="sphere1")
# result.context == {"count": 5, "name": "sphere1"}

# error_result — positional args (message, error), NOT keyword "message=" / "error="
result = error_result("Failed", "specific error details")

# EventBus.subscribe returns an int ID for unsubscribe
sub_id = bus.subscribe("event_name", handler_fn)
bus.unsubscribe("event_name", sub_id)

# FramedChannel — .call() is the primary RPC method (added v0.12.7)
channel = connect_ipc(TransportAddress.default_local("maya", pid))
result = channel.call("execute_python", b'cmds.sphere()')
# result: {"id": str, "success": bool, "payload": bytes, "error": str|None}
# send_request+recv for async/multiplexed:
req_id = channel.send_request("execute_python", params=b'cmds.sphere()')
msg = channel.recv(timeout_ms=10000)   # {"type": "response", ...}

# McpHttpServer — expose actions over HTTP/MCP
server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()          # returns McpServerHandle (alias: McpServerHandle)
print(handle.mcp_url())          # "http://127.0.0.1:8765/mcp"
handle.shutdown()                # or handle.signal_shutdown() (non-blocking)
```

## PR Checklist

Before opening a PR, verify:

- [ ] `vx just preflight` passes (Rust check + clippy + fmt + test-rust)
- [ ] `vx just test` passes (all Python tests)
- [ ] `vx just lint` passes (clippy + fmt-check + ruff)
- [ ] No new runtime Python dependencies added to `pyproject.toml`
- [ ] If Rust structs changed: updated `python.rs`, `src/lib.rs`, `__init__.py`, `_core.pyi`
- [ ] PR title follows Conventional Commits format (`docs:`, `feat:`, `fix:`, etc.)
- [ ] No manual version bumps (Release Please handles this)
- [ ] `.agents/` skill files added with `git add -f` (they are gitignored)
