# dcc-mcp-core development commands
# Usage: just <recipe>  (or: just --list)
#
# All CI jobs use these recipes — local and CI are identical.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["sh", "-cu"]

default:
    @just --list

# ── Rust ──────────────────────────────────────────────────────────────────────

# Check all crates compile
check:
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

# Run Rust unit/integration tests
test-rust:
    cargo test --workspace

# Rust test coverage via cargo-tarpaulin (install: cargo install cargo-tarpaulin)
rust-cov:
    cargo tarpaulin --workspace --out Html --out Xml --output-dir coverage/ --timeout 300

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

# Run the server locally (auto-discovers skills, MCP :8765, WS bridge :9001)
run-server *ARGS:
    cargo run --release -p dcc-mcp-server -- {{ARGS}}

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
    maturin develop --features python-bindings,ext-module

[windows]
dev:
    if (-not $env:VIRTUAL_ENV -and -not $env:CONDA_PREFIX -and -not $env:CI) { \
        if (-not (Test-Path .venv)) { python -m venv .venv }; \
        & .\.venv\Scripts\Activate.ps1; \
    }
    pip install maturin -q 2>$null
    maturin develop --features python-bindings,ext-module

# Build abi3-py38 release wheel and install it
install:
    maturin build --release --out dist --features python-bindings,ext-module,abi3-py38
    pip install --force-reinstall --no-index --find-links dist dcc-mcp-core

# Build abi3-py38 release wheel (dist/ only, no install)
build:
    maturin build --release --features python-bindings,ext-module,abi3-py38

# Build Python 3.7 wheel (non-abi3, for py37-specific CI jobs)
build-py37:
    maturin build --release --out dist --features python-bindings,ext-module

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

# Pre-flight: Rust check + clippy + fmt + tests — run before every commit
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
