# dcc-mcp-core development commands
# Usage: vx just <recipe>

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["sh", "-cu"]

default:
    @just --list

# ── Rust ──

# Check all Rust crates
check:
    cargo check --workspace

# Run Rust tests
test-rust:
    cargo test --workspace

# Run clippy
clippy:
    cargo clippy --workspace -- -D warnings

# Format Rust code
fmt:
    cargo fmt --all

# Format check
fmt-check:
    cargo fmt --all -- --check

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
lint:
    ruff check python/dcc_mcp_core/ tests/
    isort --check-only python/dcc_mcp_core/ tests/

# Fix Python lint issues
lint-fix:
    ruff check --fix python/dcc_mcp_core/ tests/
    ruff format python/dcc_mcp_core/ tests/
    isort python/dcc_mcp_core/ tests/

# ── Full CI ──

# Run all checks (Rust + Python)
ci: check clippy fmt-check test-rust dev test lint

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
