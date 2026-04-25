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
