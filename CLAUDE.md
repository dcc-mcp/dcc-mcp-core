# CLAUDE.md — dcc-mcp-core Instructions for Claude

> **Purpose**: Claude-specific instructions. **Read `AGENTS.md` first** for full project context,
> architecture, commands, and pitfalls. This file adds only Claude-specific guidance.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC (Digital Content Creation) applications. The Python package name is `dcc_mcp_core`.

## Response Language

- Reply to the user in **Simplified Chinese** (中文简体) by default.
- Keep all code, identifiers, commit messages, branch names, docstrings,
  comments, and file contents in **English** — this rule governs only the
  conversational/assistant-facing output, not anything written to disk or
  pushed to git.
- If the user explicitly requests another language for a specific reply,
  follow that request for that turn.

## Document Hierarchy (Progressive Disclosure)

When you need information, read in this order — stop when you find what you need:

1. **`AGENTS.md`** — Navigation map: where to find everything, traps, Do/Don't
2. **`llms.txt`** — Compressed API reference for AI agents (token-efficient)
3. **`python/dcc_mcp_core/__init__.py`** — Complete public API surface (~177 symbols)
4. **`python/dcc_mcp_core/_core.pyi`** — Parameter names, types, signatures
5. **`llms-full.txt`** — Complete API reference with examples (when `llms.txt` lacks detail)
6. **`docs/guide/`** + **`docs/api/`** — Conceptual guides and per-module API docs
7. **`tests/`** — 120+ usage examples in test form

## Claude-Specific Workflows

### When Adding a New Python-Accessible Symbol

1. Implement in the appropriate `crates/dcc-mcp-*/src/` Rust crate
2. Add PyO3 bindings in the crate's `python.rs` module (`#[pyclass]` / `#[pymethods]`)
3. Register in `src/lib.rs` in the corresponding `register_*()` function
4. Re-export in `python/dcc_mcp_core/__init__.py` (both import and `__all__`)
5. Update `python/dcc_mcp_core/_core.pyi` stubs
6. Add pytest tests in `tests/test_<module>.py`

### When Working With Skills

- Skills are discovered via `SKILL.md` files in directories listed in `DCC_MCP_SKILL_PATHS`
- Each skill's scripts become automatically registered actions
- Action naming: `{skill_name}__{script_stem}` (double underscore, hyphens→underscores)
- Use `scan_and_load()` or `scan_and_load_lenient()` — not the old `scan_and_load_skills()`
- **`scan_and_load` returns a 2-tuple**: `(List[SkillMetadata], List[str])` — always unpack both
- See `examples/skills/` for 11 reference implementations
- **`search-hint` in SKILL.md**: add `search-hint: "keyword1, keyword2"` to improve `search_skills` matching without loading full schemas
- **On-demand discovery**: `tools/list` returns skill stubs (`__skill__<name>`) for unloaded skills; use `search_skills(query)` then `load_skill(name)` to activate
- **Bundled skills**: 2 core skills shipped inside the wheel (`dcc_mcp_core/skills/`):
  `dcc-diagnostics`, `workflow`
  — use `get_bundled_skills_dir()` / `get_bundled_skill_paths()` to get the path.
  DCC adapters include these by default (`include_bundled=True`).
- **Skill authoring templates**: `skills/templates/` provides three starter templates:
  - `minimal` — 1 tool, 1 script (simplest possible skill)
  - `dcc-specific` — DCC binding + required_capabilities + next-tools chaining
  - `with-groups` — tool groups for progressive exposure
  See `skills/README.md` for quick-start guide.
- **DCC integration architectures**: `skills/integration-guide.md` covers three patterns:
  - **Embedded Python** (`DccServerBase`) — Maya, Blender, Houdini, Unreal
  - **WebSocket Bridge** (`DccBridge`) — Photoshop, ZBrush, Unity, After Effects
  - **WebView Host** (`WebViewAdapter`) — AuroraView, Electron panels
- **SKILL.md frontmatter fields**: agentskills.io standard (`name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`) + dcc-mcp-core extensions (`dcc`, `tags`, `search-hint`, `tools`, `groups`, `depends`, `next-tools`)
- **`next-tools`**: Per-tool field guiding AI agents to follow-up tools (`on-success` / `on-failure`). dcc-mcp-core extension, not in agentskills.io spec.
- **`allowed-tools`**: Experimental agentskills.io field — space-separated pre-approved tool strings (e.g. `Bash(git:*) Read`)

```python
# Correct usage:
skills, skipped = scan_and_load(dcc_name="maya")
# NOT: skills = scan_and_load(dcc_name="maya")  ← returns tuple, iterating gives wrong results

# Bundled skills (zero-config):
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths
paths = get_bundled_skill_paths()              # [".../dcc_mcp_core/skills"]
paths = get_bundled_skill_paths(False)         # [] — opt-out
```

### When Understanding the Transport Layer

- Uses IPC (Unix socket / named pipe) for process communication
- `TransportManager` manages connection pools with `CircuitBreaker` resilience
- `FramedChannel` for reliable message delivery with message framing
- Connect (client): `connect_ipc(address, timeout_ms=10000) -> FramedChannel`
- Listen (server): `IpcListener.bind(address)` → `.accept(timeout_ms=None) -> FramedChannel`
  - Note: the method is `.bind()` (static) + `.accept()` (blocking) — not `.new()` + `.start()`
- **`FramedChannel.call(method, params, timeout_ms)` — primary RPC helper** (added v0.12.7):
  sends a Request and waits for the correlated Response atomically.
  - `result = channel.call("execute_python", b'cmds.sphere()')` → `{"id", "success", "payload", "error"}`
  - Use `send_request()` + `recv()` only when you need async/multiplexed patterns

### When Using MCP HTTP Server

```python
# Skills-First (recommended)
from dcc_mcp_core import create_skill_server, McpHttpConfig
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())  # "http://127.0.0.1:8765/mcp"
# tools/list returns 6 core tools + __skill__<name> stubs; search_skills → load_skill → use

# Manual registry wiring (low-level)
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig, McpServerHandle

registry = ToolRegistry()
registry.register("get_scene", description="Get scene", category="scene", dcc="maya")

server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()   # returns McpServerHandle (alias for McpServerHandle)
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"
handle.shutdown()
# Note: register ALL actions BEFORE calling server.start()
```

### Quick Lookup: Common Method Signatures

```python
# ToolDispatcher — only .dispatch(), never .call()
dispatcher = ToolDispatcher(registry)   # takes ONE arg; no validator param
result = dispatcher.dispatch("action_name", json.dumps({"key": "value"}))
# result keys: "action", "output", "validation_skipped"

# scan_and_load — ALWAYS returns a 2-TUPLE
skills, skipped = scan_and_load(dcc_name="maya")   # never: skills = scan_and_load(...)

# success_result — extra kwargs go into context, NOT "context=" keyword arg
result = success_result("message", prompt="hint", count=5)
# result.context == {"count": 5}

# error_result — positional args
result = error_result("Failed", "specific error string")

# EventBus.subscribe returns int ID
sub_id = bus.subscribe("event_name", handler_fn)
bus.unsubscribe("event_name", sub_id)

# ToolRegistry.register — takes keyword args, NOT handler=
registry.register(name="action", description="...", dcc="maya", version="1.0.0")
# Use dispatcher.register_handler() to attach a Python callable

# FramedChannel.call() — primary RPC helper (v0.12.7+)
channel = connect_ipc(TransportAddress.default_local("maya", pid))
result = channel.call("execute_python", b'cmds.sphere()', timeout_ms=30000)
# result: {"id": str, "success": bool, "payload": bytes, "error": str|None}
# Alternative (async): req_id = channel.send_request(...); msg = channel.recv(timeout_ms=...)

# McpHttpServer — expose registry over HTTP/MCP
server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()   # McpServerHandle
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"
```

### When Exploring Unknown Symbols

```bash
# Check what's available in the public API
grep -n "from dcc_mcp_core._core import" python/dcc_mcp_core/__init__.py

# Find parameter signatures
grep -A5 "class SkillMetadata" python/dcc_mcp_core/_core.pyi

# Find Rust implementation
grep -rn "SkillMetadata" crates/ --include="*.rs" | grep "pub struct\|pyclass"
```

### When Debugging Build/Import Issues

```bash
# Rebuild dev wheel
vx just dev

# Verify import works
python -c "import dcc_mcp_core; print(dir(dcc_mcp_core))"

# Check for PyO3 registration gaps (symbol in Rust but missing from Python)
python -c "import dcc_mcp_core; print(hasattr(dcc_mcp_core, 'MyNewSymbol'))"

# Verbose cargo build
cargo build --workspace --features python-bindings 2>&1 | grep -E "error|warning" | head -30
```

## Claude-Specific Tips

- **Prefer reading `__init__.py`** over guessing imports — it has the complete public API surface
- **`_core.pyi` is the ground truth** for parameter names and types
- **For large refactors**, use `cargo check --workspace` early to catch errors before building the full wheel
- **The `justfile` is cross-platform**: recipes work on both Windows PowerShell and Unix sh
- **When debugging Python-Rust binding issues**: check that the symbol is registered in `src/lib.rs` AND re-exported in `__init__.py` AND listed in `_core.pyi`
- **Use `vx just test-cov`** to see coverage gaps before adding new features
- **Don't use legacy APIs**: `ActionManager`, `create_action_manager()`, `MiddlewareChain`, `Action` base class — all removed in v0.12+. Note: `LoggingMiddleware` IS still available (use via `pipeline.add_logging()`).
- **The project has zero runtime Python dependencies by design** — never add `dependencies = [...]` to `pyproject.toml`
- **`DeferredExecutor` is not in public `__init__.py`**: import via `from dcc_mcp_core._core import DeferredExecutor` until it is promoted to the public API
- **`CompatibilityRouter` is not a standalone Python class**: access via `VersionedRegistry.router()` — it borrows the registry for constraint-based version resolution
- **`external_deps` on SkillMetadata**: a JSON string field for declaring external requirements (MCP servers, env vars, binaries). Set via `md.external_deps = json.dumps(deps)`, read via `json.loads(md.external_deps)`. Returns `None` if not set.
- **MCP spec**: `McpHttpServer` implements 2025-03-26 spec. The 2026 roadmap focuses on: (1) transport scalability — `.well-known` capability discovery, stateless session model; (2) agent communication — Tasks lifecycle (experimental), retry/expiration semantics; (3) governance — contributor ladder, delegated workgroups; (4) enterprise readiness — audit, SSO, gateway behavior (mostly extensions, not core spec changes). No new transport types in 2026 — only Streamable HTTP evolution. Do NOT implement these manually — wait for the library to add support.
- **Bridge system**: `BridgeRegistry`, `BridgeContext`, `register_bridge()`, `get_bridge_context()` — for inter-protocol bridging (RPyC ↔ MCP etc.). Don't build custom bridge registries.
- **Scene data model**: `BoundingBox`, `FrameRange`, `ObjectTransform`, `SceneNode`, `SceneObject`, `RenderOutput` — use for structured scene data instead of raw dicts. `BoundingBox` may be `None`.
- **Serialization**: `serialize_result()` / `deserialize_result()` with `SerializeFormat` enum — for transport-safe ToolResult serialization. Don't use `json.dumps()` on ToolResult.
- **SkillScope & SkillPolicy** (v0.13+): Trust hierarchy (`Repo` < `User` < `System` < `Admin`) — higher scopes shadow lower for same-name skills. **These are Rust-level types not directly importable from Python.** Configure via SKILL.md frontmatter (`allow_implicit_invocation`, `products`) and access via `SkillMetadata.is_implicit_invocation_allowed()` / `SkillMetadata.matches_product(dcc_name)`.
- **WebViewAdapter** (Python-only): `from dcc_mcp_core import WebViewAdapter, WebViewContext, CAPABILITY_KEYS, WEBVIEW_DEFAULT_CAPABILITIES` — for embedding browser panels in DCC applications. Not in `_core.pyi`.
- **`skill_warning()` / `skill_exception()`**: Pure-Python helpers in `skill.py`. `skill_warning()` returns a partial-success dict with warnings; `skill_exception()` wraps exceptions into error dict format.
- **Action→Tool rename** (v0.13): Conceptual rename complete; some Rust API method names (`get_action`, `list_actions`, `search_actions`) remain as compatibility aliases — not bugs.
- **MCP best practices**: Design tools around user workflows, not raw API calls. Use `ToolAnnotations` for safety hints (`read_only_hint`, `destructive_hint`, `idempotent_hint`). Return human-readable errors.
- **Security**: Use `SandboxPolicy` + `SandboxContext` for AI-driven tool execution. Validate inputs with `ToolValidator`. Never hardcode secrets.
- **Commit messages**: Use Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`). Never manually bump versions — Release Please manages this.

## AI Agent Tool Priority

When building tools or interacting with DCCs, follow this priority order:

1. **Skill Discovery** (start here): `search_skills(query)` → `load_skill(name)` → use skill tools
2. **Skill-Based Tools** (preferred): Tools with validated schemas, error handling, `next-tools` guidance, and `ToolAnnotations` safety hints
3. **Diagnostics Tools** (for verification): `diagnostics__screenshot`, `diagnostics__audit_log`, `diagnostics__process_status`
4. **Direct Registry Access** (last resort): Only when no skill tool covers the operation; must validate with `ToolValidator` and sandbox with `SandboxPolicy`

**Why skills first?** Safety (annotations), discoverability (search-hint), chainability (next-tools), progressive exposure (tool groups), validation (input_schema).
