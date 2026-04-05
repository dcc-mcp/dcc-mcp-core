# AGENTS.md — dcc-mcp-core AI Agent Guide

> **Purpose**: This file helps AI coding agents (Claude, Copilot, Cursor, Codex, etc.) understand, navigate, and contribute to this project effectively.

## Project Overview

**dcc-mcp-core** is the foundational library for the DCC (Digital Content Creation) Model Context Protocol (MCP) ecosystem. It provides a Rust-powered core with Python bindings (PyO3/maturin) that enables AI assistants to interact with DCC software (Maya, Blender, Houdini, etc.).

### Key Architecture Facts

- **Language**: Rust core (11 crates workspace) + Python bindings via PyO3
- **Build system**: `cargo` (Rust) + `maturin` (Python wheels)
- **Python package**: `dcc_mcp_core` with ~105 public symbols re-exported from `_core` native extension
- **Zero runtime Python dependencies** — everything is compiled into the Rust core
- **Version**: 0.12.x (use Release Please for versioning — never manually bump)

## Repository Structure

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 entry point (_core module)
├── Cargo.toml                  # Workspace definition (11 crates)
├── pyproject.toml              # Python package metadata
├── justfile                    # Development commands (use: vx just <recipe>)
│
├── crates/                     # Rust workspace crates
│   ├── dcc-mcp-models/         # ActionResultModel, SkillMetadata
│   ├── dcc-mcp-actions/        # ActionRegistry, EventBus, Pipeline, Dispatcher, Validator
│   ├── dcc-mcp-skills/         # SkillScanner, SkillLoader, SkillWatcher, Resolver
│   ├── dcc-mcp-protocols/      # MCP types: ToolDefinition, ResourceDefinition, Prompt, DccAdapter
│   ├── dcc-mcp-transport/      # IPC, ConnectionPool, SessionManager, CircuitBreaker, FramedChannel
│   ├── dcc-mcp-process/        # PyDccLauncher, ProcessMonitor, ProcessWatcher, CrashRecovery
│   ├── dcc-mcp-telemetry/      # Tracing/recording infrastructure
│   ├── dcc-mcp-sandbox/        # Security policy, input validation, audit logging
│   ├── dcc-mcp-shm/            # Shared memory buffers (LZ4 compressed)
│   ├── dcc-mcp-capture/        # Screen/window capture backend
│   ├── dcc-mcp-usd/            # USD scene description bridge
│   └── dcc-mcp-utils/          # Filesystem, type wrappers, constants, JSON helpers
│
├── python/dcc_mcp_core/
│   ├── __init__.py             # Public API re-exports (~105 symbols)
│   ├── _core.pyi               # Type stubs (75 KB, auto-generated-ish)
│   └── py.typed                # PEP 561 marker
│
├── tests/                      # Python integration tests (19 files)
├── examples/skills/            # 10 example SKILL.md packages
├── docs/                       # VitePress documentation site (EN + ZH)
│   ├── api/                    # API reference per module
│   └── guide/                  # User guides & tutorials
└── .agents/skills/             # VX toolchain skills (IDE-agnostic)
```

## Build & Test Commands

### Prerequisites

```bash
# Install vx (recommended): https://github.com/loonghao/vx
# Or ensure you have: Rust 1.85+, Python 3.8+, maturin
```

### Core Commands (via `vx just` or `just`)

| Command | Purpose |
|---------|---------|
| `vx just check` | Cargo check all workspace crates |
| `vx just clippy` | Clippy lint with `-D warnings` (CI strict) |
| `vx just fmt` | Format Rust + Python |
| `vx just fmt-check` | Check format only (CI mode) |
| `vx just test-rust` | Run all Rust unit tests (`cargo test --workspace`) |
| `vx just dev` | Build + install wheel in dev mode (`maturin develop`) |
| `vx just install` | Build release wheel + pip install |
| `vx just test` | Run pytest on `tests/` |
| `vx just test-cov` | Run tests with coverage report |
| `vx just lint-py` | Ruff check Python code |
| `vx just lint-py-fix` | Fix + format Python |
| `vx just lint` | Full lint: clippy + fmt-check + lint-py |
| `vx just lint-fix` | Auto-fix all lint issues |
| `vx just preflight` | Pre-commit: check + clippy + fmt-check + test-rust |
| `vx just ci` | Full CI pipeline: preflight + install + test + lint-py |
| `vx just build` | Build release wheel |
| `vx just clean` | Remove all build artifacts |

### Running Specific Tests

```bash
# Run a single test file
vx uvx nox -s pytest -- tests/test_skills.py -v

# Run with coverage for a specific module
pytest tests/test_actions.py -v --cov=dcc_mcp_core --cov-report=term-missing
```

## Code Style & Conventions

### Rust

- Edition 2024, MSRV 1.85
- Use `tracing` for logging (never `println!` / `eprintln!`)
- Use `thiserror` for error types
- Use `parking_lot` instead of `std::sync::Mutex`
- Keep files under 1000 lines (split into submodules if needed)

### Python

- Formatter: `ruff format` (line length: 120, double quotes)
- Linter: `ruff check` (includes isort via `I` rules)
- Target: Python 3.8+ (CI tests 3.9–3.13)
- Docstrings: Google-style
- All public APIs must have type annotations

### Import Organization (Strict)

```python
# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import ActionResultModel
```

## API Reference (What AI Agents Should Know)

### Current Public API (Rust-backed, v0.12+)

The public API is in `python/dcc_mcp_core/__init__.py`. Key domains:

#### Actions System
- `ActionRegistry` — Register/dispatch/invoke actions
- `ActionDispatcher` — Typed action dispatch with validation
- `ActionValidator` — Input parameter validation
- `EventBus` — Pub/sub event system for action lifecycle
- `ActionResultModel` / `success_result()` / `error_result()`

#### Skills System
- `SkillScanner` — Scan directories for `SKILL.md` files
- `SkillWatcher` — File-watching auto-reload for skills
- `SkillMetadata` — Parsed skill metadata model
- `parse_skill_md()` / `scan_skill_paths()` / `scan_and_load()` / `scan_and_load_lenient()`
- `resolve_dependencies()` / `expand_transitive_dependencies()` / `validate_dependencies()`

#### MCP Protocol Types
- `ToolDefinition`, `ToolAnnotations` — MCP Tool schema
- `ResourceDefinition`, `ResourceAnnotations` — MCP Resource schema
- `PromptDefinition`, `PromptArgument` — MCP Prompt schema
- `DccInfo`, `DccCapabilities`, `DccError`, `DccErrorCode` — DCC adapter types

#### Transport Layer
- `TransportManager` — Manage IPC connections
- `TransportAddress`, `TransportScheme`, `RoutingStrategy` — Addressing
- `IpcListener`, `ListenerHandle`, `FramedChannel` — IPC primitives
- `connect_ipc()` — Connect to an IPC server

#### Process Management
- `PyDccLauncher` — Launch DCC processes
- `PyProcessMonitor` — Track running processes
- `PyProcessWatcher` — Auto-restart crashed processes
- `PyCrashRecoveryPolicy` — Crash recovery configuration
- `ScriptResult`, `ScriptLanguage` — Script execution results

#### Other Modules
- **Sandbox**: `SandboxContext`, `SandboxPolicy`, `InputValidator`, `AuditEntry`, `AuditLog`
- **Shared Memory**: `PyBufferPool`, `PySharedBuffer`, `PySharedSceneBuffer`
- **Capture**: `Capturer`, `CaptureFrame`, `CaptureResult`
- **Telemetry**: `TelemetryConfig`, `RecordingGuard`
- **USD**: `UsdStage`, `UsdPrim`, `SdfPath`, `VtValue`, `scene_info_json_to_stage()`, `stage_to_scene_info_json()`

### Legacy APIs (DO NOT USE in new code)

The README still references legacy Python-only APIs that were removed in v0.12+:
- ~~`ActionManager`~~ → Use `ActionRegistry` + `ActionDispatcher`
- ~~`Action` base class~~ → Actions are now registered via `ActionRegistry`
- ~~`Middleware` / `MiddlewareChain`~~ → Use `ActionPipeline` with middleware context
- ~~`create_action_manager()`~~ → Use `ActionRegistry()` directly

## Skills System (How It Works)

Skills allow zero-code registration of scripts as MCP tools:

1. Create a directory with `SKILL.md` (YAML frontmatter + markdown body) and `scripts/` folder
2. Set `DCC_MCP_SKILL_PATHS` env var to point to skill directories
3. Call `scan_and_load()` or use `ActionRegistry` which auto-loads from env
4. Each script becomes an invocable action with `{skill_name}__{script_name}` naming

See `examples/skills/` for 10 complete examples covering Python, MEL, Shell, Batch, JavaScript.

## Commit & PR Guidelines

- Use [Conventional Commits](https://www.conventionalcommits.org/)
- Prefixes: `feat:`, `fix:`, `docs:`, `chore:`, `ci:`, `refactor:`, `test:`
- Breaking changes: `feat!:` or add `BREAKING CHANGE:` footer
- Never manually bump version or edit CHANGELOG.md (Release Please handles this)
- Always run `vx just preflight` before committing
- PR title format: follow Conventional Commits

## Common Pitfalls for AI Agents

1. **Don't import from internal paths** like `dcc_mcp_core.actions.base` — these don't exist anymore. Use the public API from `dcc_mcp_core`.
2. **Don't manually edit version numbers** — handled by Release Please.
3. **Don't add runtime Python dependencies** — this project has zero runtime deps by design.
4. **Rust changes need Python binding updates** — if you modify a Rust struct that's exposed via PyO3, update the corresponding `python.rs` file and potentially `_core.pyi` stubs.
5. **Test with `--features python-bindings` when testing Rust-Python integration**.
6. **Use `vx` prefix for all commands** in this project (e.g., `vx just test` not `pytest`).

## CI/CD

- Main branch is protected; all PRs must pass CI
- CI runs: `preflight` → `install` → `test` → `lint-py`
- PyPI publishing uses Trusted Publishing (no tokens needed)
- Documentation deploys to GitHub Pages via VitePress
