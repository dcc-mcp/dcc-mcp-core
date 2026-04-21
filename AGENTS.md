# AGENTS.md — dcc-mcp-core

> **This file is a navigation map, not a reference manual.**
> It tells you *where to look*, not *what every API does*.
> Follow the links; don't read everything upfront.
>
> **Document hierarchy** (progressive disclosure — read only what you need):
>
> | Layer | File | What it gives you | When to read it |
> |-------|------|-------------------|-----------------|
> | 🗺️ Navigation | `AGENTS.md` (this file) | Where to find everything | First contact with the project |
> | ⚡ AI-friendly index | `llms.txt` | Compressed API reference optimised for token efficiency | When an AI agent needs to *use* the APIs |
> | 📖 Full index | `llms-full.txt` | Complete API reference with copy-paste examples | When `llms.txt` lacks detail |
> | 📚 Human docs | `docs/guide/` + `docs/api/` | Conceptual guides and per-module API docs | When building a new adapter or skill |
> | 🔧 LLM-specific | `CLAUDE.md` / `GEMINI.md` / `CODEBUDDY.md` | Agent-specific workflows and tips | When using Claude Code, Gemini CLI, or CodeBuddy Code |
> | 🧩 Skill authoring | `skills/README.md` + `examples/skills/` | Templates, examples, SKILL.md format | When creating or modifying skills |

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
| IPC between processes | `IpcChannelAdapter` / `SocketServerAdapter` + `DccLinkFrame` |
| Multi-DCC gateway | `McpHttpConfig(gateway_port=9765)` |
| Trust-based skill scoping | `SkillScope` (Repo → User → System → Admin) — **Rust-only**; Python uses string values via `SkillMetadata` |
| Progressive tool exposure | `SkillGroup` with `default_active` + `activate_tool_group()` |
| Instance-bound diagnostics | `DccServerBase(..., dcc_pid=pid)` → scoped `diagnostics__*` tools |

**The three files that define the entire public API surface — read them in this order:**

1. `python/dcc_mcp_core/__init__.py` — every public symbol, nothing hidden
2. `python/dcc_mcp_core/_core.pyi` — ground truth for parameter names, types, and signatures
3. `llms.txt` — compressed version of (1)+(2) optimised for token efficiency

---

## AI Agent Tool Priority — Start Here

When an AI agent needs to interact with DCC software, follow this priority order:

### 1. Skill Discovery (always start here)
```
search_skills(query="...") → find relevant skills
load_skill(skill_name="...") → register tools
tools/list → see available tools
```

### 2. Skill-Based Tools (preferred over raw API calls)
- Use skill tools (e.g. `maya_geometry__create_sphere`) — they have validated schemas, error handling, and `next-tools` guidance
- Check `ToolAnnotations` for safety hints before calling destructive tools
- Use `next-tools` from tool results to chain follow-up actions

### 3. Diagnostics Tools (for debugging/verification)
```
diagnostics__screenshot → verify visual state
diagnostics__audit_log → check execution history
diagnostics__tool_metrics → measure performance
diagnostics__process_status → check DCC process health
```

### 4. Direct Registry Access (last resort)
- Only when no skill tool covers the needed operation
- Must validate inputs with `ToolValidator` before execution
- Must use `SandboxPolicy` for AI-initiated calls

### Decision Tree
```
Need to interact with DCC?
├── Know the skill? → load_skill(name) → use tool
├── Don't know? → search_skills(query) → load_skill → use tool
├── Need to verify? → diagnostics__screenshot / process_status
└── No skill exists? → register custom tool with ToolRegistry
```

### Why Skills First?
1. **Safety**: Skills declare `ToolAnnotations` — agents can check `destructive_hint`, `read_only_hint`
2. **Discoverability**: `search_skills` + `search-hint` keywords find the right tool without trial-and-error
3. **Chainability**: `next-tools` guides follow-up actions, reducing hallucination
4. **Progressive exposure**: Tool groups keep `tools/list` small — agents activate only what they need
5. **Validation**: Skill tools have `input_schema` — parameters are validated before execution

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

**Exposing live DCC state (scene, window capture, audit log) to MCP clients?**
→ [`docs/api/resources.md`](docs/api/resources.md) — Resources primitive (#350)
→ Config: `McpHttpConfig.enable_resources` (default `True`), `.enable_artefact_resources` (default `False`)
→ Built-ins: `scene://current`, `capture://current_window`, `audit://recent`, `artefact://<id>` (stub)
→ Rust wiring: `server.resources().set_scene(...)` / `.wire_audit_log(...)` / `.add_producer(...)` before `start()`

**Bridging a non-Python DCC (Photoshop, ZBrush via WebSocket)?**
→ `python/dcc_mcp_core/bridge.py` — `DccBridge`
→ Register with: `BridgeRegistry`, `register_bridge()`, `get_bridge_context()`
→ Full examples: [`skills/integration-guide.md`](skills/integration-guide.md) (Photoshop UXP, Unity C#, ZBrush HTTP)

**IPC / named pipe / unix socket between processes?**
→ [`docs/api/transport.md`](docs/api/transport.md)
→ Key pattern: `IpcChannelAdapter.create(name)` → `.wait_for_client()` | `IpcChannelAdapter.connect(name)` → `.send_frame()` / `.recv_frame()`
→ Frame type: `DccLinkFrame(msg_type, seq, body)`

**DCC main-thread safety (Maya cmds, bpy, hou…)?**
→ [`docs/guide/dcc-thread-safety.md`](docs/guide/dcc-thread-safety.md) — full guide (chunking, forbidden patterns, per-DCC defer primitives)
→ [`docs/adr/002-dcc-main-thread-affinity.md`](docs/adr/002-dcc-main-thread-affinity.md) — architectural rationale
→ [`docs/guide/getting-started.md`](docs/guide/getting-started.md) (DeferredExecutor section) — minimal example
→ `from dcc_mcp_core._core import DeferredExecutor` (not yet in public `__init__`)

### Thread Safety (quick rules — see `docs/guide/dcc-thread-safety.md`)

- All scene-mutating calls go through `DeferredExecutor` — never call `maya.cmds` / `bpy.ops` / `hou.*` / `pymxs.runtime` from a Tokio worker or `threading.Thread`.
- Pump the queue via `poll_pending_bounded(max=8)` from the DCC's defer primitive (`maya.utils.executeDeferred`, `bpy.app.timers.register`, `hou.ui.addEventLoopCallback`). Never `poll_pending()` in production — it drains unboundedly and freezes the UI under bursts.
- Long-running jobs must be chunked into per-tick units with cooperative checkpoints (see #329 `check_cancelled()`, #332 `@chunked_job`).
- Forbidden inside a `DccTaskFn`: `time.sleep`, spawning OS threads for scene ops, blocking I/O (`requests.get`, sync DB, large file reads). Do I/O on the Tokio worker, then defer only the scene call.
- Source of truth: `crates/dcc-mcp-http/src/executor.rs` (`DeferredExecutor`), `crates/dcc-mcp-process/src/dispatcher.rs` (`ThreadAffinity`, `JobRequest`, `HostDispatcher`).

**Skills hot-reload during development?**
→ `python/dcc_mcp_core/hotreload.py` — `DccSkillHotReloader`
→ Or directly: `SkillWatcher(debounce_ms=300).watch("/path")`

**Multi-DCC gateway failover (automatic election)?**
→ `python/dcc_mcp_core/gateway_election.py` — `DccGatewayElection`
→ [`docs/guide/gateway-election.md`](docs/guide/gateway-election.md)

**Enable durable rolling file logs (multi-gateway debugging)?**
→ `FileLoggingConfig` + `init_file_logging()` / `shutdown_file_logging()`
→ Environment vars: `DCC_MCP_LOG_DIR`, `DCC_MCP_LOG_MAX_SIZE`, `DCC_MCP_LOG_ROTATION`

**Deploying `dcc-mcp-server` to production (Docker, systemd, k8s, LB)?**
→ [`docs/guide/production-deployment.md`](docs/guide/production-deployment.md)
→ Artifacts: [`examples/compose/gateway-ha/`](examples/compose/gateway-ha/), [`examples/k8s/gateway-ha/`](examples/k8s/gateway-ha/), [`examples/systemd/`](examples/systemd/)

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
→ Full examples: [`skills/integration-guide.md`](skills/integration-guide.md) (AuroraView, Electron, capabilities model)
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

**Validate tool names or action IDs (SEP-986)?**
→ [`docs/guide/naming.md`](docs/guide/naming.md)
→ `validate_tool_name(name)` / `validate_action_id(name)` — raise `ValueError` on invalid names
→ Constants: `TOOL_NAME_RE`, `ACTION_ID_RE`, `MAX_TOOL_NAME_LEN`

---

## Repo Layout (What Lives Where)

```
dcc-mcp-core/
├── src/lib.rs                      # PyO3 entry point — registers all 15 crates into _core
├── Cargo.toml                      # Workspace: 15 Rust crates
├── pyproject.toml                  # Python package
├── justfile                        # Dev commands (always prefix with vx)
│
├── crates/                         # Rust — one crate per concern
│   ├── dcc-mcp-naming/             # SEP-986 tool-name / action-id validators (TOOL_NAME_RE, validate_tool_name)
│   ├── dcc-mcp-models/             # ToolResult, SkillMetadata, ToolDeclaration
│   ├── dcc-mcp-actions/            # ToolRegistry, ToolDispatcher, ToolPipeline, EventBus
│   ├── dcc-mcp-skills/             # SkillScanner, SkillCatalog, SkillWatcher
│   ├── dcc-mcp-protocols/          # MCP types: ToolDefinition, DccCapabilities, BridgeKind
│   ├── dcc-mcp-transport/          # DccLink adapters (ipckit), FileRegistry (discovery)
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

**SKILL.md sibling-file pattern — THE rule for every new extension (v0.15+ / #356):**

Do **not** add new top-level frontmatter keys to `SKILL.md`. agentskills.io
1.0 only allows `name`, `description`, `license`, `compatibility`,
`metadata`, `allowed-tools` at the top level. Every dcc-mcp-core
extension — `tools`, `groups`, `workflows`, `prompts`, behaviour
chains, annotations, templates, examples packs, anything future —
MUST be expressed as:

1. A **namespaced key under `metadata:`** using the `dcc-mcp.<feature>` convention.
2. The key's **value is a glob or filename** pointing at a sibling
   file (YAML or Markdown) that carries the actual payload.
3. The sibling file lives **inside the skill directory**, not
   inline in `SKILL.md`.

```yaml
---
name: maya-animation
description: >-
  Maya animation keyframes, timeline, curves. Use when the user asks to
  set/query keyframes, change timeline range, or bake simulations.
license: MIT
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: "tools.yaml"              # ✓ points at sibling
  dcc-mcp.groups: "tools.yaml"             # ✓ same or separate file
  dcc-mcp.workflows: "workflows/*.workflow.yaml"
  dcc-mcp.prompts: "prompts/*.prompt.yaml"
  dcc-mcp.examples: "references/EXAMPLES.md"
---
# body — human-readable instructions only
```

```
maya-animation/
├── SKILL.md                    # metadata map + body
├── tools.yaml                  # tools + groups
├── workflows/
│   ├── vendor_intake.workflow.yaml
│   └── nightly_cleanup.workflow.yaml
├── prompts/
│   └── review_scene.prompt.yaml
└── references/
    └── EXAMPLES.md
```

Why this is non-negotiable:

- **`skills-ref validate` passes** — no custom top-level fields.
- **Progressive disclosure** — agents only pay tokens for the sibling
  files they actually need; a 60-tool skill stays cheap to index.
- **Diffable** — one PR per workflow/prompt file, not buried in a
  monster SKILL.md block.
- **Forward-compatible** — future extensions add a new
  `metadata.dcc-mcp.<x>` key and a new sibling schema, without
  re-negotiating the frontmatter spec.

When you design a new feature that touches SKILL.md, the design review
gate is: "Can this live as a `metadata.dcc-mcp.<feature>` pointer to
sibling files?" If the answer is no, bring it to a proposal before
implementing (see `docs/proposals/`).

**`ToolRegistry` method names still use "action" (v0.13 compatibility):**
```python
# The Rust API was renamed action→tool in v0.13, but some method names
# remain as "action" for backward compatibility:
registry.get_action("create_sphere")           # still "get_action"
registry.list_actions(dcc_name="maya")         # still "list_actions"
registry.search_actions(category="geometry")   # still "search_actions"
# These are NOT bugs — they are compatibility aliases.
```

**DccLink IPC — primary RPC path (v0.14+, issue #251):**
```python
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter
channel = IpcChannelAdapter.connect("dcc-mcp-maya-12345")  # Named Pipe / UDS
channel.send_frame(DccLinkFrame(msg_type="Call", seq=1, body=b"{...}"))
reply = channel.recv_frame()   # DccLinkFrame: msg_type, seq, body
# Legacy FramedChannel.call / connect_ipc were REMOVED in v0.14 (#251).
```

**Multi-client IPC server:**
```python
from dcc_mcp_core import SocketServerAdapter
server = SocketServerAdapter("/tmp/maya.sock", max_connections=8,
                             connection_timeout_secs=30)
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
    open_world_hint=False,     # tool may interact with external systems
    deferred_hint=None,        # full schema deferred until load_skill (set by server, not user)
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

**`next-tools` in SKILL.md — guide AI to follow-up tools:**
```yaml
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya_geometry__bevel_edges]   # suggested after success
      on-failure: [dcc_diagnostics__screenshot]   # debug on failure
```
- `next-tools` is a dcc-mcp-core extension (not in agentskills.io spec)
- Helps AI agents chain tool calls without trial-and-error
- Both `on-success` and `on-failure` accept lists of fully-qualified tool names

**agentskills.io fields — `license`, `compatibility`, `allowed-tools`:**
```yaml
---
name: my-skill
description: "Does X. Use when user asks to Y."
license: MIT                          # optional — SPDX identifier or file reference
compatibility: "Maya 2024+, Python 3.7+"  # optional — environment requirements
allowed-tools: Bash(git:*) Read       # optional — pre-approved tools (experimental)
---
```
- `license` and `compatibility` are parsed into `SkillMetadata` fields
- `allowed-tools` is experimental in agentskills.io spec — space-separated tool strings
- Most skills don't need `compatibility`; only include it when there are hard requirements

**`external_deps` — declare external requirements (MCP servers, env vars, binaries):**
```python
import json
from dcc_mcp_core import SkillMetadata
# external_deps is a JSON string field on SkillMetadata
md.external_deps = json.dumps({
    "tools": [
        {"type": "mcp", "value": "github-mcp-server"},
        {"type": "env_var", "value": "GITHUB_TOKEN"},
        {"type": "bin", "value": "ffmpeg"},
    ]
})
# Read it back:
deps = json.loads(md.external_deps) if md.external_deps else None
```
- Declared in SKILL.md frontmatter as `external_deps:` (YAML mapping)
- Parsed into `SkillMetadata.external_deps` as a JSON string
- Access via `json.loads(metadata.external_deps)` — returns `None` if not set
- See [`docs/guide/skill-scopes-policies.md`](docs/guide/skill-scopes-policies.md) for the full schema

**`CompatibilityRouter` — not a standalone Python class:**
```python
# CompatibilityRouter is returned by VersionedRegistry.router()
# It is NOT importable directly — access via:
from dcc_mcp_core import VersionedRegistry
vr = VersionedRegistry()
router = vr.router()  # -> CompatibilityRouter (borrows the registry)
# For most use cases, use VersionedRegistry.resolve() directly instead
result = vr.resolve("create_sphere", "maya", "^1.0.0")
```

**SEP-986 tool naming — validate names before registration:**
```python
from dcc_mcp_core import validate_tool_name, validate_action_id, TOOL_NAME_RE
# Tool names: dot-separated lowercase (e.g. "scene.get_info")
validate_tool_name("scene.get_info")     # ✓ passes
validate_tool_name("Scene/GetInfo")      # ✗ raises ValueError
# Action IDs: dotted lowercase identifier chains
validate_action_id("maya-geometry.create_sphere")  # ✓
# Regex constants for custom validation:
# TOOL_NAME_RE, ACTION_ID_RE, MAX_TOOL_NAME_LEN (48 chars)
```

**`lazy_actions` — opt-in meta-tool fast-path:**
```python
# When enabled, tools/list surfaces only 3 meta-tools:
# list_actions, describe_action, call_action
# instead of every registered tool at once.
config = McpHttpConfig(port=8765)
config.lazy_actions = True   # opt-in; default is False
```

**`bare_tool_names` — collision-aware bare action names (#307):**
```python
# Default True. tools/list emits "execute_python" instead of
# "maya-scripting.execute_python" when the bare name is unique.
# Collisions fall back to the full "<skill>.<action>" form.
# tools/call accepts BOTH shapes for one release cycle.
config = McpHttpConfig(port=8765)
config.bare_tool_names = True   # default

# Opt-out only if a downstream client hard-coded the prefixed form
# and cannot be updated in lock-step:
config.bare_tool_names = False
```

**`ToolResult.to_json()` — JSON serialization:**
```python
result = success_result("done", count=5)
json_str = result.to_json()    # JSON string
# Also: result.to_dict()       # Python dict
```

---

## Do and Don't — Quick Reference

### Do ✅

- Use `create_skill_server("maya", McpHttpConfig(port=8765))` — the Skills-First entry point since v0.12.12
- Use `success_result("msg", count=5)` — extra kwargs become `context` dict
- Use `ToolAnnotations(read_only_hint=True, destructive_hint=False)` — helps AI clients choose safely
- Use `next-tools: on-success/on-failure` in SKILL.md — guides AI agents to follow-up tools
- Use `search-hint:` in SKILL.md — improves `search_skills` keyword matching
- Use tool groups with `default_active: false` for power-user features — keeps `tools/list` small
- For every new SKILL.md extension, use a `metadata.dcc-mcp.<feature>` key pointing at a sibling file (see "SKILL.md sibling-file pattern" in Traps). Same rule for `tools`, `groups`, `workflows`, `prompts`, and anything future.
- Unpack `scan_and_load()`: `skills, skipped = scan_and_load(dcc_name="maya")`
- Register ALL handlers BEFORE `McpHttpServer.start()` — the server reads the registry at startup
- Use `SandboxPolicy` + `InputValidator` for AI-driven tool execution
- Use `DccServerBase` as the base class for DCC adapters — skill/lifecycle/gateway inherited
- Use `vx just dev` before `vx just test` — the Rust extension must be compiled first
- Keep `SKILL.md` body under 500 lines / 5000 tokens — move details to `references/`
- Use Conventional Commits for PR titles — `feat:`, `fix:`, `docs:`, `refactor:`
- Use `registry.list_actions()` (shows all) vs `registry.list_actions_enabled()` (active only)
- Start with `search_skills(query)` when looking for a tool — don't guess tool names
- Use `init_file_logging(FileLoggingConfig(...))` for durable logs in multi-gateway setups
- Rely on bare tool names in `tools/call` — both `execute_python` and `maya-scripting.execute_python` work during the one-release grace window

### Don't ❌

- Don't iterate over `scan_and_load()` result directly — it returns `(list, list)`, not skill objects
- Don't use `success_result("msg", context={"count": 5})` — kwargs go into context automatically
- Don't call `ToolDispatcher.call()` — method is `.dispatch(name, json_str)`
- Don't pass positional args to `ToolRegistry.register()` — keyword args only
- Don't import `SkillScope` or `SkillPolicy` from Python — they are Rust-only types
- Don't import `DeferredExecutor` from public `__init__` — use `from dcc_mcp_core._core import DeferredExecutor`
- Don't call `.new_auto()` then `.capture_window()` — use `.new_window_auto()` for single-window capture
- Don't use legacy APIs: `ActionManager`, `create_action_manager()`, `MiddlewareChain`, `Action` — removed in v0.12+
- Don't put ANY dcc-mcp-core extension at the top level of a new SKILL.md (v0.15+ / #356) — **the rule is architectural, not a list of specific fields**. `tools`, `groups`, `workflows`, `prompts`, `next-tools` behaviour chains, `examples` packs, and any future extension MUST be a `metadata.dcc-mcp.<feature>` key pointing at a sibling file. See the "SKILL.md sibling-file pattern" trap for the full rationale. Legacy top-level `dcc:`/`tags:`/`tools:`/`groups:`/`depends:`/`search-hint:` still parse for backward compat but emit a deprecation warn and make `is_spec_compliant()` return `False`. See `docs/guide/skills.md#migrating-pre-015-skillmd`.
- Don't inline large payloads (workflow specs, prompt templates, example dialogues, annotation tables) into SKILL.md frontmatter or body, even under `metadata:` — use sibling files. SKILL.md body stays ≤500 lines / ≤5000 tokens.
- Don't use removed transport APIs: `FramedChannel`, `connect_ipc()`, `IpcListener`, `TransportManager`, `CircuitBreaker`, `ConnectionPool` — removed in v0.14 (#251). Use `IpcChannelAdapter` / `DccLinkFrame` instead
- Don't add Python runtime dependencies — the project is zero-dep by design
- Don't manually bump versions or edit `CHANGELOG.md` — Release Please handles this
- Don't hardcode API keys, tokens, or passwords — use environment variables
- Don't use `docs/` prefix in branch names — causes `refs/heads/docs/...` conflicts
- Don't hard-code the legacy `<skill>.<action>` prefixed form in `tools/call` — bare names are the default since v0.14.2 (#307)
- Don't reference `ActionMeta.enabled` in Python — use `ToolRegistry.set_tool_enabled()` instead
- Don't use `json.dumps()` on `ToolResult` — use `result.to_json()` or `serialize_result()`
- Don't guess tool names — use `search_skills(query)` to discover the right tool

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

## Dev Environment Tips

- **Build before testing**: Always run `vx just dev` before `vx just test` — the Rust extension must be compiled first.
- **Preflight before PR**: `vx just preflight` runs cargo check + clippy + fmt + test-rust — catch issues early.
- **Lint auto-fix**: `vx just lint-fix` auto-fixes both Rust (cargo fmt) and Python (ruff + isort) issues.
- **Version never manual**: Release Please owns versioning — never manually edit `CHANGELOG.md` or version strings.
- **Docs-only changes**: Changes to `docs/`, `*.md`, `llms*.txt` skip Rust rebuild in CI — fast turnaround.
- **Branch naming**: Avoid `docs/` prefix (causes `refs/heads/docs/...` conflicts). Use flat names like `feat-xxx` or `enhance-xxx`.

## Security Considerations

- **Sandbox**: Use `SandboxPolicy` + `SandboxContext` for AI-driven tool execution. Never expose unrestricted filesystem or process access.
- **Input validation**: Always validate AI-provided parameters with `ToolValidator.from_schema_json()` before execution.
- **ToolAnnotations**: Signal safety properties (`read_only_hint`, `destructive_hint`, `idempotent_hint`, `open_world_hint`, `deferred_hint`) so AI clients make informed choices.
- **SkillScope**: Trust hierarchy prevents project-local skills from shadowing enterprise-managed ones.
- **Audit log**: `AuditLog` / `AuditMiddleware` provide traceability for all AI-initiated tool calls.
- **No secrets in code**: Never hardcode API keys, tokens, or passwords. Use environment variables or config files outside the repo.

## PR Instructions

- **Title format**: Use Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`
- **Scope optional**: `feat(capture): add DXGI backend`
- **Breaking changes**: `feat!: rename action→tool` with footer `BREAKING CHANGE: ...`
- **Squash merge**: PRs are squash-merged — write the final commit message in the PR title.
- **CI must pass**: `vx just preflight` + `vx just test` + `vx just lint` must all be green.
- **No version bumps**: Release Please handles versioning — never manually bump.

## Commit Message Guidelines

- Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`
- Scope is optional: `feat(capture): add DXGI backend`
- Breaking changes: `feat!: rename action→tool` with footer `BREAKING CHANGE: ...`
- Version bumps are handled by Release Please — never manually edit `CHANGELOG.md` or version strings

## CI & Release

- PRs must pass: `vx just preflight` + `vx just test` + `vx just lint`
- CI matrix: Python 3.7, 3.9, 3.11, 3.13 on Linux / macOS / Windows
- Versioning: Release Please (Conventional Commits) — never manually bump
- PyPI: Trusted Publishing (no tokens)
- Docs-only changes skip Rust rebuild → CI passes quickly
- Squash merge convention for PRs

---

## External Standards & Specifications

| What | Where |
|------|-------|
| MCP spec (implemented: 2025-03-26) | https://modelcontextprotocol.io/specification/2025-03-26 |
| SKILL.md format (agentskills.io) | https://agentskills.io/specification |
| AGENTS.md standard | https://agents.md/ |
| llms.txt format | https://llmstxt.org/ |
| PyO3 (Rust→Python bindings) | https://pyo3.rs/ |
| maturin (wheel builder) | https://www.maturin.rs/ |
| vx (tool manager) | https://github.com/loonghao/vx |

> **MCP spec note**: Library implements 2025-03-26 (Streamable HTTP, Tool Annotations, OAuth 2.1).
> Later specs add: 2025-06-18 (Structured Tool Output, Elicitation, Resource Links, JSON-RPC batching removed);
> 2025-11-25 (icon metadata, Tasks, Sampling with tools, JSON Schema 2020-12).
> The 2026 roadmap focuses on four priority areas:
> **1) Transport scalability** — `.well-known` server capability discovery, stateless session model for horizontal scaling;
> **2) Agent communication** — Tasks primitive (experimental in 2025-11-25), retry/expiration semantics pending;
> **3) Governance** — contributor ladder, delegated workgroup model for faster SEP review;
> **4) Enterprise readiness** — audit trails, SSO integration, gateway behavior, configuration portability (mostly as extensions, not core spec changes).
> No new official transport types will be added in the 2026 cycle — only evolution of Streamable HTTP.
> Do NOT implement these manually — wait for library support.

> **agentskills.io note**: The V1.0 specification (stewarded by Anthropic, released 2025-12-18) defines
> `name` (required, 1-64 chars, lowercase + hyphens, must match directory name),
> `description` (required, 1-1024 chars, should describe **what** and **when to use**),
> `license` (optional, SPDX identifier or file reference),
> `compatibility` (optional, max 500 chars, environment requirements — most skills don't need this),
> `metadata` (optional, arbitrary string→string key-value map), and
> `allowed-tools` (experimental, space-separated pre-approved tool strings like `Bash(git:*) Read`)
> as standard SKILL.md frontmatter fields.
> dcc-mcp-core extends this with `dcc`, `tags`, `search-hint`, `tools`, `groups`, `depends`, `external_deps`, and `next-tools`.
> Validation tool: `skills-ref validate ./my-skill` (from [agentskills/agentskills](https://github.com/agentskills/agentskills)).
> **Progressive disclosure**: Keep `SKILL.md` body < 500 lines / < 5000 tokens; move details to `references/` (loaded on demand).

---

## LLM-Specific Guides

- `CLAUDE.md` — Claude Code workflows and tips (references AGENTS.md for project context)
- `GEMINI.md` — Gemini-specific guidance (references AGENTS.md for project context)
- `CODEBUDDY.md` — CodeBuddy Code-specific guidance (references AGENTS.md for project context)
- `llms.txt` — token-optimised API reference (for AI agents that need to *use* the APIs)
- `llms-full.txt` — complete API reference with copy-paste examples
