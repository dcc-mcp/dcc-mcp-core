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
OPT_FEATURES := "workflow,scheduler,prometheus,job-persist-sqlite,admin"
CLI_BIN := if os_family() == "windows" { ".\\target\\release\\dcc-mcp-cli.exe" } else { "./target/release/dcc-mcp-cli" }

# Feature set for `maturin develop` (no abi3, extension-module linkage)
DEV_FEATURES := "python-bindings,ext-module," + OPT_FEATURES

# Feature set for abi3 release wheels (Python 3.8+)
WHEEL_FEATURES := "python-bindings,ext-module,abi3-py38," + OPT_FEATURES

# Feature set for Python 3.7 native wheels (non-abi3 — PyO3 requires >=3.8 for abi3)
WHEEL_FEATURES_PY37 := "python-bindings,ext-module," + OPT_FEATURES

default:
    @just --list

# ── Git hooks ─────────────────────────────────────────────────────────────────

# Install git hooks from .githooks/ (sets core.hooksPath).
# Run once after clone. Also invoked automatically by `dev`.
githooks-install:
    git config core.hooksPath .githooks

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

# ── Admin UI ──────────────────────────────────────────────────────────────────
#
# The Vite bundle is written to crates/dcc-mcp-gateway/src/gateway/admin/generated/
# and is intentionally gitignored. It is rebuilt automatically whenever Cargo compiles
# `dcc-mcp-gateway` with the `admin` feature (wheel builds, local `cargo check`, etc.);
# `crates/dcc-mcp-gateway/build.rs` runs `vx npm ci` (if needed) and `vx npm run build`.
#
# Use these recipes when you iterate on JSX/CSS only and want fast rebuilds without
# recompiling the whole Rust workspace.

# Install admin UI dependencies from the committed lockfile (Node from vx.toml via vx)
# Note: npm ci skips optional deps by default; use --include=optional to
# ensure native bindings (rolldown) are installed and the Vite build succeeds.
admin-install:
    vx npm --prefix admin-ui ci

# Build the React admin UI into the Rust-embedded generated HTML
admin-build: admin-install
    vx npm --prefix admin-ui run build

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

# Rust test coverage via cargo-llvm-cov (install: cargo install cargo-llvm-cov)
# Generates lcov.info; CI uploads to Codecov (set `files: coverage/lcov.info`).
rust-cov:
    mkdir -p coverage
    cargo llvm-cov --workspace --lcov --output-path coverage/lcov.info

# Run criterion benchmarks for IPC transport
bench:
    cargo bench -p dcc-mcp-transport

# Regenerate workspace-hack (run after dependency updates to fix CI)
hakari-generate:
    cargo hakari generate

# Verify workspace-hack is up to date (local preflight check)
hakari-verify:
    cargo hakari verify

# ── Standalone binaries ─────────────────────────────────────────────────────

# Build and stage the isolated PrintWindow helper used by Windows wheels.
# Non-Windows packages intentionally do not carry this executable.
[windows]
stage-capture-helper:
    cargo build --release -p dcc-mcp-capture --bin dcc-mcp-capture-helper
    python scripts/packaging/stage_capture_helper.py

[unix]
stage-capture-helper:
    @true

# Build dcc-mcp-cli for the current platform
build-cli:
    cargo build --release -p dcc-mcp-cli

# Build dcc-mcp-cli universal2 binary for macOS (requires both targets installed)
[unix]
build-cli-universal:
    #!/usr/bin/env sh
    set -eu
    rustup target add x86_64-apple-darwin aarch64-apple-darwin 2>/dev/null || true
    cargo build --release -p dcc-mcp-cli --target x86_64-apple-darwin
    cargo build --release -p dcc-mcp-cli --target aarch64-apple-darwin
    lipo -create -output dcc-mcp-cli-macos-universal2 \
        target/x86_64-apple-darwin/release/dcc-mcp-cli \
        target/aarch64-apple-darwin/release/dcc-mcp-cli
    echo "Built: dcc-mcp-cli-macos-universal2"

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
    cargo run -p dcc-mcp-server -- --app maya --scene shot01.ma

demo-gateway-photoshop:
    cargo run -p dcc-mcp-server -- --app photoshop --scene poster.psd

# ── Python ────────────────────────────────────────────────────────────────────

# Build and install wheel in dev/editable mode.
# Always run maturin with *this repo's* .venv interpreter. If another project
# (e.g. dcc-mcp-maya) left VIRTUAL_ENV active, the old recipe used that Python
# and produced the wrong wheel ABI (e.g. cp312) for Maya's embedded runtime.
[unix]
dev:
    #!/usr/bin/env sh
    set -eu
    just githooks-install
    if [ ! -d .venv ]; then python -m venv .venv; fi
    .venv/bin/python -m pip install maturin 2>/dev/null || true
    just stubgen
    .venv/bin/python -m maturin develop --features {{DEV_FEATURES}}

[windows]
dev:
    just githooks-install
    if (-not (Test-Path .venv)) { python -m venv .venv }
    & .\.venv\Scripts\python.exe -m pip install --disable-pip-version-check maturin -q
    just stubgen
    just stage-capture-helper
    & .\.venv\Scripts\python.exe -m maturin develop --features {{DEV_FEATURES}}

# Build abi3-py38 release wheel and install it
install:
    just stubgen
    just stage-capture-helper
    maturin build --release --out dist --features {{WHEEL_FEATURES}}
    pip install --force-reinstall --no-index --find-links dist dcc-mcp-core

# Build abi3-py38 release wheel (dist/ only, no install).
# EXTRA is forwarded to maturin — used by CI to pass --sdist,
# --find-interpreter, --target, etc. without duplicating feature flags.
build *EXTRA:
    just stubgen
    just stage-capture-helper
    maturin build --release --out dist --features {{WHEEL_FEATURES}} {{EXTRA}}

# Build the py37-lite pure-Python wheel (py3-none-any).
# Uses no PyO3 features so maturin produces a wheel with only pure-Python
# sources — no compiled _core extension. The wheel is tagged py3-none-any
# and installable on Python 3.7 (Maya 2022 / Blender 2.83).
build-py37-lite:
    python scripts/build_py37_pure_wheel.py

# Build Python 3.7 native wheel (non-abi3, compiled _core extension).
# EXTRA is forwarded to maturin (e.g. `-i python3.7`, `--target x86_64`).
build-py37 *EXTRA:
    just stubgen
    just stage-capture-helper
    maturin build --release --out dist --features {{WHEEL_FEATURES_PY37}} {{EXTRA}}

# Install dev/test dependencies
install-dev-deps:
    pip install maturin pytest pytest-cov anyio ruff

# ── Python tests ──────────────────────────────────────────────────────────────

# Run Python test suite (single-process, fastest for local iteration)
test:
    pytest tests/ -q --tb=short --show-capture=no

# Run full Python test suite with file-level process isolation (CI).
# ``--dist loadfile`` runs each test file in its own subprocess, preventing
# Rust ``OnceLock`` singleton conflicts, env-var leaks, and port TIME_WAIT
# cascades between test modules. Use this for the authoritative CI gate.
# ``-n 4`` caps workers to avoid OOM on smaller CI runners.
test-suite:
    pytest tests/ -q --tb=short --show-capture=no -n 4 --dist loadfile

# Run Python tests with coverage report
test-cov:
    pytest tests/ -q --tb=short --show-capture=no --cov=dcc_mcp_core --cov-report=term --cov-report=xml:coverage.xml

# Run mcpcall MCP end-to-end tests (set MCPCALL_CMD="vx mcpcall" for vx-managed local runs)
test-e2e:
    pytest tests/test_mcp_mcpcall_e2e.py -v --tb=short

# Replay a Verified Regression Suite trace (HTTP against gateway REST /v1/*).
# Example: `just vrs-replay`
# Example: `just vrs-replay BASE=http://127.0.0.1:9765 TRACE=tests/vrs/traces/maya-215-execute-python-regression.jsonl`
vrs-replay BASE="http://127.0.0.1:9765" TRACE="tests/vrs/traces/gateway-smoke.jsonl":
    python scripts/vrs_replay.py --base-url "{{BASE}}" --trace "{{TRACE}}"

# Lightweight standalone server idle-memory smoke (#1354).
idle-memory-smoke:
    python scripts/idle_memory_smoke.py

# ── Type stubs (pyo3-stub-gen) ───────────────────────────────────────────

# Generate python/dcc_mcp_core/_core.pyi from annotated Rust code.
# Also run automatically as part of `build`, `dev`, and `install`.
# Note: use `vx cargo` to ensure vx's cargo wrapper is used (CI fix).
stubgen:
    vx cargo run --bin stub_gen --features stub-gen

# Check that _core.pyi is in sync with the Rust source (for CI drift detection).
stubgen-check:
    vx cargo run --bin stub_gen --features stub-gen -- --check

# ── Docs ──────────────────────────────────────────────────────────────────────

# Check VitePress docs build (catches dead links, syntax errors)
docs-check:
    #!/usr/bin/env bash
    cd docs && npm ci && npm run docs:build

# ── Lint ──────────────────────────────────────────────────────────────────────

# Lint Python source (ruff check only)
lint-py:
    ruff check python/dcc_mcp_core/ tests/ examples/ scripts/
    ruff format --check python/dcc_mcp_core/ tests/ examples/ scripts/

# Lint bundled, example, and fixture skills with the built production CLI
lint-skills: build-cli
    {{CLI_BIN}} lint --max-depth 4 skills/dcc-mcp python/dcc_mcp_core/skills examples/skills examples/remote-server/skills examples/rez-skills tests/fixtures/skills tests/fixtures/prompts_skills

# Verify pure-Python sources parse on Python 3.7 (cp37 wheel parity).
check-py37-syntax:
    vx python@3.7 scripts/check_py37_syntax.py

# Verify package metadata, build profiles, CI jobs, and docs match the LTS contract.
check-python-support:
    python scripts/ci/check_python_support.py

# Auto-fix Python lint issues and format
lint-py-fix:
    ruff check --fix python/dcc_mcp_core/ tests/ examples/ scripts/
    ruff format python/dcc_mcp_core/ tests/ examples/ scripts/

# Lint everything: Rust (clippy + fmt-check) + Python (ruff) + skills
lint: clippy fmt-check lint-py lint-skills

# Fix all fixable lint issues (Rust fmt + Python ruff)
lint-fix: fmt lint-py-fix

# ── Aggregate targets (CI + local) ────────────────────────────────────────────

# Pre-flight: Rust check + clippy + fmt + tests + docs — run before every commit
preflight: check clippy fmt-check test-rust

# Full local CI pipeline: compatibility contract → preflight → wheel → tests → lint
ci: check-python-support preflight install test lint-py

# ── ClawHub (https://clawhub.ai/) ─────────────────────────────────────────────

# Package skills from .github/clawhub-skills.json (zip under dist/skills/).
package-clawhub-skills:
    python scripts/package_openclaw_skill.py skills/dcc-mcp dist/skills

# Validate publish commands without uploading (PR / local).
clawhub-sync-dry-run:
    python scripts/clawhub_sync.py --dry-run

# Publish manifest skills to ClawHub (requires CLAWHUB_TOKEN + login).
clawhub-sync:
    python scripts/clawhub_sync.py

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
