# dcc-mcp-core development commands
# Usage: just <recipe>  (or: just --list)
#
# All CI jobs use these recipes — local and CI are identical.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["sh", "-cu"]

# ── Feature sets (single source of truth) ─────────────────────────────────────
# Opt-in Cargo features that must ship in every wheel. Add new features here
# and every recipe below — as well as CI workflows invoking `just build-*` —
# will pick them up automatically.
OPT_FEATURES := "workflow,scheduler,prometheus,job-persist-sqlite"

# Feature set for `maturin develop` (no abi3, extension-module linkage)
DEV_FEATURES := "python-bindings,ext-module," + OPT_FEATURES

# Feature set for abi3 release wheels (Python 3.8+)
WHEEL_FEATURES := "python-bindings,ext-module,abi3-py38," + OPT_FEATURES

# Feature set for Python 3.7 wheels (non-abi3 — PyO3 requires >=3.8 for abi3)
WHEEL_FEATURES_PY37 := "python-bindings,ext-module," + OPT_FEATURES

default:
    @just --list

# ── Feature introspection (for CI / scripts) ──────────────────────────────────
# CI workflows call these to pick up the canonical feature list from justfile
# rather than hard-coding feature names in workflow YAML.
#
# Example (GitHub Actions):
#   FEATURES=$(just print-wheel-features)
#   maturin build --release --features "$FEATURES"

print-opt-features:
    @echo "{{OPT_FEATURES}}"

print-dev-features:
    @echo "{{DEV_FEATURES}}"

print-wheel-features:
    @echo "{{WHEEL_FEATURES}}"

print-wheel-features-py37:
    @echo "{{WHEEL_FEATURES_PY37}}"

# ── Rust ──────────────────────────────────────────────────────────────────────

# Check all crates compile (also regenerates _core.pyi for IDE completions)
check:
    just stubgen
    cargo check --workspace

# Run clippy (same flags as CI)
clippy:
    cargo clippy --workspace -- -D warnings

# Format Rust source
fmt:
    cargo fmt --all

# Check Rust formatting (CI mode — no writes)
fmt-check:
    cargo fmt --all -- --check

# Run Rust unit/integration tests (nextest for unit/integration, cargo test for doctests).
# nextest runs each test in its own process (≈2-3× faster on Windows-MSVC) but does
# not run doctests, so we chain a `cargo test --doc` pass to preserve coverage.
test-rust:
    cargo nextest run --workspace
    cargo test --workspace --doc

# Rust test coverage via cargo-tarpaulin (install: cargo install cargo-tarpaulin)
rust-cov:
    cargo tarpaulin --workspace --out Html --out Xml --output-dir coverage/ --timeout 300

# Run criterion benchmarks for IPC transport
bench:
    cargo bench -p dcc-mcp-transport

# ── Standalone binary (dcc-mcp-server) ───────────────────────────────────────

# Build dcc-mcp-server for the current platform
build-server:
    cargo build --release -p dcc-mcp-server

# Build dcc-mcp-server universal2 binary for macOS (requires both targets installed)
[unix]
build-server-universal:
    #!/usr/bin/env sh
    set -eu
    rustup target add x86_64-apple-darwin aarch64-apple-darwin 2>/dev/null || true
    cargo build --release -p dcc-mcp-server --target x86_64-apple-darwin
    cargo build --release -p dcc-mcp-server --target aarch64-apple-darwin
    lipo -create -output dcc-mcp-server-macos-universal2 \
        target/x86_64-apple-darwin/release/dcc-mcp-server \
        target/aarch64-apple-darwin/release/dcc-mcp-server
    echo "Built: dcc-mcp-server-macos-universal2"

# Run the server locally (auto-discovers skills, competes for gateway :9765)
run-server *ARGS:
    cargo run --release -p dcc-mcp-server -- {{ARGS}}

# Run two server instances to demo auto-gateway (open two terminals)
# Terminal 1: just demo-gateway-maya   → wins gateway :9765
# Terminal 2: just demo-gateway-maya2  → plain instance
demo-gateway-maya:
    cargo run -p dcc-mcp-server -- --dcc maya --scene shot01.ma

demo-gateway-photoshop:
    cargo run -p dcc-mcp-server -- --dcc photoshop --scene poster.psd

# ── Python ────────────────────────────────────────────────────────────────────

# Build and install wheel in dev/editable mode
[unix]
dev:
    #!/usr/bin/env sh
    set -eu
    if [ -z "${VIRTUAL_ENV:-}" ] && [ -z "${CONDA_PREFIX:-}" ] && [ -z "${CI:-}" ]; then \
        if [ ! -d .venv ]; then python -m venv .venv; fi; \
        . .venv/bin/activate; \
    fi
    pip install maturin 2>/dev/null || true
    just stubgen
    maturin develop --features {{DEV_FEATURES}}

[windows]
dev:
    if (-not $env:VIRTUAL_ENV -and -not $env:CONDA_PREFIX -and -not $env:CI) { \
        if (-not (Test-Path .venv)) { python -m venv .venv }; \
        & .\.venv\Scripts\Activate.ps1; \
    }
    pip install maturin -q 2>$null
    just stubgen
    maturin develop --features {{DEV_FEATURES}}

# Build abi3-py38 release wheel and install it
install:
    just stubgen
    maturin build --release --out dist --features {{WHEEL_FEATURES}}
    pip install --force-reinstall --no-index --find-links dist dcc-mcp-core

# Build abi3-py38 release wheel (dist/ only, no install).
# EXTRA is forwarded to maturin — used by CI to pass --sdist,
# --find-interpreter, --target, etc. without duplicating feature flags.
build *EXTRA:
    just stubgen
    maturin build --release --out dist --features {{WHEEL_FEATURES}} {{EXTRA}}

# Build Python 3.7 wheel (non-abi3, for py37-specific CI jobs).
# EXTRA is forwarded to maturin (e.g. `-i python3.7`, `--target x86_64`).
build-py37 *EXTRA:
    just stubgen
    maturin build --release --out dist --features {{WHEEL_FEATURES_PY37}} {{EXTRA}}

# Install dev/test dependencies
install-dev-deps:
    pip install maturin pytest pytest-cov anyio ruff

# ── Python tests ──────────────────────────────────────────────────────────────

# Run Python test suite
test:
    pytest tests/ -v --tb=short

# Run Python tests with coverage report
test-cov:
    pytest tests/ -v --cov=dcc_mcp_core --cov-report=term --cov-report=xml:coverage.xml

# Run mcporter MCP end-to-end tests (requires: npm install -g mcporter)
test-e2e:
    pytest tests/test_mcp_mcporter_e2e.py -v --tb=short

# ── Type stubs (pyo3-stub-gen) ───────────────────────────────────────────

# Generate python/dcc_mcp_core/_core.pyi from annotated Rust code.
# Also run automatically as part of `build`, `dev`, and `install`.
stubgen:
    cargo run --bin stub_gen --features stub-gen

# Check that _core.pyi is in sync with the Rust source (for CI drift detection).
stubgen-check:
    cargo run --bin stub_gen --features stub-gen -- --check

# ── Docs ──────────────────────────────────────────────────────────────────────

# Check VitePress docs build (catches dead links, syntax errors)
docs-check:
    #!/usr/bin/env bash
    cd docs && npm ci && npm run docs:build

# ── Lint ──────────────────────────────────────────────────────────────────────

# Lint Python source (ruff check only)
lint-py:
    ruff check python/dcc_mcp_core/ tests/ examples/

# Auto-fix Python lint issues and format
lint-py-fix:
    ruff check --fix python/dcc_mcp_core/ tests/ examples/
    ruff format python/dcc_mcp_core/ tests/ examples/

# Lint everything: Rust (clippy + fmt-check) + Python (ruff)
lint: clippy fmt-check lint-py

# Fix all fixable lint issues (Rust fmt + Python ruff)
lint-fix: fmt lint-py-fix

# ── Aggregate targets (CI + local) ────────────────────────────────────────────

# Pre-flight: Rust check + clippy + fmt + tests + docs — run before every commit
preflight: check clippy fmt-check test-rust

# Full local CI pipeline: preflight → build wheel → Python tests → lint
ci: preflight install test lint-py

# ── Clean ─────────────────────────────────────────────────────────────────────

[unix]
clean:
    rm -rf dist build target .coverage coverage.xml

[windows]
clean:
    if (Test-Path dist) { Remove-Item -Recurse -Force dist }
    if (Test-Path build) { Remove-Item -Recurse -Force build }
    if (Test-Path target) { Remove-Item -Recurse -Force target }
    Remove-Item -ErrorAction SilentlyContinue -Force .coverage, coverage.xml
