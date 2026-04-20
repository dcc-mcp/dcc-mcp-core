# GEMINI.md — dcc-mcp-core Instructions for Gemini

> **Purpose**: Gemini-specific instructions. **Read `AGENTS.md` first** for full project context,
> architecture, commands, and pitfalls. This file adds only Gemini-specific guidance.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC
(DDigital Content Creation) applications. Python package: `dcc_mcp_core`. ~154 public symbols,
zero runtime Python dependencies (everything compiled into Rust core via PyO3), plus pure-Python
helpers (DccServerBase, DccGatewayElection, DccSkillHotReloader, factory, skill helpers, WebViewAdapter).

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
3. **`python/dcc_mcp_core/__init__.py`** — Complete public API (ground truth for imports)
4. **`python/dcc_mcp_core/_core.pyi`** — Type stubs (parameter names, signatures, docstrings)
5. **`llms-full.txt`** — Comprehensive API reference with copy-paste examples
6. **`docs/guide/`** + **`docs/api/`** — Conceptual guides and per-module API docs
7. **`tests/`** — Usage examples in executable test form

Gemini tip: Use the `llms-full.txt` file as your primary API reference — it has copy-paste examples
for every API area. The file is structured with a **Quick Decision Guide** table at the top.

## Essential Commands

```bash
vx just dev          # Build + install dev wheel (required before running Python tests)
vx just test         # Run all Python tests
vx just test-rust    # Run all Rust unit tests
vx just lint         # Full lint: clippy + fmt-check + ruff
vx just preflight    # Pre-commit check: check + clippy + fmt + test-rust
vx just lint-fix     # Auto-fix all lint issues
```

## Key Architecture Facts

- **14 Rust crates** under `crates/`, compiled into `dcc_mcp_core._core` native extension + pure-Python helpers
- **~154 public Python symbols** exported from `python/dcc_mcp_core/__init__.py`
- **Zero runtime Python deps** — all logic in Rust, no `dependencies = [...]` in pyproject.toml
- Python 3.7–3.13 supported (abi3-py38 wheel; separate non-abi3 wheel for 3.7)
- **Version: current** — managed by Release Please, never manually bump

## Gemini-Specific Workflows

### Exploring the API

```python
# What's available?
import dcc_mcp_core
print(dir(dcc_mcp_core))  # all ~140 symbols

# Parameter signatures — read _core.pyi
# grep equivalent:
# python/dcc_mcp_core/_core.pyi: has full class/function docstrings + type annotations
```

### Finding Rust Implementation

```bash
# Find the Rust struct behind a Python class
grep -rn "struct ToolRegistry\|pyclass.*ToolRegistry" crates/ --include="*.rs"

# Find PyO3 bindings
grep -rn "#\[pymethods\]" crates/dcc-mcp-actions/src/ --include="*.rs"

# Find where a Python symbol is registered
grep -n "ToolRegistry" src/lib.rs
```

### When Adding a New Python-Accessible Symbol

1. Implement in the appropriate `crates/dcc-mcp-*/src/` Rust crate
2. Add `#[pyclass]` / `#[pymethods]` in the crate's `python.rs`
3. Register in `src/lib.rs` in the `register_*()` function
4. Re-export in `python/dcc_mcp_core/__init__.py` (import + `__all__`)
5. Add to `python/dcc_mcp_core/_core.pyi` stubs
6. Add pytest tests in `tests/test_<module>.py`

### When Writing Tests

```python
from __future__ import annotations
import pytest
from dcc_mcp_core import success_result, error_result, ToolResult

def test_result_creation():
    r = success_result("done", prompt="next step hint", count=5)
    assert r.success
    assert r.context["count"] == 5
    assert r.prompt == "next step hint"

def test_skill_scan(tmp_path):
    skill_dir = tmp_path / "my-skill"
    (skill_dir / "scripts").mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text("---\nname: my-skill\ndcc: python\n---\n")
    (skill_dir / "scripts" / "do_thing.py").write_text("print('hello')")

    from dcc_mcp_core import parse_skill_md
    meta = parse_skill_md(str(skill_dir))
    assert meta is not None
    assert meta.name == "my-skill"
    assert len(meta.scripts) == 1
```

### Understanding the Skills Pipeline

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

Action naming: `{skill_name}__{script_stem}` (hyphens → underscores, `__` separator)

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

# McpHttpServer — expose registry over HTTP/MCP
server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()   # McpServerHandle
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"
```

### On-Demand Skill Discovery (MCP HTTP)

`tools/list` returns three tiers:
1. **6 core tools** (always): `find_skills`, `list_skills`, `get_skill_info`, `load_skill`, `unload_skill`, `search_skills`
2. **Loaded skill tools** — full `input_schema` from ToolRegistry
3. **Unloaded skill stubs** — `__skill__<name>` with one-line description only

Workflow: `search_skills(query="keyword")` → `load_skill("skill-name")` → use tools

SKILL.md `search-hint` field improves keyword matching:
```yaml
search-hint: "polygon modeling, bevel, extrude, mesh editing"
```

## Common Pitfalls

1. **Import from public API only**: `from dcc_mcp_core import X` — never `from dcc_mcp_core._core import X` (except `DeferredExecutor`)
2. **No manual version bumps**: Release Please owns `CHANGELOG.md` and version strings
3. **No runtime Python deps**: Never add to `dependencies` in `pyproject.toml`
4. **Rust changes need Python updates**: Modify `python.rs` → `src/lib.rs` → `__init__.py` → `_core.pyi`
5. **Build before testing**: `vx just dev` before `vx just test`
6. **Use vx prefix**: `vx just test` not `pytest`, `vx just lint` not `ruff check`
7. **Legacy APIs removed in v0.12+**: `ActionManager`, `Action` base class, `create_action_manager()`, `MiddlewareChain`
8. **scan_and_load returns tuple**: `(List[SkillMetadata], List[str])` — unpack both — `skills = scan_and_load(...)` is WRONG
9. **`_core.pyi` is authoritative**: When unsure of param names/types, read stubs first
10. **`.agents/` is gitignored**: Use `git add -f` for files there
11. **`ToolDispatcher` takes ONE arg**: `ToolDispatcher(registry)` — no `validator=` param; method is `.dispatch(name, json_str)` not `.call()`
12. **`success_result` kwargs → context**: `success_result("msg", count=5)` → `context={"count":5}` — do NOT use `context=` keyword
13. **`error_result` positional args**: `error_result("msg", "error string")` — not `error_result(message=..., error=...)`
14. **`EventBus.subscribe` returns int**: Store the return value to unsubscribe later: `sub_id = bus.subscribe(...)`
15. **`FramedChannel.call()` IS available** (v0.12.7+): Use `channel.call(method, params_bytes, timeout_ms)` for synchronous RPC. Use `send_request()` + `recv()` only for async/multiplexed patterns.
16. **`IpcListener.bind(addr)`** creates listener (static method); `.accept()` blocks until client connects. There is no `.new()` or `.start()` method.
17. **`McpServerHandle` is an alias**: `server.start()` returns `McpServerHandle`; it is re-exported as `McpServerHandle` in `__init__.py`. Import as `from dcc_mcp_core import McpServerHandle`.
18. **`McpHttpServer` registry population**: All actions must be registered in `ToolRegistry` BEFORE calling `server.start()`. The server reads metadata from the registry at startup.

19. **MCP spec version awareness**: `McpHttpServer` implements 2025-03-26 spec (Streamable HTTP, Tool Annotations, OAuth 2.1). The 2026 roadmap focuses on: (1) transport scalability — `.well-known` capability discovery, stateless sessions; (2) agent communication — Tasks lifecycle, retry/expiration; (3) governance — contributor ladder, delegated workgroups; (4) enterprise readiness — audit, SSO, gateway behavior (mostly extensions). No new transport types in 2026. Do NOT implement these manually — wait for the library to add support.

20. **`scan_and_load` keyword args only**: Both `extra_paths` and `dcc_name` must be passed as keyword arguments: `scan_and_load(dcc_name="maya", extra_paths=["/path"])` — never as positionals.

21. **`DeferredExecutor` import path**: `DeferredExecutor` is Rust-backed and must be imported via `from dcc_mcp_core._core import DeferredExecutor` until it is added to the public `__init__.py` exports. Always check `__init__.py` first.

22. **`CompatibilityRouter` not standalone**: Access via `VersionedRegistry.router()`. It borrows the registry and provides constraint-based version resolution. For most use cases, `VersionedRegistry.resolve()` is sufficient.

23. **`external_deps` on SkillMetadata**: A JSON string field for declaring external requirements (MCP servers, env vars, binaries). Set via `md.external_deps = json.dumps(deps)`, read via `json.loads(md.external_deps)`. Returns `None` when not set. See `docs/guide/skill-scopes-policies.md` for the schema.

24. **tools/list has 6 core tools** (not 5): `find_skills`, `list_skills`, `get_skill_info`, `load_skill`, `unload_skill`, **`search_skills``. Unloaded skills appear as `__skill__<name>` stubs — calling a stub returns a `load_skill` hint, not an error about missing handlers.

25. **`search_hint` fallback**: If `search-hint:` is not in SKILL.md, `SkillSummary.search_hint` falls back to `description`. Set `search-hint` explicitly for better keyword matching.

26. **SkillScope & SkillPolicy** (v0.13+): Trust hierarchy `Repo` < `User` < `System` < `Admin`. Higher-scope skills shadow lower-scope ones with the same name. **These are Rust-level types not directly importable from Python.** Configure via SKILL.md frontmatter (`allow_implicit_invocation`, `products`) and access via `SkillMetadata.is_implicit_invocation_allowed()` / `SkillMetadata.matches_product(dcc_name)`.

27. **WebViewAdapter** (Python-only): `from dcc_mcp_core import WebViewAdapter, WebViewContext, CAPABILITY_KEYS, WEBVIEW_DEFAULT_CAPABILITIES` — for embedding browser panels in DCC applications. Not in `_core.pyi`.

28. **`skill_warning()` / `skill_exception()`**: Pure-Python helpers in `skill.py`. `skill_warning()` returns a partial-success dict with warnings; `skill_exception()` wraps exceptions into error dict format.

29. **Action→Tool compatibility** (v0.13): The project renamed "action" → "tool" conceptually. Method names `get_action`, `list_actions`, `search_actions` remain as compatibility aliases — not bugs.

30. **MCP best practices**: Design tools around user workflows, not API calls. Use `ToolAnnotations` for safety hints (`read_only_hint`, `destructive_hint`, `idempotent_hint`). Return human-readable errors. Use `notifications/tools/list_changed` when the tool set changes.

31. **`ActionMeta` is Rust-only**: Do not reference `ActionMeta.enabled` or `ActionMeta.group` in Python code. Use `ToolRegistry.set_tool_enabled(name, enabled)` and `ToolRegistry.list_tools_in_group(skill, group)` instead.

32. **SKILL.md frontmatter fields**: agentskills.io standard (`name` required, `description` required, `license`, `compatibility`, `metadata`, `allowed-tools` experimental) + dcc-mcp-core extensions (`dcc`, `tags`, `search-hint`, `tools`, `groups`, `depends`, `next-tools`). The `allowed-tools` field uses space-separated tool strings like `Bash(git:*) Read`.

33. **`next-tools` field** (dcc-mcp-core extension): Declared per-tool in SKILL.md to guide AI agents to follow-up tools. `on-success` and `on-failure` accept lists of fully-qualified tool names. Not part of agentskills.io spec.

34. **Security**: Use `SandboxPolicy` + `SandboxContext` for AI-driven execution. Validate inputs with `ToolValidator`. Never hardcode secrets in code.

35. **Commit messages**: Use Conventional Commits (`feat:`, `fix:`, `docs:`, etc.). Never manually bump versions.

36. **AI Agent Tool Priority**: When interacting with DCCs, prefer: (1) Skill Discovery (`search_skills` → `load_skill`), (2) Skill-based tools (validated schemas + `next-tools` + `ToolAnnotations`), (3) Diagnostics tools (`diagnostics__screenshot` etc.), (4) Direct registry access (last resort, must validate + sandbox). Skills-first approach provides safety, discoverability, chainability, progressive exposure, and validation.

## CI/CD Summary

- 35 total CI jobs: Rust lint/test (3 platforms) + Python test matrix (Linux/macOS/Windows × py3.7–3.13) + DCC integration tests
- Docs-only changes skip Rust rebuild → CI passes quickly
- Squash merge convention for PRs
- `docs/` prefix in branch names causes `refs/heads/docs/...` conflicts — use flat names like `feat-xxx`
