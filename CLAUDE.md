# CLAUDE.md — dcc-mcp-core Instructions for Claude Code

> **Purpose**: Claude Code specific instructions. Complements AGENTS.md with Claude-specific guidance.

## Project Identity

You are working on **dcc-mcp-core**, a Rust-powered MCP (Model Context Protocol) library for DCC (Digital Content Creation) applications. The Python package name is `dcc_mcp_core`.

## Quick Reference

### Before Making Changes

1. Read `AGENTS.md` for full project context
2. Current branch convention: `feat/`, `fix/`, `docs/`, `refactor/`
3. Always run commands with `vx` prefix

### Essential Commands

```bash
vx just preflight     # Before committing (Rust check + clippy + fmt + test)
vx just test          # Python tests
vx just lint          # Full lint (Rust + Python)
vx just dev           # Build dev wheel (needed before running Python tests)
```

### Architecture Summary

- **11 Rust crates** under `crates/`, compiled into `_core` native extension
- **~105 public Python symbols** exported from `python/dcc_mcp_core/__init__.py`
- **Zero runtime Python deps** — all logic in Rust
- Key entry point: `src/lib.rs` (PyO3 `#[pymodule]`)

### When Working With This Codebase

**Adding a new Python-accessible function/class:**
1. Implement in the appropriate `crates/dcc-mcp-*/src/` Rust crate
2. Add PyO3 bindings in the crate's `python.rs` module
3. Register in `src/lib.rs` corresponding `register_*()` function
4. Re-export in `python/dcc_mcp_core/__init__.py`
5. Update type stubs if needed (`_core.pyi`)
6. Add pytest tests in `tests/`

**Working with Skills:**
- Skills are discovered via `SKILL.md` files in directories listed in `DCC_MCP_SKILL_PATHS`
- Each skill's scripts become automatically registered actions
- See `examples/skills/` for reference implementations

**Understanding the Transport layer:**
- Uses IPC (Unix socket / named pipe) for process communication
- `TransportManager` manages connection pools with `CircuitBreaker` resilience
- `FramedMessage` protocol for reliable message delivery

## Claude-Specific Tips

- Prefer reading `__init__.py` over guessing imports — it has the complete public API surface
- For large refactors across crates, use `cargo check --workspace` early to catch errors
- The `justfile` has cross-platform recipes (Windows PowerShell + Unix sh)
- When debugging Python-Rust binding issues, check `_core.pyi` stubs match actual PyO3 registrations
- Use `vx just test-cov` to see coverage gaps before adding new features
