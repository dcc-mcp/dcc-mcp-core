# AI Agent Guide — dcc-mcp-core

> **Quick orientation for AI agents**: This guide teaches you how to effectively use dcc-mcp-core to interact with DCC (Digital Content Creation) software like Maya, Blender, Houdini, etc.

## 🎯 Core Philosophy: Skills-First

**When interacting with DCC applications, ALWAYS prefer dcc-mcp-core Skills over raw CLI scripting or direct API calls.**

Load the default `dcc-mcp` skill whenever the user asks to do work **in** Maya,
Blender, Houdini, Photoshop, 3ds Max, Nuke, Unreal, Substance 3D, or another
supported DCC. The user does not need to mention MCP or know a tool name:
requests such as “help me model this in Maya” and “render this in Houdini” are
direct triggers. Inventory the requested host, search by intent, inspect the
schema, and call the structured tool before considering raw scripts or GUI
automation.

### Why Skills-First?

| Aspect | dcc-mcp-core Skills | Raw CLI / Scripting |
|--------|---------------------|---------------------|
| **Input Validation** | JSON Schema validated | None — garbage in, garbage out |
| **Safety** | `ToolAnnotations` (read-only, destructive, idempotent) | Unknown |
| **Discoverability** | `search_skills()` + semantic search | Manual exploration |
| **Follow-up Guidance** | `next-tools` chains | Manual discovery |
| **Progressive Loading** | Load only what you need | All or nothing |
| **Error Recovery** | Structured `error_result` with `prompt` suggestions | Unstructured stderr |
| **Traceability** | Audit log + telemetry | None |

## 🚀 Agent Entry Strategy: CLI+REST (default) vs IDE MCP

**As an AI agent, your default path is CLI+REST through the gateway.** Human IDE users continue using MCP configuration. The gateway serves both paths simultaneously.

### Decision Table — Which Path Should I Use?

| If you are... | Use this path | How to start |
|---------------|---------------|--------------|
| An AI agent in OpenClaw, Hermes, Codex CLI, or any headless agent runtime | **CLI+REST** ← **this is you** | Load `dcc-mcp` skill → call `dcc-mcp-cli search/describe/call` |
| An AI agent in Cursor / Claude Desktop / VS Code with MCP enabled | **Either works**, prefer CLI+REST | CLI+REST via `dcc-mcp` is preferred; IDE MCP is available as fallback |
| Running a CI/CD or automation script | **CLI+REST** | `dcc-mcp-cli` with structured output and exit codes |
| Troubleshooting DCC connectivity | **CLI+REST** | `dcc-mcp-cli health/list/smoke` |
| A human IDE user reading this guide | **IDE MCP** | Configure `mcp_servers.json` → gateway MCP tools |
| A GUI artist using DCC plugin directly | **IDE MCP** | DCC's built-in MCP plugin |

### Core Principle

> **Agent → CLI+REST → `dcc-mcp-cli` → gateway REST API → DCC control**
> **Human IDE → MCP → gateway MCP surface → DCC control**

CLI-first does **not** deprecate MCP. The gateway always exposes both MCP and REST side by side.

### Install and keep the CLI current

If `dcc-mcp-cli` is missing, obtain the user's consent, install the public
`dcc-mcp` Skill, and run its bundled verified helper from the Skill directory:

```bash
python scripts/check_cli.py --ensure-cli --pretty
```

The helper is fixed to the official `dcc-mcp/dcc-mcp-core` release. It checks
the platform update manifest and CLI SHA-256 before replacing anything, and
fails closed on an invalid URL, manifest, digest, or download. SHA-256 verifies
that the binary matches the release manifest; it is not a digital signature. If
the Skill is unavailable,
[download the official installer to a local file, inspect it, and then execute
that file](docs/guide/getting-started.md#cli-from-the-dcc-mcp-skill). Never pipe
a remote installer directly into a shell or bypass the machine's script
execution policy.

Keep an official build current through the release manifest:

```bash
dcc-mcp-cli update check
dcc-mcp-cli update apply
```

`update apply` downloads and stages the latest CLI for the next launch. It does
not update a running `dcc-mcp-server`; update that server in its own environment.

### Computer Use is an agent-directed fallback

Do not start with GUI automation. Discover and call structured DCC skills,
host APIs, or adapter scripts first. Load `ui-control` only when that path returns
`unsupported` or `capability_missing`. Policy denial, user interruption,
authentication, or desktop unavailability are stop conditions, not fallback
signals:

1. `ui_control__snapshot` scoped to the exact process or window. Every Windows UIA
   mutation requires the adapter/operator to bind its DCC with
   `DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE`.
   A request may select that exact PID/HWND or narrow it further, but cannot
   replace the trusted runtime scope with another application.
   The visible session and bounded native screenshot do not require raw input.
   Native pointer or keyboard control has a second gate and additionally
   requires `DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true`.
2. One semantic action when possible, otherwise one screenshot-coordinate
   `ui_control__act` using the returned `snapshot_id`.
3. `ui_control__snapshot` after every action before deciding what to do next.
4. `ui_control__stop_computer_use` when the task completes, fails, or is abandoned.

Windows plug-in setup that needs an HKCU string/DWORD or a file/directory
symbolic link uses the separate `ui_control__system_operation` tool. It requires an
exact operator-owned grant catalog and trusted action-time confirmation, and it
does not require a PID, HWND, or snapshot. It has no shell, deletion,
replacement, alternate-hive, or elevation form. Treat `elevation_required`,
`approval_required`, and `system_operation_not_granted` as stop conditions.

Never automate the whole desktop or reuse coordinates from an older snapshot.
The Windows session shows a click-through target border, control banner, and
pointer-action markers. The user stops control by pressing `Esc`.
After `user_interrupted`, stop and do not retry, change `session_id`, or
automatically restart; the stop is latched across DCC adapter processes in the
same Windows logon session.
Resume only with `ui_control__snapshot(resume_computer_use=true)` after the user
explicitly asks to continue and while no Computer Use owner is active.

Treat `desktop_unavailable` as a Windows desktop pause, not a failed logical
session: lock, disconnect, and secure-desktop transitions stop UIA and raw
input without sending input. Stop UI calls, ask the user to unlock or
reconnect, and do not poll autonomously. Keep the same `session_id`.
Structured DCC/MCP work may continue only if the host remains ready.
After unlock or reconnect, discard old observations and control ids and take a
fresh exact-target snapshot; the banner returns with that successful snapshot.
An ordinary process cannot place the banner on the Windows lock screen. Use a
dedicated, always-unlocked VM for truly uninterrupted Windows GUI control.

Computer Use runs on the adapter host in the interactive Windows logon session
that owns the DCC. The central gateway only routes calls: never send gateway,
other-host, or other-session coordinates to that DCC. An RDP disconnect or
session switch preserves the logical session but returns
`desktop_unavailable`; reconnect to the DCC session and take a fresh snapshot.

Screenshots are bounded to the scoped target window, never the whole desktop.
That target may span displays with negative virtual-desktop origins or
different DPI. Use only coordinates from the latest returned PNG. Any monitor
topology, resolution, or DPI/scaling change invalidates the observation and
requires a fresh snapshot. The banner follows the scoped target within its
owning logon session.

Never target LockApp, Windows Security, credential/authentication/password
manager windows, the Windows Run dialog, terminals, PowerShell, or `cmd`.
These backend-enforced boundaries cannot be bypassed by switching automation
methods. A script editor hosted by the bound DCC process remains in scope.
Passwords stay a user hand-off or application-owned OAuth/browser flow; never
place them in `ui_control__act.text` or a system-operation registry value.

`ui_control__act` advertises a destructive annotation so the calling host can
apply its confirmation policy. A model-supplied `confirmed` argument or
environment bypass is not a trusted approval. If host policy requires user
confirmation and none is available, stop; do not use another automation path.

### Record and replay demonstrations

Record-replay turns an operator demonstration into a reviewable local Skill,
not a mouse/keyboard macro. Use one trusted `--agent-session-id` throughout:

```bash
dcc-mcp-cli --agent-session-id task-42 record-replay start --dcc-type maya
# demonstrate through gateway search -> describe -> call; use scoped UI Control only as fallback
dcc-mcp-cli --agent-session-id task-42 record-replay stop <recording-id>
dcc-mcp-cli --agent-session-id task-42 record-replay review <recording-id>
dcc-mcp-cli --agent-session-id task-42 record-replay compile <recording-id> \
  --name reviewed-scene-build --reviewed
```

Compilation resolves the current backend tool and schema, removes failed
exploration and demonstration-time approvals, parameterizes reviewed inputs,
and emits `SKILL.md`, `workflows/replay.workflow.yaml`, and
`references/REPLAY_CONTRACT.md`. Replay remains a separate grant and requires
`--approve-replay`; it must stop on schema/target/approval/desktop/observation
drift. Never record raw prompts, credentials, reusable grants, global
coordinates, or stale control ids.

## 🚀 Quick Start Workflow

### Default Agent Path: CLI+REST

```bash
# 0. Inspect catalog-backed DCC identifiers when support is unclear
dcc-mcp-cli dcc-types

# 1. Select a live instance; local list auto-ensures the loopback gateway
dcc-mcp-cli list

# 2. Search for tools
dcc-mcp-cli search --query "create sphere" --dcc-type maya

# 3. Inspect a tool schema
dcc-mcp-cli describe maya.a1b2c3d4.create_sphere

# 4. Call the tool
dcc-mcp-cli call maya.a1b2c3d4.create_sphere \
  --json '{"radius": 2.0}' \
  --meta-json '{"agent_context":{"session_id":"task-42"}}'

# 5. Batch calls
dcc-mcp-cli call --batch --steps '[
  {"tool_slug": "maya.a1b2c3d4.create_sphere", "arguments": {"radius": 2.0}},
  {"tool_slug": "maya.a1b2c3d4.assign_material", "arguments": {"name": "mat_blue"}}
]'
```

Use the `dcc-mcp` skill to wrap these CLI calls as structured MCP tools in your agent runtime. This is the recommended pattern for all agent integrations.

`dcc-types` reads the release catalog without starting a gateway; it reports
adapter-backed identifiers such as `godot` and `renderdoc`, while `list` reports
live sessions. After the task passes its acceptance checks, query bounded
evidence with `dcc-mcp-cli stats --range 24h --session-id task-42`, then use the
`review_skill_improvement` prompt from `dcc-mcp-skills-creator`. Treat zero calls
as missing evidence, never send raw prompts or secrets, and prefer no change or
an existing-skill update over creating another skill.

### Quick Start: Skills (Python API)

For embedded / in-process Python usage:

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry, scan_and_load

# Always start by discovering what's available.
# Returns: (List[SkillMetadata], List[str] skipped_dirs).
skills, skipped = scan_and_load(dcc_name="maya")

# For AI agents: use search_skills for semantic discovery.
registry = ToolRegistry()
catalog = SkillCatalog(registry)
results = catalog.search_skills(query="create sphere geometry")
```

### 2. Load the Skill

```python
# Load a specific skill to expose its tools
catalog.load_skill("maya-geometry")
```

### 3. Call Tools with Validation

```python
# Tools are now available via the dispatcher
result = dispatcher.dispatch("maya-geometry__create_sphere", '{"radius": 2.0}')

# Always check the result structure
if result.get("success"):
    print(f"Tool succeeded: {result.get('message')}")
else:
    print(f"Tool failed: {result.get('error')}")
    print(f"Suggestion: {result.get('prompt')}")

# Over MCP, follow-up hints are attached to CallToolResult._meta["dcc.next_tools"].
# Use .on_success after successful calls and .on_failure after errors when present.
```

### 4. Follow next-tools Guidance

When an MCP `tools/call` response includes `CallToolResult._meta["dcc.next_tools"].on_success` or `.on_failure`, **always consider calling those tools next**. This creates a guided workflow chain; the declarations live per tool in sibling `tools.yaml`, not as top-level `SKILL.md` keys.

---

> **Note for AI agents**: The sections below describe the IDE / MCP integration path. Your default is the **CLI+REST** path above. Use these MCP sections when:
> - You are running inside an IDE with MCP support (Cursor, Claude Desktop)
> - You need gateway resources/prompts not yet exposed via REST
> - You are troubleshooting MCP-specific behavior

### IDE Path: Direct Per-DCC MCP Discovery

If your MCP connection is a direct Maya/Blender/Houdini/etc. server, do not
treat the first `tools/list` page as the complete tool index. `tools/list` is
paginated and may put a newly loaded tool on a later page.

Use this compact flow instead:

```python
# Direct per-DCC MCP workflow
hits = search_tools(query="capture viewport", limit=5)
info = get_skill_info(skill_name=hits["skill_candidates"][0]["skill_name"])
load_skill(skill_name=info["name"])
result = tools_call(name="maya_render__capture_viewport", arguments={})
```

Use `search_tools` for active tools and unloaded skill candidates. Use
`search_skills` when you are looking for a skill by intent rather than a known
tool name. Use `get_skill_info` to inspect a selected skill's full tool schemas
before loading it. If you intentionally call `tools/list`, follow every
`nextCursor` until it is absent.

### IDE Path: Gateway MCP Surface

If your MCP connection is the multi-DCC gateway, do not expect backend actions to appear directly in `tools/list`. The gateway surface is intentionally fixed and bounded; use the dynamic-capability workflow instead:

```python
# Gateway MCP four-tool workflow
hits = search(kind="tool", query="create sphere", dcc_type="maya", limit=5)
info = describe(tool_slug=hits["hits"][0]["tool_slug"])
result = call(tool_slug=info["record"]["tool_slug"], arguments={"radius": 2.0})

# Ordered MCP batch flow (max 25 calls)
batch = call(
    calls=[
        {"tool_slug": info["record"]["tool_slug"], "arguments": {"radius": 2.0}},
        {"tool_slug": "maya.a1b2c3d4.assign_material", "arguments": {"name": "mat_blue"}},
    ],
    stop_on_error=True,
)
```

Use `search(kind="skill", ...)` to find unloaded skills, then `load_skill(skill_name="...", instance_id="...")` when a search hit's `next_step` asks for activation. Gateway `tools/list` advertises exactly `search`, `describe`, `load_skill`, and `call`. Hidden MCP compatibility routes still accept older `search_tools` / `describe_tool` / `call_tool` / `call_tools` names, but new agent workflows should use the four canonical tools.

Wrapper payloads accept only `tool_slug`, `arguments`, and optional `meta`. Put backend-specific inputs such as `code`, `script`, `file_path`, or `radius` inside `arguments`, never at the wrapper top level. `dcc-mcp-wire` normalizes missing / `null` / empty-string arguments to `{}` and rejects non-object roots; Python host wrappers can call `dcc_mcp_core.host.normalize_tool_arguments()` / `normalize_tool_meta()`.

For ad-hoc script execution, prefer typed tools first, then materialize source
on the DCC host and execute by path. Use
`dcc_mcp_core.materialize_script(content, dcc_type=..., instance_id=..., session_id=...)`
to write under the configurable `~/.dcc-mcp/<dcc_type>/temp/<instance_id>/<session_id>/`
store and receive a descriptor with `file_ref`, `file_path`, `sha256`,
`bytes`, TTL, session, tool-call, and correlation metadata. `write_temp_script`
is still available for compatibility, but the structured descriptor is the
auditable contract.

Core script execution helpers now normalize through
`script_materialization_policy = off | auto | require`. The default `auto`
mode transparently turns inline `code` into a materialized host-local
`file_path` before execution. Use `require` when an adapter boundary must reject
raw inline code, and use `off` only as a short-lived compatibility escape hatch.
Execution results should return `context.materialized_script` with `path`,
`file_ref`, `sha256`, `bytes`, and `reused` metadata; legacy spilled-script
context keys are migration aliases, not the preferred contract.

Agents can also call the `materialize_script` MCP/REST tool exposed by
`DccServerBase` adapters. Discover it with `search_tools("materialize script")`,
call it with `content` (or legacy `code`), then pass the returned `file_path`
to the execution tool. The tool returns FileRef/path/hash/TTL/session metadata
and never echoes raw source. Gateway traces and admin audit rows redact
script-source input fields by default and keep the descriptor metadata instead.

Pure HTTP clients use the same REST endpoints directly: `POST /v1/search`, `POST /v1/describe`, `POST /v1/call`, and gateway `POST /v1/call_batch`. Gateway REST returns compact TOON by default; send `Accept: application/json` or body `response_format: "json"` when a legacy JSON client needs compatibility. See `docs/guide/gateway.md` and `docs/guide/rest-api-surface.md`.

### Gateway workflow guide (`gateway://docs/agent-workflows`)

**`resources/read`** with **`uri=gateway://docs/agent-workflows`** is the **platform-agnostic** copy bundled with the gateway: MCP **tools** vs **`resources/list`/`read`** / **`prompts`**, using **`describe`** (schema, **affinity**, execution mode, timeouts), fewer redundant round-trips, optional **`call({calls:[...]})`** / **`POST /v1/call_batch`** (≤25 ordered steps), and reading **host-published help** URIs exactly as listed—never inventing schemes. Re-fetch in very long sessions if the contract might have fallen out of context.

### Gateway Instance Discovery

Usually you do **not** need to enumerate instances: let gateway `search` and `call` route for you. When you must pick a concrete DCC session, inspect context metadata, or connect directly, read the gateway-native MCP resource instead of looking for instance-discovery tools:

```python
# MCP request shape; use your client's resources/read helper if it has one.
{"method": "resources/read", "params": {"uri": "gateway://instances"}}
{"method": "resources/read", "params": {"uri": "gateway://instances/{instance_id}"}}
```

Each entry carries `mcp_url`, so no separate connect verb is needed. The legacy `list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`, and non-standard `instances/list` surfaces were removed in #813 phase 1.

### Gateway Resources and Prompts


Use MCP resources for files, scene artefacts, thumbnails, diagnostics, and other hand-off data that should not be squeezed into tool text output:

1. Call `resources/list` and keep the returned URI exactly as-is. Gateway-prefixed URIs encode the owning DCC instance (`dcc://<type>/<id>` or `<scheme>://<id8>/<rest>`).
2. `resources/list` advertises `gateway://instances` as one root pointer; read `gateway://instances/{id}` directly when you know an instance id because per-instance URIs are intentionally not fanned out.
3. Call `resources/read` with that exact URI. Do not remove or rewrite the instance prefix client-side.
4. Optional: **`resources/read` `uri=gateway://docs/agent-workflows`** — same content as the subsection above; use one or the other as a reminder in long sessions.
5. Use `resources/subscribe` only when you need live `notifications/resources/updated` events, then call `resources/unsubscribe` when done.
6. Prefer resources over ad-hoc local file paths in tool messages; resources are portable across DCC hosts and easier for agents to trace.
7. For reusable prompt templates, call gateway `prompts/list` and then `prompts/get` with the returned namespaced prompt name.

### Gateway Admin Observability

When debugging routing, slow calls, or worker availability, use the elected gateway's read-only admin JSON APIs before guessing from logs: `GET /admin/api/instances`, `/tools`, `/calls`, `/traces`, `/traces/{request_id}`, `/stats?range=24h`, `/workers`, `/logs`, and `/health`. The `/logs` feed merges gateway contention events, on-disk `*.log` rows from `DCC_MCP_LOG_DIR` (or the platform default), and audited call summaries. The HTML dashboard remains `GET /admin`; disable it with `--no-admin`, `DCC_MCP_NO_ADMIN=true`, or `cfg.admin_enabled = False`. For restart-stable call/trace history, operators can set `DCC_MCP_GATEWAY_AUDIT_DIR` to persist `audit.jsonl` and `traces.jsonl`.

## 📚 Key Concepts You Must Understand

### 1. scan_and_load Returns a 2-Tuple

```python
# ✓ CORRECT - always unpack both values
skills, skipped = scan_and_load(dcc_name="maya")

# ✗ WRONG - don't iterate directly
for skill in scan_and_load(...):  # BREAKS - returns tuple, not list
```

### 2. ToolResult Structure

Always use the provided factories (`success_result`, `error_result`) — never hand-roll dicts:

```python
from dcc_mcp_core import success_result, error_result

# ✓ CORRECT - use factories
result = success_result("Created sphere", prompt="Add material next", count=5)
# result.to_dict() -> {"success": True, "message": "...", "context": {"count": 5}}

# ✗ WRONG - hand-rolled dict
result = {"success": True, "message": "..."}  # Missing context, not forward-compatible
```

### 3. Tool Annotations for Safety

Tools declare their safety hints via `ToolAnnotations`:

- `read_only_hint=True` — does not modify state (safe to call)
- `destructive_hint=True` — modifies state, possibly irreversible
- `idempotent_hint=True` — safe to call multiple times

**Always check annotations before calling tools on production scenes.**

### 4. Progressive Loading with Tool Groups

Skills can expose tools progressively:

```python
# List all declared groups as (skill_name, group_name, active) tuples.
groups = catalog.list_groups()

# Activate/deactivate by group name.
catalog.activate_group("advanced")
catalog.deactivate_group("experimental")
active = catalog.active_groups()
```

### 5. Lifecycle Hooks — Observe and Control

`LifecycleHooks` provides a typed, fail-safe observer system for skill/tool/session events:

- **Policy events** (`BEFORE_SKILL_LOAD`, `BEFORE_TOOL_CALL`, `BEFORE_SEARCH`): Raise `HookDeny` to veto
- **Observation events** (`AFTER_*`, `SESSION_*`): Log and analytics only — exceptions are swallowed

```python
from dcc_mcp_core import LifecycleHooks, HookEvent, HookDeny

hooks = LifecycleHooks()

@hooks.on(HookEvent.BEFORE_TOOL_CALL)
def block_dangerous(ctx):
    if "dangerous" in ctx.payload.get("tool_name", ""):
        raise HookDeny("blocked", hint="use the safe alternative")

server.register_lifecycle_hooks(hooks)
```

### 6. Agent Memory — Automatic Context Retention

`MemoryRecorder` automatically records skill/tool outcomes and injects memory
summaries into search and tool-call context — no manual logging needed:

```python
from dcc_mcp_core import InMemoryMemoryStore, MemoryRecorder

store = InMemoryMemoryStore()
MemoryRecorder(store).install(hooks)  # wires 6 lifecycle events
# From now on: skill loads → EPHEMERAL, tool calls → WORKING,
# session end → compacted to LONGTERM patterns
# BEFORE_SEARCH and BEFORE_TOOL_CALL auto-inject memory_summary
```

## 🔧 Common Tasks — Which API to Use

| Task | Use this API |
|------|---------------|
| **Control DCC via CLI (agent default)** | Load `dcc-mcp` skill → `dcc-mcp-cli search/describe/call` |
| **Expose DCC tools over MCP** | `DccServerOptions.from_env(...)` → `DccServerBase(opts)` → `start()` |
| **Zero-code tool registration** | agentskills.io `SKILL.md` + `metadata.dcc-mcp.tools` pointing at sibling `tools.yaml` + `scripts/` |
| **Return structured results** | `success_result()` / `error_result()` |
| **Rich error with traceback** | `skill_error_with_trace()` |
| **Bridge non-Python DCC** | `DccBridge` (WebSocket JSON-RPC 2.0) |
| **Register lifecycle hooks** | `LifecycleHooks()` + `server.register_lifecycle_hooks(hooks)` |
| **Enable agent memory** | `MemoryRecorder(InMemoryMemoryStore()).install(hooks)` |
| **Register all built-in tools** | `register_all_builtin_skills(server, dcc_name=..., skills=...)` |
| **IPC between processes** | `IpcChannelAdapter` / `SocketServerAdapter` |
| **Hand off files between tools** | `FileRef` + `artefact_put_file()` / `artefact_get_bytes()` |
| **Multi-DCC gateway** | `McpHttpConfig(gateway_port=9765)` |
| **Long-lived cancellation support** | `check_cancelled()` / `check_dcc_cancelled()` |

## 🎭 Skill Authoring for AI Agents

When creating skills, optimize for AI agent discoverability:

### Description Pattern (Required)

Every skill `description` must follow this 3-part structure (max 1024 chars):

```
<Layer> skill — <one-sentence what + scope keywords>. Use when <trigger>.
Not for <counter-example> — use <other-skill> for that.
```

**Example (Domain skill):**
```yaml
description: >-
  Domain skill — Maya polygon geometry: create spheres, cubes, cylinders;
  bevel and extrude polygon components. Use when the user asks to create or
  modify 3D meshes in Maya. Not for USD export pipelines — use
  maya-pipeline for that. Not for raw USD file inspection — use usd-tools for that.
```

### search-hint Optimization

Include specific keywords that AI agents will match against:

```yaml
metadata:
  dcc-mcp:
    search-hint: "polygon modeling, bevel, extrude, mesh creation, Maya geometry"
```

### next-tools Chains

Always provide follow-up guidance in the sibling `tools.yaml` referenced by `metadata.dcc-mcp.tools`:

```yaml
# tools.yaml
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya_geometry__bevel_edges, maya_geometry__apply_material]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

## 🔴 Red Lines — Python 3.7 Support Policy

**dcc-mcp treats Python 3.7 as a long-term-support profile.** Removing it requires
an accepted superseding ADR, a major release, at least 180 days of notice, and
an adapter migration path. The source of truth is `compatibility/python.json`.

This is a hard requirement — Maya 2022, Blender 2.83, and many DCC hosts embed Python 3.7. Every change you make must keep this constraint in mind:

1. **`py37-lite` / fallback mode is NOT sufficient evidence** of Python 3.7 support. Native Linux and Windows cp37 gates are required as well.
2. **PyO3 must stay on the contracted series** unless the proposed upgrade passes native py37 builds and runtime validation in the same change.
3. **`requires-python = ">=3.7"`** in `pyproject.toml` is the canonical classifier. Do not bump it without explicit policy override.
4. **CI must include a Python 3.7 job** for every PR that touches Rust/PyO3/maturin wiring, Python API surface, or packaging. A py37-lite or py38-only CI pass is insufficient.
5. **Release/review agents**: verify py37 wheels exist and py37 CI is green before approving any release or merge. The `compatibility` field in SKILL.md frontmatter should include `Python 3.7+`.

If you are uncertain whether a change affects py37 compatibility, ask. Never assume "it probably works on 3.7 too."

## 🚫 Top Traps — Memorize These

1. **`scan_and_load` returns a 2-tuple** → `skills, skipped = scan_and_load(...)`
2. **`success_result` kwargs become context** → `success_result("msg", count=5)` — never `context=`
3. **`ToolDispatcher` uses `.dispatch()`** → never `.call()`
4. **Register ALL handlers BEFORE `server.start()`**
5. **SKILL.md extensions use `metadata.dcc-mcp.<feature>`** → sibling files, never top-level extension keys
6. **Use `dcc_mcp_core.METADATA_*` / `LAYER_*` / `CATEGORY_*`** → re-exported at top level
7. **Gateway wrappers accept only `tool_slug`, `arguments`, `meta`** → backend inputs go inside `arguments`
8. **Return `ToolResult` from Python tool handlers** → `ToolResult.ok("...", **ctx).to_dict()`
9. **Lifecycle hooks: policy events veto, observation events don't** → `BEFORE_*` events propagate `HookDeny`; `AFTER_*` events swallow it
10. **Agent memory: `install()` is mandatory** → `MemoryRecorder` does nothing until wired to `LifecycleHooks` via `.install(hooks)`

## 📖 Further Reading

- **Default entry skill**: [`dcc-mcp`](skills/dcc-mcp/SKILL.md) — load this skill for CLI+REST DCC control
- **CLI reference**: [`docs/guide/cli-reference.md`](docs/guide/cli-reference.md) — full `dcc-mcp-cli` command reference
- **Navigation map**: [`AGENTS.md`](AGENTS.md) — start here for detailed rules
- **API index**: [`llms.txt`](llms.txt) — compressed API reference for AI agents
- **Skill authoring guide**: [`docs/guide/skills.md`](docs/guide/skills.md) — current SKILL.md + sibling-file pattern
- **Skill ownership policy**: [`docs/POLICY_SKILL_OWNERSHIP.md`](docs/POLICY_SKILL_OWNERSHIP.md) — avoid duplicating bundled adapter file-operation skills
- **Bundled examples**: [`examples/skills/`](examples/skills/) — complete SKILL.md packages
- **Detailed traps**: [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)
- **Lifecycle hooks reference**: [`docs/guide/agents-reference.md#lifecycle-hooks-typed-observerpub-sub-1337`](docs/guide/agents-reference.md#lifecycle-hooks-typed-observerpub-sub-1337)
- **Agent memory reference**: [`docs/guide/agents-reference.md#agent-memory-three-tier-1334`](docs/guide/agents-reference.md#agent-memory-three-tier-1334)

## 💡 Pro Tips for AI Agents

1. **CLI+REST is your default path** — load `dcc-mcp` skill and use `dcc-mcp-cli search/describe/call`. Only fall back to MCP when running inside an IDE.
2. **Always search before assuming** — use `dcc-mcp-cli search --query "..." --dcc-type ...` or `search_skills()` to discover relevant tools
3. **Read tool annotations** — respect safety hints (`read_only`, `destructive`)
4. **Follow next-tools chains** — they guide you through complex workflows
5. **Handle errors gracefully** — check `error_result` and follow `prompt` suggestions
6. **Use progressive loading** — don't load all skills at once, activate groups as needed
7. **Prefer structured skill tools over raw scripting** — they provide validation, safety, and traceability
8. **Check cancellations** — in long-running tools, periodically call `check_cancelled()`
9. **Choose by `jobStrategy`** — `chunked` means bounded host ticks,
   `isolated` means a durable external job, and absent/`monolithic` means an
   indivisible call. Never infer that arbitrary script code is splittable.
10. **Recover before retrying** — retain async `job_id`; after transport loss,
    rediscover the instance and call its `jobs_get_status`. If the DCC crashed,
    wait for a replacement instance, then query persisted status and treat
    `interrupted` as terminal unless the tool exposes an explicit resume path.
11. **Wire lifecycle hooks for policy control** — use `BEFORE_TOOL_CALL` + `HookDeny` to block dangerous operations without modifying tool code
12. **Enable agent memory for smarter searches** — `MemoryRecorder` auto-injects `memory_prefer_tools`/`memory_avoid_tools` so search ranking improves over time
13. **Use `register_all_builtin_skills` for a complete baseline** — one call registers diagnostics, introspection, feedback, recipes, UI inspector, and script materialization tools
14. **Read `_meta` for request-level context** — tools receive `params._meta.agent_context` (caller identity), `credential_profile` (env tier), `permission_hint` (read-only/read-write), and `project_scope` (data isolation). See [agents-reference.md](docs/guide/agents-reference.md#request-level-context-passthrough-_meta----pip-520) for patterns.

---

**Remember**: When in doubt, read `AGENTS.md` → `docs/guide/agents-reference.md` → `llms.txt`. The documentation hierarchy is designed for progressive disclosure.
