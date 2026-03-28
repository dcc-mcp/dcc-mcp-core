#!/bin/bash
# Oracle solution: apply the refactor/architecture-review branch changes
set -e

cd /app

# Fetch the solution branch
git fetch origin refactor/architecture-review
git checkout refactor/architecture-review

# Build and verify
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo test --workspace
cargo check --features python-bindings,ext-module,abi3-py38

# Build and install wheel
maturin build --release --out /tmp/dist --features python-bindings,ext-module,abi3-py38
pip install --break-system-packages --force-reinstall /tmp/dist/*.whl

# Run Python tests
pytest tests/ -v --tb=short

echo "Solution applied successfully"
