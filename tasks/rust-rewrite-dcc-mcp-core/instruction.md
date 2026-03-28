# Rewrite Python+Pydantic library to Rust+PyO3 with zero dependencies

## Objective

Rewrite the `dcc-mcp-core` Python library — the foundational core of a DCC (Digital Content Creation) MCP (Model Context Protocol) framework ecosystem — from a pure Python+Pydantic implementation to a Rust+PyO3+maturin architecture with **zero Python runtime dependencies** for Python 3.8+.

## Starting Point

You are given a Python project at `/app` (cloned from `https://github.com/loonghao/dcc-mcp-core`, branch `main` at tag `v0.10.0`). The project currently uses:
- **Python** with **Pydantic v2** for models and validation
- **platformdirs** for platform directories
- **loguru** for logging
- **Jinja2** for templating
- **Poetry** as build system

The project has the following key modules under `dcc_mcp_core/`:
- `models.py` — `ActionResultModel`, `SkillMetadata` (Pydantic BaseModel)
- `actions/` — `ActionRegistry` (singleton), `EventBus`, `Action` base class, middleware
- `protocols/` — MCP type definitions (`ToolDefinition`, `ResourceDefinition`, etc.)
- `skills/` — `SkillScanner`, SKILL.md parser
- `utils/` — filesystem helpers, type wrappers (for RPyC), exceptions, decorators

## Requirements

### 1. Rust Workspace Architecture
Create a Cargo workspace with these sub-crates:
- `crates/dcc-mcp-models` — `ActionResultModel` (serde struct), `SkillMetadata`
- `crates/dcc-mcp-actions` — `ActionRegistry` (thread-safe, DashMap), `EventBus`
- `crates/dcc-mcp-protocols` — MCP protocol types (`ToolDefinition`, `ToolAnnotations`, etc.)
- `crates/dcc-mcp-skills` — `SkillScanner`, SKILL.md YAML parser
- `crates/dcc-mcp-utils` — Constants, filesystem (dirs crate), logging (tracing), type wrappers

The root crate (`src/lib.rs`) serves as the PyO3 `#[pymodule]` entry point (`dcc_mcp_core._core`).

### 2. Python Bindings (PyO3 0.23+)
- All struct fields must use `#[getter]`/`#[setter]` in `#[pymethods]` blocks (NOT `#[cfg_attr(feature, pyo3(get, set))]` on struct fields — this doesn't work with PyO3 proc-macros).
- `bool.into_pyobject(py)` returns `Borrowed<PyBool>` in PyO3 0.23 — use `PyBool::new(py, val).to_owned().into_any()` instead.
- Feature-gate all Python bindings behind `python-bindings` feature.
- Support `abi3-py38` for stable ABI (one wheel for all 3.8+ versions).

### 3. Python Package
- `pyproject.toml` with `maturin` build backend
- `python/dcc_mcp_core/__init__.py` that re-exports all public API from `_core`
- `python/dcc_mcp_core/py.typed` marker (PEP 561)
- **Zero runtime dependencies** for Python 3.8+ (`typing_extensions` only for 3.7)

### 4. Build & CI
- `justfile` with unified commands: `preflight`, `install`, `dev`, `test`, `lint`, `build`
- `.pre-commit-config.yaml` with `cargo-fmt` and `cargo-clippy` hooks
- CI workflow: build ABI3 wheel once per OS → test on Python 3.9-3.13
- Release workflow: release-please → build wheels → PyPI publish
- Tag format: `v0.x.0` (NOT `dcc-mcp-core-v*`)
- MSRV: Rust 1.75 (use `once_cell::sync::Lazy` instead of `std::sync::LazyLock`)

### 5. Tests
Write comprehensive Python tests (`tests/`) covering all exposed PyO3 APIs:
- `test_models.py` — ActionResultModel, factory functions (success_result, error_result, from_exception, validate_action_result)
- `test_actions.py` — ActionRegistry (register, get, list, reset), EventBus (subscribe, unsubscribe, publish)
- `test_protocols.py` — All 6 MCP types with getters/setters
- `test_skills.py` — SkillMetadata, SkillScanner, parse_skill_md
- `test_utils.py` — filesystem, type wrappers, constants

### 6. Cleanup
- Delete the old `dcc_mcp_core/` directory (pure Python implementation)
- Delete old test files, `poetry.lock`, `requirements-dev.txt`, `pytest.ini`
- Delete old CI workflows (`mr-test.yml`, `codecov.yml`, `release-please.yml`)

## Success Criteria

1. `cargo check --workspace` passes
2. `cargo clippy --workspace -- -D warnings` passes
3. `cargo fmt --all -- --check` passes
4. `cargo test --workspace` passes (Rust unit tests)
5. `cargo check --features python-bindings,ext-module,abi3-py38` passes
6. `maturin build --release --features python-bindings,ext-module,abi3-py38` produces a wheel
7. `pip install <wheel> && pytest tests/ -v` passes all Python tests
8. The installed package has **zero Python dependencies** (verify with `pip show dcc-mcp-core`)
9. `python -c "import dcc_mcp_core; print(dcc_mcp_core.__version__)"` works

## Context

This is part of a larger DCC MCP ecosystem:
- `dcc-mcp-core` (this repo) — Core library
- `dcc-mcp-rpyc` — RPyC transport layer
- `dcc-mcp-maya`, `dcc-mcp-3dsmax`, `dcc-mcp-houdini`, `dcc-mcp-blender` — DCC plugins
- `dcc-mcp` — Orchestrator

The key feature is progressive skill discovery — DCC applications discover and reuse skills at runtime via SKILL.md files.
