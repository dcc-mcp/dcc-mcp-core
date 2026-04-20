# CODEBUDDY.md — dcc-mcp-core Instructions for CodeBuddy Code

> **Purpose**: CodeBuddy Code-specific instructions. **Read `AGENTS.md` first** for full project context,
> architecture, commands, and pitfalls. This file adds only CodeBuddy-specific guidance.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC (Digital Content Creation) applications. The Python package name is `dcc_mcp_core`.

## Response Language

- Reply to the user in **Simplified Chinese** (中文简体) by default.
- Keep all code, identifiers, commit messages, branch names, docstrings, comments, and file contents in **English** — this rule governs only the conversational/assistant-facing output, not anything written to disk or pushed to git.
- If the user explicitly requests another language for a specific reply, follow that request for that turn.

## Document Hierarchy (Progressive Disclosure)

When you need information, read in this order — stop when you find what you need:

1. **`AGENTS.md`** — Navigation map: where to find everything, traps, Do/Don't
2. **`llms.txt`** — Compressed API reference for AI agents (token-efficient)
3. **`python/dcc_mcp_core/__init__.py`** — Complete public API surface (~177 symbols)
4. **`python/dcc_mcp_core/_core.pyi`** — Parameter names, types, signatures
5. **`llms-full.txt`** — Complete API reference with examples (when `llms.txt` lacks detail)
6. **`docs/guide/`** + **`docs/api/`** — Conceptual guides and per-module API docs
7. **`tests/`** — 120+ usage examples in test form

## CodeBuddy-Specific Workflows

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

### When Using MCP HTTP Server

```python
# Skills-First (recommended)
from dcc_mcp_core import create_skill_server, McpHttpConfig
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())  # "http://127.0.0.1:8765/mcp"

# Manual registry wiring (low-level)
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig
registry = ToolRegistry()
registry.register("get_scene", description="Get scene", category="scene", dcc="maya")
server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()
# Note: register ALL actions BEFORE calling server.start()
```

### When Debugging Build/Import Issues

```bash
# Rebuild dev wheel
vx just dev

# Verify import works
python -c "import dcc_mcp_core; print(dir(dcc_mcp_core))"

# Verbose cargo build
cargo build --workspace --features python-bindings 2>&1 | grep -E "error|warning" | head -30
```

## CodeBuddy-Specific Tips

- **Prefer reading `__init__.py`** over guessing imports — it has the complete public API surface
- **`_core.pyi` is the ground truth** for parameter names and types
- **For large refactors**, use `cargo check --workspace` early to catch errors before building the full wheel
- **Use `vx just dev`** before running any Python tests — the Rust extension must be compiled first
- **Don't use legacy APIs**: `ActionManager`, `create_action_manager()`, `MiddlewareChain`, `Action` base class — all removed in v0.12+. Note: `LoggingMiddleware` IS still available.
- **The project has zero runtime Python dependencies by design** — never add `dependencies = [...]` to `pyproject.toml`
- **`DeferredExecutor` is not in public `__init__.py`**: import via `from dcc_mcp_core._core import DeferredExecutor`
- **Commit messages**: Use Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`). Never manually bump versions.

## AI Agent Tool Priority

When building tools or interacting with DCCs, follow this priority order:

1. **Skill Discovery** (start here): `search_skills(query)` → `load_skill(name)` → use skill tools
2. **Skill-Based Tools** (preferred): Tools with validated schemas, error handling, `next-tools` guidance, and `ToolAnnotations` safety hints
3. **Diagnostics Tools** (for verification): `diagnostics__screenshot`, `diagnostics__audit_log`, `diagnostics__process_status`
4. **Direct Registry Access** (last resort): Only when no skill tool covers the operation; must validate with `ToolValidator` and sandbox with `SandboxPolicy`
