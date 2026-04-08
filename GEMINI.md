# GEMINI.md — dcc-mcp-core Instructions for Gemini

> **Purpose**: Gemini-specific instructions. Complements AGENTS.md with Gemini-specific guidance.
> Read AGENTS.md first for full project context, then this file.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC
(Digital Content Creation) applications. Python package: `dcc_mcp_core`. ~120 public symbols,
zero runtime Python dependencies (everything compiled into Rust core via PyO3).

## Priority Reading Order

1. `python/dcc_mcp_core/__init__.py` — Complete public API (ground truth for imports)
2. `python/dcc_mcp_core/_core.pyi` — Type stubs (parameter names, signatures, docstrings)
3. `AGENTS.md` — Architecture, commands, pitfalls, AI integration patterns
4. `llms-full.txt` — Comprehensive API reference with copy-paste examples
5. `tests/` — Usage examples in executable test form

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

- **12 Rust crates** under `crates/`, compiled into `dcc_mcp_core._core` native extension
- **~130 public Python symbols** exported from `python/dcc_mcp_core/__init__.py`
- **Zero runtime Python deps** — all logic in Rust, no `dependencies = [...]` in pyproject.toml
- Python 3.7–3.13 supported (abi3-py38 wheel; separate non-abi3 wheel for 3.7)
- Version: **0.12.9** — managed by Release Please, never manually bump

## Gemini-Specific Workflows

### Exploring the API

```python
# What's available?
import dcc_mcp_core
print(dir(dcc_mcp_core))  # all ~120 symbols

# Parameter signatures — read _core.pyi
# grep equivalent:
# python/dcc_mcp_core/_core.pyi: has full class/function docstrings + type annotations
```

Gemini tip: Use the `llms-full.txt` file as your primary reference — it has copy-paste examples
for every API area. The file is structured with a **Quick Decision Guide** table at the top.

### Finding Rust Implementation

```bash
# Find the Rust struct behind a Python class
grep -rn "struct ActionRegistry\|pyclass.*ActionRegistry" crates/ --include="*.rs"

# Find PyO3 bindings
grep -rn "#\[pymethods\]" crates/dcc-mcp-actions/src/ --include="*.rs"

# Find where a Python symbol is registered
grep -n "ActionRegistry" src/lib.rs
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
from dcc_mcp_core import success_result, error_result, ActionResultModel

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
  ActionRegistry.register()     # register each script as an action
        ↓
  ToolDefinition(...)           # expose as MCP tool to LLM
```

Action naming: `{skill_name}__{script_stem}` (hyphens → underscores, `__` separator)

## Common Pitfalls

1. **Import from public API only**: `from dcc_mcp_core import X` — never `from dcc_mcp_core._core import X`
2. **No manual version bumps**: Release Please owns `CHANGELOG.md` and version strings
3. **No runtime Python deps**: Never add to `dependencies` in `pyproject.toml`
4. **Rust changes need Python updates**: Modify `python.rs` → `src/lib.rs` → `__init__.py` → `_core.pyi`
5. **Build before testing**: `vx just dev` before `vx just test`
6. **Use vx prefix**: `vx just test` not `pytest`, `vx just lint` not `ruff check`
7. **Legacy APIs removed in v0.12+**: `ActionManager`, `Action` base class, `create_action_manager()`, `MiddlewareChain`
8. **scan_and_load returns tuple**: `(List[SkillMetadata], List[str])` — unpack both — `skills = scan_and_load(...)` is WRONG
9. **`_core.pyi` is authoritative**: When unsure of param names/types, read stubs first
10. **`.agents/` is gitignored**: Use `git add -f` for files there
11. **`ActionDispatcher` takes ONE arg**: `ActionDispatcher(registry)` — no `validator=` param; method is `.dispatch(name, json_str)` not `.call()`
12. **`success_result` kwargs → context**: `success_result("msg", count=5)` → `context={"count":5}` — do NOT use `context=` keyword
13. **`error_result` positional args**: `error_result("msg", "error string")` — not `error_result(message=..., error=...)`
14. **`EventBus.subscribe` returns int**: Store the return value to unsubscribe later: `sub_id = bus.subscribe(...)`
15. **`FramedChannel.call()` IS available** (v0.12.7+): Use `channel.call(method, params_bytes, timeout_ms)` for synchronous RPC. Use `send_request()` + `recv()` only for async/multiplexed patterns.
16. **`IpcListener.bind(addr)`** creates listener (static method); `.accept()` blocks until client connects. There is no `.new()` or `.start()` method.
17. **`McpServerHandle` is an alias**: `server.start()` returns `ServerHandle`; it is re-exported as `McpServerHandle` in `__init__.py`. Import as `from dcc_mcp_core import McpServerHandle`.
18. **`McpHttpServer` registry population**: All actions must be registered in `ActionRegistry` BEFORE calling `server.start()`. The server reads metadata from the registry at startup.

19. **MCP spec version awareness**: `McpHttpServer` implements 2025-03-26 spec. The 2025-11-05 draft adds JSON-RPC batching and resource links in tool results. Do NOT implement these manually — wait for the library to add support.

20. **`scan_and_load` keyword args only**: Both `extra_paths` and `dcc_name` must be passed as keyword arguments: `scan_and_load(dcc_name="maya", extra_paths=["/path"])` — never as positionals.

21. **`DeferredExecutor` import path**: `DeferredExecutor` is Rust-backed and must be imported via `from dcc_mcp_core._core import DeferredExecutor` until it is added to the public `__init__.py` exports. Always check `__init__.py` first.

## CI/CD Summary

- 35 total CI jobs: Rust lint/test (3 platforms) + Python test matrix (Linux/macOS/Windows × py3.7–3.13) + DCC integration tests
- Docs-only changes skip Rust rebuild → CI passes quickly
- Squash merge convention for PRs
- `docs/` prefix in branch names causes `refs/heads/docs/...` conflicts — use flat names like `feat-xxx`
