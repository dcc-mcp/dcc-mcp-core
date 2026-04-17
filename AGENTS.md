# AGENTS.md — dcc-mcp-core

> **This file is a navigation map, not a reference manual.**
> It tells you *where to look*, not *what every API does*.
> Follow the links; don't read everything upfront.

---

## Start Here — Orient in 60 Seconds

**What is this?**
A Rust-powered MCP (Model Context Protocol) library that lets AI agents interact with DCC software (Maya, Blender, Houdini, Photoshop…). Compiled to a native Python extension via PyO3/maturin. Zero runtime Python dependencies. Implements [MCP 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26) Streamable HTTP transport.

**What does it provide to downstream adapter packages (`dcc-mcp-maya`, `dcc-mcp-blender`, …)?**

| Need | What to use |
|------|-------------|
| Expose DCC tools over MCP HTTP | `DccServerBase` → subclass, call `start()` |
| Zero-code tool registration | Drop `SKILL.md` + `scripts/` in a directory ([agentskills.io](https://agentskills.io/specification) format) |
| AI-safe result structure | `success_result()` / `error_result()` |
| Bridge non-Python DCCs (Photoshop, ZBrush) | `DccBridge` (WebSocket JSON-RPC 2.0) |
| IPC between processes | `IpcListener.bind()` / `connect_ipc()` / `FramedChannel.call()` |
| Multi-DCC gateway | `McpHttpConfig(gateway_port=9765)` |
| Trust-based skill scoping | `SkillScope` (Repo → User → System → Admin) — **Rust-only**; Python uses string values via `SkillMetadata` |
| Progressive tool exposure | `SkillGroup` with `default_active` + `activate_tool_group()` |
| Instance-bound diagnostics | `DccServerBase(..., dcc_pid=pid)` → scoped `diagnostics__*` tools |

**The three files that define the entire public API surface — read them in this order:**

1. `python/dcc_mcp_core/__init__.py` — every public symbol, nothing hidden
2. `python/dcc_mcp_core/_core.pyi` — ground truth for parameter names, types, and signatures
3. `llms.txt` — compressed version of (1)+(2) optimised for token efficiency

---

## Decision Tree — Find the Right API Fast

**Building a DCC adapter (maya, blender, houdini…)?**
→ [`docs/guide/getting-started.md`](docs/guide/getting-started.md)
→ Read: `python/dcc_mcp_core/server_base.py` (DccServerBase — subclass this)
→ Read: `python/dcc_mcp_core/factory.py` (make_start_stop — zero-boilerplate pair)

**Adding tools via SKILL.md (zero Python code)?**
→ [`docs/guide/skills.md`](docs/guide/skills.md)
→ Examples: `examples/skills/` (11 complete packages)

**Writing tool handler Python scripts?**
→ `python/dcc_mcp_core/skill.py` — `@skill_entry`, `skill_success()`, `skill_error()`

**Setting up MCP HTTP server + gateway?**
→ [`docs/api/http.md`](docs/api/http.md)
→ Key types: `McpHttpServer`, `McpHttpConfig`, `McpServerHandle`, `create_skill_server`

**Bridging a non-Python DCC (Photoshop, ZBrush via WebSocket)?**
→ `python/dcc_mcp_core/bridge.py` — `DccBridge`
→ Register with: `BridgeRegistry`, `register_bridge()`, `get_bridge_context()`

**IPC / named pipe / unix socket between processes?**
→ [`docs/api/transport.md`](docs/api/transport.md)
→ Key pattern: `IpcListener.bind(addr)` → `.accept()` | `connect_ipc(addr)` → `channel.call()`

**DCC main-thread safety (Maya cmds, bpy, hou…)?**
→ [`docs/guide/getting-started.md`](docs/guide/getting-started.md) (DeferredExecutor section)
→ `from dcc_mcp_core._core import DeferredExecutor` (not yet in public `__init__`)

**Skills hot-reload during development?**
→ `python/dcc_mcp_core/hotreload.py` — `DccSkillHotReloader`
→ Or directly: `SkillWatcher(debounce_ms=300).watch("/path")`

**Multi-DCC gateway failover (automatic election)?**
→ `python/dcc_mcp_core/gateway_election.py` — `DccGatewayElection`
→ [`docs/guide/gateway-election.md`](docs/guide/gateway-election.md)

**Structured results, input validation, event bus?**
→ [`docs/api/actions.md`](docs/api/actions.md)
→ [`docs/api/models.md`](docs/api/models.md)

**Security, sandbox, audit log?**
→ [`docs/api/sandbox.md`](docs/api/sandbox.md)

**USD scene exchange?**
→ [`docs/api/usd.md`](docs/api/usd.md)

**WebView integration (embedded browser panels)?**
→ `python/dcc_mcp_core/adapters/webview.py` — `WebViewAdapter`, `WebViewContext`
→ Constants: `CAPABILITY_KEYS`, `WEBVIEW_DEFAULT_CAPABILITIES`
→ Note: Currently Python-only, not in `_core.pyi`

**Screen capture, shared memory, telemetry, process management?**
→ `docs/api/capture.md`, `docs/api/shm.md`, `docs/api/telemetry.md`, `docs/api/process.md`

**Capture a single DCC window (not the whole screen)?**
→ `Capturer.new_window_auto()` + `.capture_window(process_id=..., window_title=..., window_handle=...)`
→ Resolve targets first: `WindowFinder().find(CaptureTarget.process_id(pid))` → `WindowInfo`
→ Backend on Windows: HWND `PrintWindow` (falls back to Mock on other OSes)

**Bind diagnostics tools to a specific DCC instance (multi-instance safe)?**
→ `DccServerBase(..., dcc_pid=pid, dcc_window_title=title, dcc_window_handle=hwnd, resolver=...)`
→ Registers `diagnostics__screenshot` / `diagnostics__audit_log` / `diagnostics__tool_metrics` / `diagnostics__process_status`
→ Low-level: `register_diagnostic_mcp_tools(server, dcc_name=..., dcc_pid=...)` BEFORE `server.start()`

**Limit tools surfaced to the LLM client (progressive exposure)?**
→ Declare `groups:` in SKILL.md with `default_active: true|false`
→ Activate at runtime via `ToolRegistry.activate_tool_group(skill, group)` / MCP tool `activate_tool_group`
→ See `docs/guide/skills.md` — "Tool Groups (Progressive Exposure)"

---

## Repo Layout (What Lives Where)

```
dcc-mcp-core/
├── src/lib.rs                      # PyO3 entry point — registers all 14 crates into _core
├── Cargo.toml                      # Workspace: 14 Rust crates
├── pyproject.toml                  # Python package
├── justfile                        # Dev commands (always prefix with vx)
│
├── crates/                         # Rust — one crate per concern
│   ├── dcc-mcp-models/             # ToolResult, SkillMetadata, ToolDeclaration
│   ├── dcc-mcp-actions/            # ToolRegistry, ToolDispatcher, ToolPipeline, EventBus
│   ├── dcc-mcp-skills/             # SkillScanner, SkillCatalog, SkillWatcher
│   ├── dcc-mcp-protocols/          # MCP types: ToolDefinition, DccCapabilities, BridgeKind
│   ├── dcc-mcp-transport/          # IpcListener, FramedChannel, TransportManager, FileRegistry
│   ├── dcc-mcp-process/            # PyDccLauncher, PyProcessWatcher, CrashRecoveryPolicy
│   ├── dcc-mcp-http/               # McpHttpServer (MCP 2025-03-26 Streamable HTTP), Gateway
│   ├── dcc-mcp-sandbox/            # SandboxPolicy, InputValidator, AuditLog
│   ├── dcc-mcp-telemetry/          # TelemetryConfig, ToolRecorder, ToolMetrics
│   ├── dcc-mcp-shm/                # PySharedBuffer, PySharedSceneBuffer (LZ4)
│   ├── dcc-mcp-capture/            # Capturer, CaptureFrame, CaptureTarget, WindowFinder (HWND/DXGI/X11/Mock)
│   ├── dcc-mcp-usd/                # UsdStage, UsdPrim, scene_info_json_to_stage
│   ├── dcc-mcp-server/             # Binary entry point for bridge-mode DCCs
│   └── dcc-mcp-utils/              # Filesystem helpers, wrap_value, constants
│
├── python/dcc_mcp_core/
│   ├── __init__.py                 # ← READ THIS: every public symbol + __all__
│   ├── _core.pyi                   # ← READ THIS: parameter names, types, signatures
│   ├── skill.py                    # Pure-Python: @skill_entry, skill_success/error/warning
│   ├── server_base.py              # Pure-Python: DccServerBase (subclass, supports dcc_pid/dcc_window_title binding)
│   ├── factory.py                  # Pure-Python: make_start_stop, create_dcc_server
│   ├── gateway_election.py         # Pure-Python: DccGatewayElection
│   ├── hotreload.py                # Pure-Python: DccSkillHotReloader
│   ├── bridge.py                   # Pure-Python: DccBridge (WebSocket JSON-RPC 2.0)
│   ├── dcc_server.py               # Pure-Python: register_diagnostic_handlers + register_diagnostic_mcp_tools
│   ├── adapters/                   # Pure-Python: WebViewAdapter, WebViewContext, capabilities
│   └── skills/                     # Bundled: dcc-diagnostics, workflow (in wheel)
│
├── tests/                          # 120+ integration tests — executable usage examples
├── examples/skills/                # 11 complete SKILL.md packages (start here for skill authoring)
│
├── docs/
│   ├── guide/                      # Conceptual guides (getting-started, skills, gateway…)
│   └── api/                        # API reference per module
│
├── llms.txt                        # Compressed API ref for token-limited contexts
└── llms-full.txt                   # Full API ref for LLMs
```

---

## Build & Test — Essential Commands

> All commands require `vx` prefix. Install: https://github.com/loonghao/vx

```bash
vx just dev          # Build dev wheel (run this before any Python tests)
vx just test         # Run all Python integration tests
vx just preflight    # Pre-commit: cargo check + clippy + fmt + test-rust
vx just lint-fix     # Auto-fix all Rust + Python lint issues
vx just test-cov     # Coverage report — find untested paths before adding features
vx just ci           # Full CI pipeline
```

If a symbol appears in `__init__.py` but Python can't import it → run `vx just dev` first.

---

## Traps — Read Before Writing Code

These are the most common mistakes. Each takes less than 10 seconds to check.

**`scan_and_load` returns a 2-tuple — always unpack:**
```python
# ✓
skills, skipped = scan_and_load(dcc_name="maya")
# ✗ iterating gives (list, list), not skill objects
```

**`success_result` / `error_result` — kwargs go into context, not a `context=` kwarg:**
```python
# ✓
result = success_result("done", prompt="hint", count=5)
# result.context == {"count": 5}
```

**`ToolDispatcher` — only `.dispatch()`, never `.call()`:**
```python
dispatcher = ToolDispatcher(registry)          # one arg only
result = dispatcher.dispatch("name", json_str)   # returns dict
```

**`ToolRegistry.register()` — keyword args only, no positional:**
```python
registry.register(name="my_tool", description="...", dcc="maya")
```

**`ToolRegistry` method names still use "action" (v0.13 compatibility):**
```python
# The Rust API was renamed action→tool in v0.13, but some method names
# remain as "action" for backward compatibility:
registry.get_action("create_sphere")           # still "get_action"
registry.list_actions(dcc_name="maya")         # still "list_actions"
registry.search_actions(category="geometry")   # still "search_actions"
# These are NOT bugs — they are compatibility aliases.
```

**`FramedChannel.call()` — primary RPC (v0.12.7+):**
```python
result = channel.call("execute_python", b'cmds.sphere()', timeout_ms=30000)
# result: {"id": str, "success": bool, "payload": bytes, "error": str|None}
```

**`IpcListener` — bind then accept, not new+start:**
```python
listener = IpcListener.bind(addr)       # ✓
channel  = listener.accept()            # blocks until client connects
```

**`DeferredExecutor` — not in public `__init__`:**
```python
from dcc_mcp_core._core import DeferredExecutor   # direct import required
```

**`McpHttpServer` — register ALL handlers BEFORE `.start()`.**
This includes `register_diagnostic_mcp_tools(...)` for instance-bound diagnostics —
register them before calling `server.start()`, never after.

**`Capturer.new_auto()` vs `.new_window_auto()`:**
```python
# ✓ full-screen / display capture (DXGI on Windows, X11 on Linux)
Capturer.new_auto().capture()

# ✓ single-window capture (HWND PrintWindow on Windows; Mock elsewhere)
Capturer.new_window_auto().capture_window(window_title="Maya 2024")
# ✗ .new_auto() then .capture_window() — may return an incorrect backend
```

**Tool groups — inactive groups are hidden, not deleted:**
```python
# default_active=false tools are hidden from tools/list but remain in ToolRegistry.
# Use registry.list_actions() (shows all) vs registry.list_actions_enabled() (active only).
registry.activate_tool_group("maya-geometry", "rigging")   # emits tools/list_changed
```

**`skill_success()` vs `success_result()` — different types, different use cases:**
```python
# Inside a skill script (pure Python, returns dict for subprocess capture):
return skill_success("done", count=5)       # → {"success": True, ...} dict

# Inside server code (returns ToolResult for validation/transport):
return success_result("done", count=5)      # → ToolResult instance
```

**`SkillScope` — higher scope overrides lower for same-name skills:**
```python
# Scope hierarchy: Repo < User < System < Admin
# A System-scoped skill silently shadows a Repo-scoped skill with the same name.
# This prevents project-local skills from hijacking enterprise-managed ones.
# NOTE: SkillScope/SkillPolicy are Rust-level types not exported to Python.
# Access scope info via SkillMetadata: metadata.is_implicit_invocation_allowed(),
# metadata.matches_product(dcc_name). Configure via SKILL.md frontmatter:
#   allow_implicit_invocation: false
#   products: ["maya", "blender"]
```

**`allow_implicit_invocation: false` ≠ `defer-loading: true`:**
```yaml
# allow_implicit_invocation: false → skill must be explicitly load_skill()'d
# defer-loading: true → tool stub appears in tools/list but needs load_skill()
# Both delay tool availability, but the former is a *policy* (security),
# the latter is a *hint* (progressive loading). Use both for maximum control.
```

**MCP security — design tools for safe AI interaction:**
```python
# Use ToolAnnotations to signal safety properties to AI clients:
from dcc_mcp_core import ToolAnnotations
annotations = ToolAnnotations(
    read_only_hint=True,       # tool only reads data, no side effects
    destructive_hint=False,    # tool may cause irreversible changes
    idempotent_hint=True,      # repeated calls produce same result
)
# Design tools around user workflows, not raw API calls.
# Return human-readable errors via error_result("msg", "specific error").
# Use notifications/tools/list_changed when the tool set changes.
```

**`skill_warning()` / `skill_exception()` — additional skill helpers:**
```python
from dcc_mcp_core import skill_warning, skill_exception
# skill_warning() — partial success with warnings (success=True but with caveat)
# skill_exception() — wrap an exception into error dict format
# Both are pure-Python helpers in python/dcc_mcp_core/skill.py
```

---

## Code Style — Non-Negotiable

**Python:**
- `from __future__ import annotations` — first line of every module
- Import order: future → stdlib → third-party → local (with section comments)
- Formatter: `ruff format` (line length 120, double quotes)
- All public APIs: type annotations + Google-style docstrings

**Rust:**
- Edition 2024, MSRV 1.85
- `tracing` for logging (no `println!`)
- `thiserror` for error types
- `parking_lot` instead of `std::sync::Mutex`

---

## Adding a New Public Symbol — Checklist

When adding a Rust type/function that needs to be callable from Python:

1. Implement in `crates/dcc-mcp-*/src/`
2. Add `#[pyclass]` / `#[pymethods]` bindings in the crate's `python.rs`
3. Register in `src/lib.rs` via the appropriate `register_*()` function
4. Re-export in `python/dcc_mcp_core/__init__.py` (import + add to `__all__`)
5. Add stub to `python/dcc_mcp_core/_core.pyi`
6. Add tests in `tests/test_<module>.py`
7. Run `vx just dev` to rebuild, then `vx just test`

---

## CI & Release

- PRs must pass: `vx just preflight` + `vx just test` + `vx just lint`
- CI matrix: Python 3.7, 3.9, 3.11, 3.13 on Linux / macOS / Windows
- Versioning: Release Please (Conventional Commits) — never manually bump
- PyPI: Trusted Publishing (no tokens)

---

## External Standards & Specifications

| What | Where |
|------|-------|
| MCP spec (implemented: 2025-03-26) | https://modelcontextprotocol.io/specification/2025-03-26 |
| SKILL.md format | https://agentskills.io/specification |
| AGENTS.md standard | https://agents.md/ |
| llms.txt format | https://llmstxt.org/ |
| PyO3 (Rust→Python bindings) | https://pyo3.rs/ |
| maturin (wheel builder) | https://www.maturin.rs/ |
| vx (tool manager) | https://github.com/loonghao/vx |

> MCP spec note: Library implements 2025-03-26. Later specs (2025-06-18, 2025-11-25) add
> Structured Tool Output, Elicitation, Resource Links, icon metadata, Tasks. Do NOT
> implement these manually — wait for library support.

---

## LLM-Specific Guides

- `CLAUDE.md` — Claude Code workflows and tips
- `GEMINI.md` — Gemini-specific guidance
- `llms.txt` — token-optimised API reference
- `llms-full.txt` — complete API reference
