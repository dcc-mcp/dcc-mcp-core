# Agents Reference — Detailed Rules and Traps

> This file is the detailed companion to `AGENTS.md`.
> `AGENTS.md` is the navigation map (≤150 lines); this file holds the
> expanded rules, code examples, and traps that agents need on demand.
> Read `AGENTS.md` first, then follow links here when you need detail.

---

## Traps — Detailed Reference

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

**Async `tools/call` dispatch (#318) — opt-in, non-blocking:**
```python
# Any of these routes the call through JobManager and returns immediately
# with {job_id, status: "pending"}:
#   1. Request carries _meta.dcc.async = true
#   2. Request carries _meta.progressToken
#   3. Tool's ActionMeta declares execution: async or timeout_hint_secs > 0
# Otherwise dispatch is synchronous (byte-identical to pre-#318 behaviour).
body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
    "name": "render_frames",
    "arguments": {"start": 1, "end": 250},
    "_meta": {"dcc": {"async": True, "parentJobId": "<uuid-or-null>"}},
}}
# → result.structuredContent = {"job_id": "<uuid>", "status": "pending",
#                               "parent_job_id": "<uuid>|null"}
# Poll via jobs.get_status (#319); cancelling the parent cancels every child
# whose _meta.dcc.parentJobId matches (CancellationToken child-token cascade).
```

**`ToolRegistry.register()` — keyword args only, no positional:**
```python
registry.register(name="my_tool", description="...", dcc="maya")
```

**Tool annotations live in the sibling `tools.yaml`, never at the SKILL.md top level (#344):**
Declare MCP `ToolAnnotations` as a nested `annotations:` map on each
tool entry (or the legacy shorthand flat `*_hint:` keys). Nested map
wins whole-map when both forms are present. `deferred_hint` is a
dcc-mcp-core extension and rides in `_meta["dcc.deferred_hint"]` on
`tools/list` — never inside the spec `annotations` map. Full guide:
`docs/guide/skills.md#declaring-tool-annotations-issue-344`.

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

The loader accepts **both** shapes interchangeably — flat dotted keys
(`dcc-mcp.dcc: maya`) and the nested map produced by `yaml.safe_dump`
and the migration tool:

```yaml
metadata:
  dcc-mcp:
    dcc: maya
    tools: "tools.yaml"
    groups: "groups.yaml"
```

Prefer the nested form for new skills; it round-trips through standard
YAML tooling without per-key quoting.

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

**Return `ToolResult` from Python tool handlers (#487) — never hand-roll the dict:**
```python
from dcc_mcp_core.result_envelope import ToolResult

# ✓ typed envelope; serialises to the same wire shape clients already see.
# Factory methods are `success_` / `error_` (trailing underscore avoids
# shadowing the dataclass fields), with shorter aliases `ok` / `fail`.
return ToolResult.ok("Loaded skill", name=name).to_dict()
return ToolResult.fail("Skill missing", error="not_found",
                       prompt="Try `recipes__list`.").to_dict()
# `ToolResult.not_found("Skill", name)` and `ToolResult.invalid_input(msg)`
# are convenience constructors for the two most common error codes.

# ✗ ad-hoc dict — no field validation, drifts when the wire shape evolves
return {"success": True, "message": "...", "context": {"name": name}}
```
The dataclass mirrors the Rust `ToolResult` model; empty fields are pruned
by `.to_dict()` so feature-flag toggles do not perturb the JSON envelope.

> **Trap (#487):** there is no `ToolResult.success(...)` / `ToolResult.error(...)`
> classmethod — `success` and `error` are *dataclass fields*, so the factories
> are spelled `success_` / `error_` (or the cleaner aliases `ok` / `fail`).
> Calling `ToolResult.success("...")` raises
> `AttributeError: type object 'ToolResult' has no attribute 'success'`.

**Import metadata key strings from `constants.py` (#487):**
```python
from dcc_mcp_core.constants import (
    METADATA_RECIPES_KEY,    # "dcc-mcp.recipes"
    METADATA_LAYER_KEY,      # "dcc-mcp.layer"
    LAYER_THIN_HARNESS,      # "thin-harness"
    CATEGORY_RECIPES,        # "recipes"
)
# ✗ never inline literals — renaming a key now means editing one file
```
Every `"dcc-mcp.<feature>"` metadata key, every `metadata.dcc-mcp.layer` value,
and every `category` tag on `ToolRegistry.register(...)` lives in
`dcc_mcp_core.constants`. Adding a new key? Add it to `constants.py` first,
import it everywhere it appears.

**Connection-scoped tool cache (issue #438):**
`tools/list` caches a per-session snapshot by default (`enable_tool_cache=True`).
The cache is invalidated automatically on skill load/unload and group
activation/deactivation. To force a fresh build for a single request, send
`_meta.dcc.refresh=true` on the `tools/list` call. The cache does **not**
apply to `tools/call` — only to `tools/list` response construction.

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

**`next-tools` — live inside the sibling `tools.yaml`, never top-level SKILL.md (issue #342):**
```yaml
# tools.yaml  (referenced from SKILL.md via metadata.dcc-mcp.tools: tools.yaml)
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya_geometry__bevel_edges]    # suggested after success
      on-failure: [dcc_diagnostics__screenshot]   # debug on failure
```
- `next-tools` is a dcc-mcp-core extension (not in agentskills.io spec)
- Lives inside each tool entry in `tools.yaml`. Top-level `next-tools:` on SKILL.md is legacy, emits a deprecation warn, and flips `is_spec_compliant() → False`.
- Surfaces on `CallToolResult._meta["dcc.next_tools"]` — server attaches `on_success` after success and `on_failure` after error; omitted entirely when not declared.
- Invalid tool names are dropped at load-time with a warn — skill still loads.
- Both `on-success` and `on-failure` accept lists of fully-qualified tool names.

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
- See [Skill Scopes & Policies](/guide/skill-scopes-policies) for the full schema

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

**Workflow step policies — retry / timeout / idempotency (#353):**
```python
from dcc_mcp_core import WorkflowSpec, BackoffKind
spec = WorkflowSpec.from_yaml_str(yaml)
spec.validate()  # idempotency_key template refs checked HERE, not at parse
retry = spec.steps[0].policy.retry
# next_delay_ms is 1-indexed: 1 = initial attempt (returns 0), 2 = first retry
assert retry.next_delay_ms(1) == 0
assert retry.next_delay_ms(2) == retry.initial_delay_ms
# Exponential doubles: attempt n >= 2 → initial * 2^(n-2), clamped to max
```
- `max_attempts == 1` means **no retry** (not "retry once")
- `retry_on: None` = every error retryable; `retry_on: []` = no error retryable
- `idempotency_scope` defaults to `"workflow"` (per-invocation), set `"global"` for cross-invocation
- Template roots must be in `inputs`/`steps`/`item`/`env`, a top-level input key, or a step id — static-checked on `validate()`

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

## Do and Don't — Full Reference

### Do ✅

- Use `create_skill_server("maya", McpHttpConfig(port=8765))` — the Skills-First entry point since v0.12.12
- Use `success_result("msg", count=5)` — extra kwargs become `context` dict
- Use `ToolAnnotations(read_only_hint=True, destructive_hint=False)` — helps AI clients choose safely
- Use `next-tools: on-success/on-failure` in SKILL.md — guides AI agents to follow-up tools
- Use `search-hint:` in SKILL.md — improves `search_skills` keyword matching
- Use tool groups with `default_active: false` for power-user features — keeps `tools/list` small
- **Tag every skill with `metadata.dcc-mcp.layer`** — `infrastructure`, `domain`, or `example`. See `skills/README.md#skill-layering`.
- **Start every skill `description` with the layer prefix** (`Infrastructure skill —` / `Domain skill —` / `Example skill —`) followed by a "Not for X — use Y" negative routing sentence
- **Keep `search-hint` non-overlapping across layers** — infrastructure: mechanism-oriented; domain: intent-oriented; example: append "authoring reference"
- **Wire every domain skill tool `on-failure`** to `[dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]`
- **Declare `depends: [dcc-diagnostics]`** in every domain skill that uses `on-failure` chains
- For every new SKILL.md extension, use a `metadata.dcc-mcp.<feature>` key pointing at a sibling file (see "SKILL.md sibling-file pattern" in Traps). Same rule for `tools`, `groups`, `workflows`, `prompts`, and anything future.
- Unpack `scan_and_load()`: `skills, skipped = scan_and_load(dcc_name="maya")`
- Register ALL handlers BEFORE `McpHttpServer.start()` — the server reads the registry at startup
- Use `SandboxPolicy` + `InputValidator` for AI-driven tool execution
- Use `DccServerBase` as the base class for DCC adapters — skill/lifecycle/gateway inherited
- Use `vx just dev` before `vx just test` — the Rust extension must be compiled first
- Keep `SKILL.md` body under 500 lines / 5000 tokens — move details to `references/`
- Use Conventional Commits for PR titles — `feat:`, `fix:`, `docs:`, `refactor:`
- Use `registry.list_actions()` (shows all) vs `registry.list_actions_enabled()` (active only)
- Start with `search_skills(query)` when looking for a tool — don't guess tool names. `search_skills` accepts `tags`, `dcc`, `scope`, and `limit`; call it with no arguments to browse by trust scope.
- Use `init_file_logging(FileLoggingConfig(...))` for durable logs in multi-gateway setups; call `flush_logs()` to force events to disk immediately
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
- **Don't create a skill without `metadata.dcc-mcp.layer`** — untagged skills cause routing ambiguity as the catalog grows
- **Don't write a domain skill `description` without a "Not for X" sentence** — agents need explicit counter-examples to avoid picking the wrong skill
- **Don't overlap `search-hint` keywords between infrastructure and domain skills** — overlapping keywords make `search_skills()` return ambiguous results
- Don't use removed transport APIs: `FramedChannel`, `connect_ipc()`, `IpcListener`, `TransportManager`, `CircuitBreaker`, `ConnectionPool` — removed in v0.14 (#251). Use `IpcChannelAdapter` / `DccLinkFrame` instead
- Don't add Python runtime dependencies — the project is zero-dep by design
- Don't manually bump versions or edit `CHANGELOG.md` — Release Please handles this
- Don't hardcode API keys, tokens, or passwords — use environment variables
- Don't use `docs/` prefix in branch names — causes `refs/heads/docs/...` conflicts
- Don't hard-code the legacy `<skill>.<action>` prefixed form in `tools/call` — bare names are the default since v0.14.2 (#307)
- Don't reference `ActionMeta.enabled` in Python — use `ToolRegistry.set_tool_enabled()` instead
- Don't use `json.dumps()` on `ToolResult` — use `result.to_json()` or `serialize_result()`
- Don't guess tool names — use `search_skills(query)` to discover the right tool.
- **Don't add a generic `utils` / `common` / `helpers` crate** — every helper has a natural owner (a domain crate, `dcc-mcp-paths`, `dcc-mcp-logging`, or `dcc-mcp-pybridge`). See the Workspace Boundary Rationale section.

---

## Code Style

### Python

- `from __future__ import annotations` — first line of every module
- Import order: future → stdlib → third-party → local (with section comments)
- Formatter: `ruff format` (line length 120, double quotes)
- All public APIs: type annotations + Google-style docstrings

### Rust

- Edition 2024, MSRV 1.85
- `tracing` for logging (no `println!`)
- `thiserror` for error types
- `parking_lot` instead of `std::sync::Mutex`

---

## Writing Tool Descriptions — Style Guide

Every built-in MCP tool description (see `build_core_tools_inner` and
`build_lazy_action_tools` in `crates/dcc-mcp-http/src/handler.rs`) follows
the 3-layer behavioural structure adopted in issue #341: a one-sentence
present-tense "what" summary, a `When to use:` paragraph contrasting the
tool against its siblings (so the agent knows when NOT to pick it), and a
`How to use:` bullet list covering preconditions, common pitfalls, and
follow-up tools. Keep the whole string ≤ 500 chars (MCP clients truncate
long text); if more context is needed, move it to `docs/api/http.md` and
reference the anchor from the description. Per-parameter `description`
fields in the input schema are single clauses ≤ 100 chars. The structural
contract is enforced by `tests/test_tool_descriptions.py`.

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

---

## Security Considerations

- **Sandbox**: Use `SandboxPolicy` + `SandboxContext` for AI-driven tool execution. Never expose unrestricted filesystem or process access.
- **Input validation**: Always validate AI-provided parameters with `ToolValidator.from_schema_json()` before execution.
- **ToolAnnotations**: Signal safety properties (`read_only_hint`, `destructive_hint`, `idempotent_hint`, `open_world_hint`, `deferred_hint`) so AI clients make informed choices.
- **SkillScope**: Trust hierarchy prevents project-local skills from shadowing enterprise-managed ones.
- **Audit log**: `AuditLog` / `AuditMiddleware` provide traceability for all AI-initiated tool calls.
- **No secrets in code**: Never hardcode API keys, tokens, or passwords. Use environment variables or config files outside the repo.

---

## PR Instructions

- **Title format**: Use Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`
- **Scope optional**: `feat(capture): add DXGI backend`
- **Breaking changes**: `feat!: rename action→tool` with footer `BREAKING CHANGE: ...`
- **Squash merge**: PRs are squash-merged — write the final commit message in the PR title.
- **CI must pass**: `vx just preflight` + `vx just test` + `vx just lint` must all be green.
- **No version bumps**: Release Please handles versioning — never manually bump.

---

## Commit Message Guidelines

- Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`
- Scope is optional: `feat(capture): add DXGI backend`
- Breaking changes: `feat!: rename action→tool` with footer `BREAKING CHANGE: ...`
- Version bumps are handled by Release Please — never manually edit `CHANGELOG.md` or version strings

---

## CI & Release

- PRs must pass: `vx just preflight` + `vx just test` + `vx just lint`
- CI matrix: Python 3.7, 3.9, 3.11, 3.13 on Linux / macOS / Windows
- Versioning: Release Please (Conventional Commits) — never manually bump
- PyPI: Trusted Publishing (no tokens)
- Docs-only changes skip Rust rebuild → CI passes quickly
- Squash merge convention for PRs


---

## Workspace Boundary Rationale

The Rust workspace deliberately has **no `utils` / `common` / `helpers`
crate**. This is a hard architectural constraint, not a stylistic
preference: a previous `dcc-mcp-utils` crate accreted five unrelated
concerns (filesystem helpers, file logging, PyO3 bridges, skill-domain
logic, a constants bag) and forced every other crate to transitively pull
`tracing-appender`, `tracing-subscriber`, `time`, `pyo3`, etc. — even pure
data crates like `dcc-mcp-models` and `dcc-mcp-naming`. The Phase 0
re-cut (issues #485, #496, #497, #498) deleted that crate and
redistributed its contents by ownership.

### Where each kind of helper lives

| Helper kind | Crate | Notes |
|-------------|-------|-------|
| Platform directories (`get_config_dir`, `get_data_dir`, `get_cache_dir`, `get_log_dir`) | `dcc-mcp-paths` | Deps limited to `dirs` + `std` — zero PyO3 / tracing |
| `ensure_directory`, `path_to_string` | `dcc-mcp-paths` | Generic FS plumbing only |
| File logging (`init_file_logging`, `FileLoggingConfig`, `RotationPolicy`, rolling writer) | `dcc-mcp-logging` | Depends on `tracing-subscriber` + `tracing-appender`; NEVER imported by base data crates |
| Tracing-subscriber bootstrap (`init_logging`) | `dcc-mcp-logging` | Same |
| `LOG_*` env vars and defaults | `dcc-mcp-logging::constants` | Co-located with the consumer |
| PyO3 ↔ JSON bridges (`json_value_to_pyobject`, `py_any_to_json_value`, `py_dict_to_json_map`) | `dcc-mcp-pybridge` | Feature-gated `python-bindings`; pulled only by crates that actually expose Python |
| PyO3 ↔ YAML bridges (`yaml_dumps`, `yaml_loads`) | `dcc-mcp-pybridge` | Same |
| `BooleanWrapper`, `FloatWrapper`, `unwrap_to_json_value` | `dcc-mcp-pybridge` | Pure PyO3 surface — zero Rust call sites |
| Skill paths (`get_skill_paths_from_env`, `get_user_skills_dir`, `get_team_skills_dir`, `copy_skill_to_user_dir`) | `dcc-mcp-skills::paths` | Owned by the only consumer |
| Skill versioning (`archive_skill_version`, `update_version_manifest`) | `dcc-mcp-skills::versioning` | Domain logic |
| Skill feedback (`record_skill_feedback`, `FeedbackEntry`) | `dcc-mcp-skills::feedback` | Domain logic |
| Skill evolution (`archive_evolved_skill`, `save_evolved_skill_version`) | `dcc-mcp-skills::evolution` | Domain logic |
| `SKILL_*` / `ENV_*_SKILL_*` constants, `SUPPORTED_SCRIPT_EXTENSIONS`, `is_supported_extension`, `MTIME_EPSILON_SECS` | `dcc-mcp-skills::constants` | Co-located with consumer |
| `DEFAULT_DCC`, `DEFAULT_VERSION` | `dcc-mcp-naming` | Co-located with consumer |
| `DEFAULT_MIME_TYPE` | `dcc-mcp-protocols` | Co-located with consumer |
| `DEFAULT_ERROR_TYPE`, `DEFAULT_ERROR_PROMPT`, `DEFAULT_SUCCESS_MESSAGE`, `CTX_KEY_*`, `ACTION_RESULT_KNOWN_KEYS`, `default_schema()` | `dcc-mcp-models` | Co-located with consumer |
| `APP_NAME`, `APP_AUTHOR` | `dcc-mcp-paths::constants` | Used to derive platform dirs |

### Decision rule for new helpers

When you reach for a "tiny shared helper" ask in this order:

1. **Does an existing domain crate consume it?** Put it there. A helper
   used only by `dcc-mcp-skills` belongs in `dcc-mcp-skills`, even if it
   is "generic-looking".
2. **Is it a platform-dir or pathbuf helper used by ≥2 unrelated crates?**
   Put it in `dcc-mcp-paths`.
3. **Is it a logging concern?** Put it in `dcc-mcp-logging`.
4. **Is it PyO3 conversion plumbing?** Put it in `dcc-mcp-pybridge`
   under `feature = "python-bindings"`.
5. **None of the above?** Inline it at the call site. Do not create a
   new utility module just to share three lines of code, and never
   resurrect a `utils` / `common` crate.

### Compile-time invariants

- `cargo tree -p dcc-mcp-models --no-default-features` MUST NOT list
  `tracing-appender`, `tracing-subscriber`, or `pyo3`.
- `cargo tree -p dcc-mcp-naming` and `cargo tree -p dcc-mcp-protocols`
  MUST stay at the same dep-count baseline as `dcc-mcp-models`.
- The top-level `dcc-mcp-core` crate is the only place that re-exports
  PyO3 symbols across crate boundaries; every other crate uses the
  `python-bindings` feature gate locally.

---

## Project-Specific Architecture & Constraints

This section collects the runtime invariants and config-knob details that
agents must respect when modifying core subsystems. They are derived from
shipped issue resolutions and MUST NOT regress.

### Skills Pipeline (end-to-end flow)

```
DCC_MCP_SKILL_PATHS env var
        ↓
  SkillScanner.scan()           # discovers directories with SKILL.md
        ↓
  parse_skill_md(dir)           # parses YAML frontmatter + enumerates scripts/
        ↓
  resolve_dependencies(skills)  # topological sort by 'depends' field
        ↓
  SkillCatalog.load_skill(name) # on-demand: registers actions into ToolRegistry
        ↓
  ToolDefinition(...)           # expose as MCP tool to LLM
```

Action naming: `{skill_name}__{script_stem}` (hyphens → underscores, `__` separator).

`tools/list` returns three tiers:
1. **Core tools** (always): `list_skills`, `get_skill_info`, `load_skill`, `unload_skill`, `search_skills`
2. **Loaded skill tools** — full `input_schema` from `ToolRegistry`
3. **Unloaded skill stubs** — `__skill__<name>` with one-line description only

Workflow: `search_skills(query="keyword")` → `load_skill("skill-name")` → use tools.
Calling a stub returns a `load_skill` hint, not a missing-handler error.

### Bundled Skills

Two core skills ship inside the wheel under `dcc_mcp_core/skills/`:
`dcc-diagnostics`, `workflow`.

```python
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths
paths = get_bundled_skill_paths()       # [".../dcc_mcp_core/skills"]
paths = get_bundled_skill_paths(False)  # [] — opt-out
```

DCC adapters include these by default (`include_bundled=True`).

### DCC Integration Architectures

`skills/integration-guide.md` covers three patterns:

- **Embedded Python** (`DccServerBase`) — Maya, Blender, Houdini, Unreal
- **WebSocket Bridge** (`DccBridge`) — Photoshop, ZBrush, Unity, After Effects
- **WebView Host** (`WebViewAdapter`) — AuroraView, Electron panels

### MCP HTTP Server Spawn Modes (issue #303)

`McpHttpConfig.spawn_mode` picks how listeners are driven:

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

### Gateway Lifecycle Invariants (issue #303)

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

### Gateway Reliability + Security Defaults (issues #551–#558)

After the v0.14.18 reliability batch, four invariants protect the
gateway from stale or hostile FileRegistry state:

1. **Heartbeat writes are atomic.** `FileRegistry::heartbeat` serialises
   to a sibling tempfile and uses `tempfile::NamedTempFile::persist`
   (atomic rename on POSIX, `MoveFileExW` on Windows). Concurrent
   processes can never produce a half-written entry. On Windows, an
   advisory `LockFileEx`/`UnlockFileEx` cycle around `persist` prevents
   two writers from racing the rename. Do not bypass the helper — direct
   `fs::write` would re-introduce the stomp window.
2. **Dead instances are evicted by active probe, not just by TTL.** The
   gateway runtime spawns a TCP probe loop (`tasks.rs::health_check_handle`)
   that connects to each backend's listener every
   `health_check_interval` (default 10 s); after
   `health_check_max_failures` consecutive misses (default 3) the entry
   is `deregister`-ed. The same probe runs once at startup so an entry
   left behind by a crashed process disappears within the first cycle.
3. **`allow_unknown_tools` defaults to `false`.** The `tools/list`
   aggregator drops any backend whose `dcc_type` is not in the
   gateway-side known-DCC registry. This blocks a hijacked or typo'd
   FileRegistry entry from injecting tools the user never asked for.
   Tests/local development that need to surface a brand-new DCC must
   flip `McpHttpConfig.allow_unknown_tools = true` explicitly.
4. **File logging has sane defaults.** New deployments should use
   `default_file_logging_config()` instead of hand-rolling a
   `FileLoggingConfig` — it picks the platform log directory and a
   daily rotation policy. Pair it with `prune_old_logs(retention_days,
   max_total_size_mb)` (call from a `tokio::spawn` ticker or at
   process startup) to enforce both age- and size-based retention so
   long-lived gateways don't fill the disk.

### Gateway Prometheus Metrics (issue #559)

`/metrics` is **off by default**. To turn it on, build any consumer of
`dcc-mcp-http` with the `prometheus` feature
(`cargo add dcc-mcp-http --features prometheus`). With the feature on:

- `gateway::tasks::start_gateway_runtime` calls
  `super::metrics::attach_gateway_metrics_route(router)` to mount
  `GET /metrics` on the same axum `Router<()>` that serves MCP traffic
  — the helper takes an `Arc<PrometheusExporter>` closure so it does
  **not** change the router's `S` (state) type, which keeps it
  compatible with the rest of the gateway stack.
- A 5 s background task refreshes
  `dcc_mcp_instances_total{status="active"|"stale"}` from a
  `FileRegistry` snapshot. Other gauges (`dcc_mcp_tools_total`,
  `dcc_mcp_request_duration_seconds`, `dcc_mcp_requests_failed_total`)
  live on `dcc_mcp_telemetry::PrometheusExporter` and are intended for
  middleware to update on every request.

When you add new gauges, put the metric definition in
`crates/dcc-mcp-telemetry/src/prometheus.rs` (so non-gateway consumers
can reuse it) and the wiring in
`crates/dcc-mcp-http/src/gateway/metrics.rs` (so it stays behind the
`prometheus` cfg gate).

### Gateway Async-Dispatch + Wait-For-Terminal (issue #321)

The gateway now uses three per-request timeouts instead of one:

- **Sync call** (no `_meta.dcc.async`, no `progressToken`): governed by
  `McpHttpConfig.backend_timeout_ms` (default 10 s, #314).
- **Async opt-in** (`_meta.dcc.async=true` *or* `_meta.progressToken`
  present): governed by
  `McpHttpConfig.gateway_async_dispatch_timeout_ms` (default 60 s).
  Only the **queuing** step spends this budget — the backend replies
  with `{status:"pending", job_id:"…"}` once the job is enqueued.
- **Wait-for-terminal** (`_meta.dcc.wait_for_terminal=true` *and* an
  async opt-in): the gateway blocks the `tools/call` response until
  `$/dcc.jobUpdated` reports a terminal status (`completed` / `failed`
  / `cancelled` / `interrupted`). Governed by
  `McpHttpConfig.gateway_wait_terminal_timeout_ms` (default 10 min).
  On timeout, the response is the last-known envelope annotated with
  `_meta.dcc.timed_out = true`; the job keeps running on the backend.

```python
from dcc_mcp_core import McpHttpConfig
cfg = McpHttpConfig(
    port=8765,
    gateway_async_dispatch_timeout_ms=60_000,   # queuing budget
    gateway_wait_terminal_timeout_ms=600_000,   # wait-for-terminal budget
)
```

Wire-level contract:

```jsonc
// POST /mcp — client request
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
  "name":"maya__bake_simulation","arguments":{...},
  "_meta":{"dcc":{"async":true,"wait_for_terminal":true}}
}}
// Gateway blocks the response until $/dcc.jobUpdated status=terminal;
// wait_for_terminal is STRIPPED before forwarding to the backend so
// the backend contract remains unchanged.
```

Implementation notes for maintainers:

- Detection helpers live in `crates/dcc-mcp-http/src/gateway/aggregator.rs`
  (`meta_signals_async_dispatch`, `meta_wants_wait_for_terminal`,
  `strip_gateway_meta_flags`).
- The per-job broadcast bus is owned by `SubscriberManager`
  (`job_event_buses`, `job_event_channel`, `publish_job_event`,
  `forget_job_bus`). The bus is created **before** the outbound
  `tools/call` so terminal events arriving in the tiny window between
  the backend reply and the waiter installing its subscription are
  not lost.
- Backend disconnect during a wait surfaces as `-32000 backend
  disconnected` and the job stays in whatever state on the backend
  (may later become `interrupted` per #328).

### Workflow Execution Pipeline (issue #348)

`dcc-mcp-workflow` ships the full execution engine. Pipeline sketch:

```
WorkflowExecutor::run(spec, inputs, parent_job)
   → validate spec
   → create root job + CancellationToken
   → spawn tokio driver
      → drive(steps) sequentially
         → per step: retry + timeout + idempotency_key short-circuit
            → dispatch by StepKind:
               ├─ Tool        → ToolCaller::call
               ├─ ToolRemote  → RemoteCaller::call (via gateway)
               ├─ Foreach     → JSONPath items → drive(body) per item
               ├─ Parallel    → tokio::join! branches (on_any_fail)
               ├─ Approve     → ApprovalGate::wait_handle + timeout
               └─ Branch      → JSONPath cond → then | else
            → artefact handoff (FileRef → ArtefactStore)
            → emit $/dcc.workflowUpdated (enter / exit)
            → sqlite upsert (if job-persist-sqlite)
      → emit workflow_terminal
   → return WorkflowRunHandle { workflow_id, root_job_id, cancel_token, join }
```

Use `WorkflowHost` as the stable entry point — it wraps `WorkflowExecutor`
with a run registry keyed by `workflow_id`, so the three mutating MCP
tools (`workflows.run` / `workflows.get_status` / `workflows.cancel`)
can be wired with `register_workflow_handlers(&dispatcher, &host)` after
`register_builtin_workflow_tools(&registry)` has been called.

Key invariants:

1. **Every transition emits `$/dcc.workflowUpdated`.** If you add a
   new state, route it through `RunState::emit`.
2. **Cancellation cascades through `tokio_util::sync::CancellationToken`.**
   Never spawn a step future that drops the token — always pass it into
   every `ToolCaller::call` / `RemoteCaller::call` / `tokio::select!`.
3. **Idempotency short-circuit happens *before* retry attempts.** A
   cache hit skips the step entirely; retries only guard live calls.
4. **SQLite recovery flips non-terminal rows to `interrupted` — never
   auto-resumes.** Resume is explicit opt-in via a separate tool.
5. **Approve gates block on `notifications/$/dcc.approveResponse`.**
   The HTTP handler for that notification calls
   `ApprovalGate::resolve(workflow_id, step_id, response)`.

### Artefact Hand-Off (issue #349)

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

Rust side: `dcc_mcp_artefact::{FilesystemArtefactStore, InMemoryArtefactStore,
ArtefactStore, ArtefactBody, ArtefactFilter, put_bytes, put_file, resolve}`.
`FilesystemArtefactStore` persists at `<root>/<sha256>.bin` + `.json`.

### Resources Primitive (issue #350)

`McpHttpConfig.enable_resources` defaults to `True`. Built-in URIs:

- `scene://current` — JSON; update via `server.resources().set_scene(...)` in Rust.
- `capture://current_window` — PNG blob; Windows HWND `PrintWindow` backend only.
- `audit://recent?limit=N` — JSON; wire via `server.resources().wire_audit_log(log)` in Rust.
- `artefact://sha256/<hex>` — content-addressed artefact (#349); toggle via `enable_artefact_resources`.

```python
cfg = McpHttpConfig(port=8765)
cfg.enable_resources = True            # advertise capability + built-ins
cfg.enable_artefact_resources = False  # default: artefact:// returns JSON-RPC -32002
```

### Prompts Primitive (issues #351, #355)

`McpHttpConfig.enable_prompts` defaults to `True`. Prompts come from each
loaded skill's sibling file referenced by `metadata["dcc-mcp.prompts"]` —
either a single `prompts.yaml` (top-level `prompts:` + `workflows:` lists)
or a `prompts/*.prompt.yaml` glob. Workflows referenced by the spec
auto-generate a summary prompt.

Template engine is minimal: only `{{arg_name}}` substitution; missing
required args return JSON-RPC `INVALID_PARAMS`.
`notifications/prompts/list_changed` fires on skill load / unload.

### Job Lifecycle Notifications (issue #326)

Every `tools/call` emits SSE frames:

- `notifications/progress` — when `_meta.progressToken` is set.
- `notifications/$/dcc.jobUpdated` — gated by `enable_job_notifications` (default `True`).
- `notifications/$/dcc.workflowUpdated` — same gate; #348 executor populates it.

```python
cfg = McpHttpConfig(port=8765)
cfg.enable_job_notifications = False  # opt the $/dcc.* channels out
```

Polling fallback: **`jobs.get_status`** (#319, always registered) returns
the full job-state envelope for a given `job_id`. Use **`jobs.cleanup`**
(#328) with `older_than_hours` to prune terminal jobs; combine with
`McpHttpConfig.job_storage_path` + Cargo feature `job-persist-sqlite`
for restart-safe job history (pending/running rows become `Interrupted`
on reboot).

### Scheduler (issue #352)

Opt in with Cargo feature `scheduler`.

```python
from dcc_mcp_core import (
    ScheduleSpec, TriggerSpec, parse_schedules_yaml,
    hmac_sha256_hex, verify_hub_signature_256,
)
cfg = McpHttpConfig(port=8765)
cfg.enable_scheduler = True
cfg.schedules_dir = "/opt/dcc-mcp/schedules"   # loads *.schedules.yaml
```

`ScheduleSpec` / `TriggerSpec` are declarative; the `SchedulerService`
runtime is driven from Rust. Schedules live in sibling
`schedules.yaml` files (never embedded in `SKILL.md` frontmatter —
follow the #356 sibling-file pattern). Cron format is 6-field:
`"sec min hour day month weekday"`. Webhook HMAC-SHA256 via
`X-Hub-Signature-256`; secret read from `secret_env` at startup.
On terminal workflow status, host calls
`SchedulerHandle::mark_terminal(schedule_id)` to release `max_concurrent`.

### Prometheus `/metrics` Exporter (issue #331)

Opt-in behind the `prometheus` Cargo feature — **off by default**.
When compiled in, enable at runtime via
`McpHttpConfig(enable_prometheus=True, prometheus_basic_auth=(u, p))`.
Metric names live in [`docs/api/observability.md`](../api/observability.md);
see there for Grafana PromQL examples. Counters advance from the
`tools/call` wrapper in `handler.rs` — do not add recording sites
elsewhere.

---

## Rust Extension Points (post-EPIC #495)

Five trait-shaped extension points landed during the EPIC #495 architecture
audit. Each follows the same recipe: **"add a behaviour without editing the
upstream `match` table."** All are Rust-only; they live below the PyO3 layer.

### `MethodHandler` + `MethodRouter` — custom JSON-RPC methods (#492)

Crate: `dcc-mcp-http`, module `handler::router`.

```rust
use std::sync::Arc;
use dcc_mcp_http::handler::{MethodRouter, MethodHandler, HandlerFuture};
use dcc_mcp_http::handler::state::AppState;
use dcc_mcp_http::protocol::{JsonRpcRequest, JsonRpcResponse};
use dcc_mcp_http::error::HttpError;

struct PingHandler;
impl MethodHandler for PingHandler {
    fn handle<'a>(
        &'a self,
        _state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            Ok(JsonRpcResponse::success(req.id.clone(), serde_json::json!("pong")))
        })
    }
}

let router = MethodRouter::with_builtins();   // initializes, prompts, ...
router.register("ping", Arc::new(PingHandler));
// hand `router` to `AppState::with_method_router(...)`
```

Capability gating (`enable_resources`, `enable_prompts`) lives in the handler
itself — return `HttpError::method_not_found(...)` when a feature is off, never
add another arm to the dispatcher. Closures that match the
`Fn(&AppState, &JsonRpcRequest, Option<&str>) -> HandlerFuture` shape implement
`MethodHandler` automatically; reach for a struct only when you need state.

### `Registry<V>` + `RegistryEntry` — registry-shaped containers (#489)

Crate: `dcc-mcp-models`, module `registry`.

`ActionRegistry`, `SkillCatalog`, and `WorkflowCatalog` all `impl Registry<V>`
over their existing storage (per-DCC `DashMap`, file-hash `DashMap`, ordered
`RwLock<Vec>`). New registries that need only the contract — not specialised
indexes — can use `DefaultRegistry<V>` directly.

The shared contract test lives in `dcc_mcp_models::registry::testing::assert_registry_contract`
behind the `testing` feature flag; every implementor calls it once with a
fixture so register / get / list / remove / count / search semantics stay in
lockstep.

### `ValidationStrategy` + `select_strategy` — pluggable action validation (#493)

Crate: `dcc-mcp-actions`, module `validation_strategy`.

Built-ins: `NoOpValidator` (no metadata / empty schema) and
`SchemaValidator<'_>` (borrowed-meta JSON Schema check). `ActionDispatcher::dispatch`
calls `select_strategy(meta, skip_empty_schema_validation)` to pick one per call;
adding a new flavour (cached compiled schemas, sandbox precheck, contract-test
mode) means a new `impl ValidationStrategy` and one extra arm in
`select_strategy` — `dispatch()` is unaffected. The trait returns
`ValidationOutcome { skipped: bool }` so the dispatcher can record metrics
without re-deriving "did this actually run?".

### `VersionMatcher` — pluggable version-constraint shapes (#493)

Crate: `dcc-mcp-actions`, module `versioned::matcher`.

Built-in matchers (one per `VersionConstraint` variant): `AnyMatcher`,
`ExactMatcher`, `AtLeastMatcher`, `GreaterThanMatcher`, `AtMostMatcher`,
`LessThanMatcher`, `CaretMatcher`, `TildeMatcher`. Both
`VersionConstraint::matches(version)` and `Display::fmt` route through
`VersionConstraint::with_matcher(...)`, so adding a new constraint shape
takes exactly three edits, none of them in caller code:

1. one new `VersionConstraint` enum variant in `versioned/mod.rs`,
2. a new matcher struct + `impl VersionMatcher` in `versioned/matcher.rs`,
3. one extra arm in `with_matcher`.

`matches()` and `Display::fmt` need no edits at all.

### `NotificationBuilder` + `JsonRpcRequestBuilder` — JSON-RPC envelope construction (#484)

Crate: `dcc-mcp-http`, module `protocol::notification_builder`.

Six call sites previously hand-rolled
`json!({"jsonrpc":"2.0","method":..,"params":..})`. The builders are now the
single source of truth for that wire shape:

```rust
use dcc_mcp_http::protocol::NotificationBuilder;

let sse_frame = NotificationBuilder::new("notifications/tools/list_changed")
    .with_params(serde_json::json!({}))
    .as_sse_event();   // ready to push onto the per-session stream
```

`.build()` returns a typed `JsonRpcNotification`; `.to_value()` returns the raw
`serde_json::Value`. `JsonRpcRequestBuilder` is the symmetric helper for
*requests* (gateway backend client) — it owns the `id` field.

### `DccName` — typed DCC identifier (#491)

Crate: `dcc-mcp-models`.

`DccName::parse("Maya")` → `DccName::Maya`; case-insensitive aliases (`"3dsmax"`,
`"max"`, `"threedsmax"` all map to `ThreedsMax`). Round-trips through
`serde_json::to_value(...)` ↔ `serde_json::from_value(...)` losslessly via the
`#[serde(from = "String", into = "String")]` annotation. Unknown values become
`DccName::Other(String)` so the enum can grow without breaking external
callers. Aliases live in `DccName::parse(...)` itself: `"3dsmax"`, `"max"`,
and `"threedsmax"` all map to `DccName::ThreedsMax`; `"c4d"` and `"cinema4d"`
to `DccName::Cinema4d`; `"photoshop"` and `"ps"` to `DccName::Photoshop`.
Use the type at every new Rust API boundary that previously would have taken
`&str`; existing call sites such as `ActionRegistry::list_actions_for_dcc(&str)`
remain `&str` for backward compat and can be migrated lazily.

### `DccMcpError` — unified workspace error (#488)

Crate: `dcc-mcp-models`.

A single error enum with `From<HttpError>`, `From<ProcessError>`, … impls.
Crates keep their domain-specific enums (`HttpError`, `ProcessError`, …) and
convert to `DccMcpError` at the public boundary. New top-level helpers should
return `Result<T, DccMcpError>` rather than introducing yet another error type.
