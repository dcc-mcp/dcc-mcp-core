# dcc-mcp-core development commands
# Usage: just <recipe>  (or: vx just <recipe>)

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["sh", "-cu"]

default:
    @just --list

# ── Rust ──

# Check all Rust crates
check:
    cargo check --workspace

# Run clippy with CI-identical flags
clippy:
    cargo clippy --workspace -- -D warnings

# Format Rust code
fmt:
    cargo fmt --all

# Format check (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run Rust tests
test-rust:
    cargo test --workspace

# ── Python ──

# Build and install wheel in dev mode
dev:
    maturin develop --features python-bindings,ext-module

# Run Python tests (requires `just dev` first)
test:
    pytest tests/ -v --tb=short

# Run Python tests with coverage
test-cov:
    pytest tests/ -v --cov=dcc_mcp_core --cov-report=term --cov-report=xml:coverage.xml

# Lint Python code
lint-py:
    ruff check python/dcc_mcp_core/ tests/
    isort --check-only python/dcc_mcp_core/ tests/

# Fix Python lint issues
lint-py-fix:
    ruff check --fix python/dcc_mcp_core/ tests/
    ruff format python/dcc_mcp_core/ tests/
    isort python/dcc_mcp_core/ tests/

# ── Unified commands (CI + local) ──

# Lint all (Rust + Python) — same checks as CI
lint: clippy fmt-check lint-py

# Fix all lint issues
lint-fix: fmt lint-py-fix

# Pre-flight check — run before committing (same as CI)
preflight: check clippy fmt-check test-rust

# Full CI pipeline (Rust + Python)
ci: preflight dev test lint-py

# Build release wheel
build:
    maturin build --release --features python-bindings,ext-module,abi3-py38

# ── Clean ──

[unix]
clean:
    rm -rf dist build target *.egg-info .nox .coverage coverage.xml

[windows]
clean:
    if (Test-Path dist) { Remove-Item -Recurse -Force dist }
    if (Test-Path build) { Remove-Item -Recurse -Force build }
    if (Test-Path target) { Remove-Item -Recurse -Force target }
    Get-ChildItem -Filter *.egg-info -Directory -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    Remove-Item -ErrorAction SilentlyContinue -Force .coverage, coverage.xml
