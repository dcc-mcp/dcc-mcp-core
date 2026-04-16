# AGENTS.md — dcc-mcp-core

> **This file is a navigation map, not a reference manual.**
> It tells you *where to look*, not *what every API does*.
> Follow the links; don't read everything upfront.

---

## Start Here — Orient in 60 Seconds

**What is this?**
A Rust-powered MCP (Model Context Protocol) library that lets AI agents interact with DCC software (Maya, Blender, Houdini, Photoshop…). Compiled to a native Python extension via PyO3/maturin. Zero runtime Python dependencies.

**What does it provide to downstream adapter packages (`dcc-mcp-maya`, `dcc-mcp-blender`, …)?**

| Need | What to use |
|------|-------------|
| Expose DCC tools over MCP HTTP | `DccServerBase` → subclass, call `start()` |
| Zero-code tool registration | Drop `SKILL.md` + `scripts/` in a directory |
| AI-safe result structure | `success_result()` / `error_result()` |
| Bridge non-Python DCCs (Photoshop, ZBrush) | `DccBridge` (WebSocket JSON-RPC 2.0) |
| IPC between processes | `IpcListener.bind()` / `connect_ipc()` / `FramedChannel.call()` |
| Multi-DCC gateway | `McpHttpConfig(gateway_port=9765)` |

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

**Screen capture, shared memory, telemetry, process management?**
→ `docs/api/capture.md`, `docs/api/shm.md`, `docs/api/telemetry.md`, `docs/api/process.md`

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
│   ├── dcc-mcp-capture/            # Capturer, CaptureFrame (platform-native)
│   ├── dcc-mcp-usd/                # UsdStage, UsdPrim, scene_info_json_to_stage
│   ├── dcc-mcp-server/             # Binary entry point for bridge-mode DCCs
│   └── dcc-mcp-utils/              # Filesystem helpers, wrap_value, constants
│
├── python/dcc_mcp_core/
│   ├── __init__.py                 # ← READ THIS: every public symbol + __all__
│   ├── _core.pyi                   # ← READ THIS: parameter names, types, signatures
│   ├── skill.py                    # Pure-Python: @skill_entry, skill_success/error/warning
│   ├── server_base.py              # Pure-Python: DccServerBase (subclass for adapters)
│   ├── factory.py                  # Pure-Python: make_start_stop, create_dcc_server
│   ├── gateway_election.py         # Pure-Python: DccGatewayElection
│   ├── hotreload.py                # Pure-Python: DccSkillHotReloader
│   ├── bridge.py                   # Pure-Python: DccBridge (WebSocket JSON-RPC 2.0)
│   ├── dcc_server.py               # Pure-Python: register_diagnostic_handlers
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
registry.register(name="my_action", description="...", dcc="maya")
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

**`skill_success()` vs `success_result()` — different types, different use cases:**
```python
# Inside a skill script (pure Python, returns dict for subprocess capture):
return skill_success("done", count=5)       # → {"success": True, ...} dict

# Inside server code (returns ToolResult for validation/transport):
return success_result("done", count=5)      # → ToolResult instance
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
