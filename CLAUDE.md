# CLAUDE.md — dcc-mcp-core Instructions for Claude

> **Purpose**: Claude-specific instructions. Complements AGENTS.md with Claude-specific guidance.
> Read AGENTS.md first for full project context, then this file.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC (Digital Content Creation) applications. The Python package name is `dcc_mcp_core`.

## Quick Reference

### Before Making Changes

1. Read `AGENTS.md` for full project context
2. Read `python/dcc_mcp_core/__init__.py` for the complete public API surface
3. Read `python/dcc_mcp_core/_core.pyi` for parameter names/types when unsure
4. Current branch convention: `feat/`, `fix/`, `docs/`, `refactor/`, `chore/`
5. Always run commands with `vx` prefix

### Essential Commands

```bash
vx just preflight     # Before committing (Rust check + clippy + fmt + test)
vx just test          # Python tests
vx just lint          # Full lint (Rust + Python)
vx just dev           # Build dev wheel (needed before running Python tests)
vx just lint-fix      # Auto-fix all lint issues
vx just test-cov      # Coverage report to find gaps
```

### Architecture Summary

- **11 Rust crates** under `crates/`, compiled into `_core` native extension
- **~120 public Python symbols** exported from `python/dcc_mcp_core/__init__.py`
- **Zero runtime Python deps** — all logic in Rust
- Key entry point: `src/lib.rs` (PyO3 `#[pymodule]`)
- Python 3.7–3.13 supported (CI tests 3.7–3.13)

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
- See `examples/skills/` for 9 reference implementations

```python
# Correct usage:
skills, skipped = scan_and_load(dcc_name="maya")
# NOT: skills = scan_and_load(dcc_name="maya")  ← returns tuple, iterating gives wrong results
```

### When Understanding the Transport Layer

- Uses IPC (Unix socket / named pipe) for process communication
- `TransportManager` manages connection pools with `CircuitBreaker` resilience
- `FramedChannel` for reliable message delivery with message framing
- Connect (client): `connect_ipc(address) -> FramedChannel`
- Listen (server): `IpcListener.new(address)` → `.start(handler_fn) -> ListenerHandle`
  - Note: `start()` is the method name (not `.bind()` + `.accept()`)

### Quick Lookup: Common Method Signatures

```python
# ActionDispatcher — only .dispatch(), never .call()
dispatcher = ActionDispatcher(registry)   # takes ONE arg; no validator param
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

# ActionRegistry.register — takes keyword args, NOT handler=
registry.register(name="action", description="...", dcc="maya", version="1.0.0")
# Use dispatcher.register_handler() to attach a Python callable
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

### When Writing Tests

```python
# Import pattern for tests
from __future__ import annotations
import pytest
from dcc_mcp_core import ActionResultModel, success_result, error_result

# Skill tests: use tmp_path fixture + create minimal SKILL.md
def test_skill_scan(tmp_path):
    skill_dir = tmp_path / "my-skill"
    (skill_dir / "scripts").mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text("---\nname: my-skill\ndcc: python\n---\n")
    (skill_dir / "scripts" / "do_thing.py").write_text("print('hello')")

    from dcc_mcp_core import parse_skill_md
    meta = parse_skill_md(str(skill_dir))
    assert meta is not None
    assert meta.name == "my-skill"
```

## Claude-Specific Tips

- **Prefer reading `__init__.py`** over guessing imports — it has the complete public API surface
- **`_core.pyi` is the ground truth** for parameter names and types
- **For large refactors**, use `cargo check --workspace` early to catch errors before building the full wheel
- **The `justfile` is cross-platform**: recipes work on both Windows PowerShell and Unix sh
- **When debugging Python-Rust binding issues**: check that the symbol is registered in `src/lib.rs` AND re-exported in `__init__.py` AND listed in `_core.pyi`
- **Use `vx just test-cov`** to see coverage gaps before adding new features
- **Don't use legacy APIs**: `ActionManager`, `create_action_manager()`, `MiddlewareChain`, `LoggingMiddleware` — all removed in v0.12+
- **The project has zero runtime Python dependencies by design** — never add `dependencies = [...]` to `pyproject.toml`

## Key Files to Read First (Priority Order)

1. `python/dcc_mcp_core/__init__.py` — Complete public API (~120 symbols)
2. `python/dcc_mcp_core/_core.pyi` — Type stubs with parameter names
3. `AGENTS.md` — Full architecture, commands, pitfalls
4. `crates/*/src/python.rs` — PyO3 binding implementations
5. `src/lib.rs` — Module registration entry point
6. `tests/` — Usage examples in test form
