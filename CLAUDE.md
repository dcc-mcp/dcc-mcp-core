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
3. **`python/dcc_mcp_core/__init__.py`** — Complete public API surface (~180 symbols)
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
- **On-demand discovery**: `tools/list` returns skill stubs (`__skill__<name>`) for unloaded skills; use `search_skills(query)` then `load_skill(name)` to activate. As of #340 `search_skills` takes `query`/`tags`/`dcc`/`scope`/`limit` (all optional — empty call browses by trust scope). `find_skills` is a deprecated alias that logs a warning and forwards to `search_skills`; removed in v0.17.
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
- **SKILL.md frontmatter fields**: agentskills.io 1.0 allows ONLY six top-level keys — `name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`. All dcc-mcp-core extensions live under `metadata:` as namespaced `dcc-mcp.<feature>` keys. The body stays ≤500 lines / ≤5000 tokens.
- **SKILL.md sibling-file pattern (THE design rule for new features, v0.15+ / #356)**: every new extension — `tools`, `groups`, `workflows`, `prompts`, `next-tools`, `examples`, annotation packs, anything future — MUST be a `metadata.dcc-mcp.<feature>` value that **points at a sibling file** (glob or filename relative to the skill directory). Never inline the payload. This is architectural, not per-feature: when designing a new SKILL.md-touching feature, the gate is "Can it be a `metadata.dcc-mcp.<feature>` pointer to sibling YAML/MD?" If no, write a proposal under `docs/proposals/` first.
  - Good: `metadata["dcc-mcp.workflows"] = "workflows/*.workflow.yaml"` + `workflows/vendor_intake.workflow.yaml` next to SKILL.md.
  - Good: `metadata["dcc-mcp.tools"] = "tools.yaml"` + `tools.yaml` carrying the `tools:` and optional `groups:` lists.
  - Bad: putting a `workflows:` / `tools:` / `prompts:` block at the SKILL.md top level (legacy parse still works but emits a deprecation warn and flips `is_spec_compliant()` to False).
  - Bad: inlining a multi-step workflow or a long prompt template inside `metadata:` as a YAML block — use a sibling file even under `metadata:`.
  - See `docs/guide/skills.md#migrating-pre-015-skillmd` for the before/after mapping.
- **`next-tools`**: Per-tool field guiding AI agents to follow-up tools (`on-success` / `on-failure`). dcc-mcp-core extension, carried inside the `tools.yaml` sibling file — never at the SKILL.md top level.
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

- Uses IPC (Unix socket / named pipe) for process communication, implemented on top of `ipckit`.
- **DccLink adapters are the public API** (v0.14+): `IpcChannelAdapter`, `GracefulIpcChannelAdapter`,
  `SocketServerAdapter`. Each wraps an `ipckit` primitive with a stable DCC-Link frame
  (`[u32 len][u8 type][u64 seq][msgpack body]`).
- Message kinds: `DccLinkType::{Call, Reply, Err, Progress, Cancel, Push, Ping, Pong}` —
  use `DccLinkFrame` to construct them.
- Client: `IpcChannelAdapter.connect(name)`; Server: `IpcChannelAdapter.create(name)` or
  `SocketServerAdapter.new(path, max_connections, connection_timeout)`.
- Service discovery lives in `dcc_mcp_transport::discovery::FileRegistry`
  (`ServiceEntry`, `ServiceKey`, `ServiceStatus`).
- **Removed in v0.14 (issue #251)**: `FramedChannel`, `FramedIo`, `TransportManager`,
  `IpcListener` (Python class), `ListenerHandle`, `RoutingStrategy`, `ConnectionPool`,
  `InstanceRouter`, `CircuitBreaker`, `MessageEnvelope`, `Request`/`Response`/`Notification`,
  `connect_ipc`, `encode_request`/`encode_response`/`encode_notify`/`decode_envelope`.
  Do NOT reference these symbols in new code; they no longer exist.

### MCP HTTP server spawn modes (issue #303 fix)

`McpHttpConfig` exposes a `spawn_mode` that picks how listeners are driven:

- **`Ambient`** — listeners run as `tokio::spawn` tasks on the caller's runtime.
  Correct for `#[tokio::main]` binaries like `dcc-mcp-server` where a driver
  thread persists for the process lifetime.
- **`Dedicated`** — each listener runs on its own OS thread with a
  `current_thread` Tokio runtime. Default for PyO3-embedded hosts
  (Maya/Blender/Houdini). Prevents the "is_gateway=true but port
  unreachable" failure mode observed on Windows mayapy.

The Python `McpHttpConfig` defaults `spawn_mode = "dedicated"`;
`McpHttpServer.start()` self-probes the new listener and refuses to
return a handle that claims to be bound when it actually is not.
If you write new code that constructs `McpHttpServer` from Rust inside
a PyO3 binding, set `spawn_mode = ServerSpawnMode::Dedicated` explicitly.

### Prometheus `/metrics` exporter (issue #331)

Opt-in behind the `prometheus` Cargo feature — **off by default**.
When compiled in, enable at runtime via
`McpHttpConfig(enable_prometheus=True, prometheus_basic_auth=(u, p))`.
Metric names live in [`docs/api/observability.md`](docs/api/observability.md);
see there for Grafana PromQL examples. Counters advance from the
`tools/call` wrapper in `handler.rs` — do not add recording sites
elsewhere.

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

# Resources primitive (#350) — live DCC state via resources/list|read|subscribe
# McpHttpConfig.enable_resources defaults to True. Built-in URIs:
#   scene://current           (JSON; update via server.resources().set_scene(...) in Rust)
#   capture://current_window  (PNG blob; Windows HWND PrintWindow backend only)
#   audit://recent?limit=N    (JSON; wire via server.resources().wire_audit_log(log) in Rust)
#   artefact://sha256/<hex>   (content-addressed artefact; #349) — toggle via enable_artefact_resources
cfg = McpHttpConfig(port=8765)
cfg.enable_resources = True            # advertise capability + built-ins
cfg.enable_artefact_resources = False  # default: artefact:// returns JSON-RPC -32002

# Prompts primitive (#351, #355) — reusable templates served via prompts/list|get
# McpHttpConfig.enable_prompts defaults to True.
# Prompts come from each loaded skill's sibling file referenced by
# metadata["dcc-mcp.prompts"] in SKILL.md — either a single `prompts.yaml`
# (top-level `prompts:` + `workflows:` lists) or a `prompts/*.prompt.yaml` glob.
# Workflows referenced by the spec auto-generate a summary prompt.
# Template engine is minimal: only {{arg_name}} substitution, missing required
# args return JSON-RPC INVALID_PARAMS. notifications/prompts/list_changed fires
# on skill load / unload.
cfg.enable_prompts = True              # advertise capability + serve templates
```

### Artefact Hand-Off (FileRef + ArtefactStore, issue #349)

```python
from dcc_mcp_core import (
    FileRef,
    artefact_put_file, artefact_put_bytes,
    artefact_get_bytes, artefact_list,
)

# Content-addressed SHA-256 store. Duplicate bytes → same URI.
ref = artefact_put_bytes(b"hello", mime="text/plain")
ref.uri          # "artefact://sha256/<hex>"
ref.size_bytes   # 5
ref.digest       # "sha256:<hex>"
assert artefact_get_bytes(ref.uri) == b"hello"

# When McpHttpConfig.enable_artefact_resources=True the server exposes
# every FileRef as an MCP resource — clients resources/read the uri.
```

Rust: `dcc_mcp_artefact::{FilesystemArtefactStore, InMemoryArtefactStore,
ArtefactStore, ArtefactBody, ArtefactFilter, put_bytes, put_file, resolve}`.
`FilesystemArtefactStore` persists at `<root>/<sha256>.bin` + `.json`.

### Quick Lookup: Common Method Signatures

```python
# ToolDispatcher — only .dispatch(), never .call()
dispatcher = ToolDispatcher(registry)   # takes ONE arg; no validator param
result = dispatcher.dispatch("action_name", json.dumps({"key": "value"}))
# result keys: "action", "output", "validation_skipped"

# Async tools/call dispatch (#318) — opt-in, returns {job_id, status: pending}
# Opt-in when ANY of: _meta.dcc.async=true, _meta.progressToken set,
# or the ActionMeta declares execution: async / timeout_hint_secs > 0.
# Parent-job cascade: _meta.dcc.parentJobId makes the new Job's
# CancellationToken a child of the parent's; cancelling the parent cancels
# every descendant within one cooperative checkpoint.

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

# DccLink IPC — primary RPC path (v0.14+, issue #251)
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter
channel = IpcChannelAdapter.connect(f"dcc-mcp-maya-{pid}")   # Named Pipe / UDS
channel.send_frame(DccLinkFrame(msg_type="Call", seq=1, body=b"{...}"))
reply = channel.recv_frame()   # DccLinkFrame: msg_type, seq, body (bytes)
# Use SocketServerAdapter for multi-client servers.

# McpHttpServer — expose registry over HTTP/MCP
# Python default: spawn_mode="dedicated" (issue #303 fix)
server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()   # McpServerHandle; guaranteed reachable after return
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"

# Job lifecycle notifications (#326) — every tools/call emits SSE frames:
#   notifications/progress                  (when _meta.progressToken is set)
#   notifications/$/dcc.jobUpdated         (gated by enable_job_notifications, default True)
#   notifications/$/dcc.workflowUpdated    (same gate; #348 executor populates it)
cfg = McpHttpConfig(port=8765)
cfg.enable_job_notifications = False  # opt the $/dcc.* channels out

# Workflow step policies (#353) — retry / timeout / idempotency
from dcc_mcp_core import WorkflowSpec, BackoffKind
spec = WorkflowSpec.from_yaml_str(yaml)
spec.validate()                          # static check on idempotency_key refs
policy = spec.steps[0].policy            # frozen snapshot
policy.timeout_secs                      # Optional[int]
policy.retry.max_attempts                # >= 1; 1 = no retry
policy.retry.backoff                     # BackoffKind.{FIXED,LINEAR,EXPONENTIAL}
policy.retry.next_delay_ms(2)            # first-retry base delay, no jitter
policy.idempotency_scope                 # "workflow" (default) | "global"
# Executor enforcement is #348 follow-up; this release lands types+parser only.
```

### Gateway lifecycle invariants (issue #303)

These hold after v0.14 and MUST NOT regress:

1. **`handle.is_gateway == True` ⇒ the gateway port is reachable.** The
   election code runs a loopback `TcpStream::connect` self-probe before
   declaring victory; if the probe fails it falls back to plain-instance
   mode and returns `is_gateway = false`. Do not skip this probe.
2. **The gateway supervisor `JoinHandle` must outlive `GatewayHandle`.**
   Earlier versions dropped the JoinHandle at the end of
   `start_gateway_tasks`; under PyO3-embedded hosts that detached the
   accept loop and made it unreachable. Keep the `JoinHandle` in the
   `GatewayHandle` struct.
3. **Socket setup errors must not be silenced with `.ok()?`.**
   `try_bind_port` returns `io::Result`; only `AddrInUse` is treated as
   a lost election, all other errors are logged at warn level.
4. **Python / PyO3 callers default to `ServerSpawnMode::Dedicated`.**
   `PyMcpHttpConfig::new` sets this automatically; `py_create_skill_server`
   also coerces `Ambient` → `Dedicated`. Do not revert to Ambient inside
   Python bindings.

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
- **Declaring tool annotations** (#344): Put MCP `ToolAnnotations` inside each tool entry in the sibling `tools.yaml` file — never as a top-level SKILL.md frontmatter key. Canonical form is a nested `annotations:` map; shorthand flat `*_hint:` keys on the tool entry still parse for backward compatibility. When both forms are present the nested map wins whole-map (not per-field merge). `deferred_hint` is a dcc-mcp-core extension and surfaces in `_meta["dcc.deferred_hint"]` on `tools/list`, never inside the spec `annotations` map. See `docs/guide/skills.md#declaring-tool-annotations-issue-344`.
- **Security**: Use `SandboxPolicy` + `SandboxContext` for AI-driven tool execution. Validate inputs with `ToolValidator`. Never hardcode secrets.
- **Tool descriptions** (#341): Every built-in MCP tool description follows a 3-layer structure — 1-sentence "what" / `When to use:` / `How to use:` bullets — capped at 500 chars, with per-param descriptions ≤ 100 chars. Enforced by `tests/test_tool_descriptions.py`. See the "Writing Tool Descriptions" section in `AGENTS.md`; move long prose to `docs/api/http.md`.
- **Commit messages**: Use Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`). Never manually bump versions — Release Please manages this.

## AI Agent Tool Priority

When building tools or interacting with DCCs, follow this priority order:

1. **Skill Discovery** (start here): `search_skills(query)` → `load_skill(name)` → use skill tools
2. **Skill-Based Tools** (preferred): Tools with validated schemas, error handling, `next-tools` guidance, and `ToolAnnotations` safety hints
3. **Diagnostics Tools** (for verification): `diagnostics__screenshot`, `diagnostics__audit_log`, `diagnostics__process_status`, and — as a polling fallback when you can't subscribe to the `$/dcc.jobUpdated` SSE channel — **`jobs.get_status`** (#319, always registered, returns the full job-state envelope for a given `job_id`). Use **`jobs.cleanup`** (#328) with `older_than_hours` to prune terminal jobs; combine with `McpHttpConfig.job_storage_path` + Cargo feature `job-persist-sqlite` for restart-safe job history (pending/running rows become `Interrupted` on reboot).
4. **Direct Registry Access** (last resort): Only when no skill tool covers the operation; must validate with `ToolValidator` and sandbox with `SandboxPolicy`

**Why skills first?** Safety (annotations), discoverability (search-hint), chainability (next-tools), progressive exposure (tool groups), validation (input_schema).
