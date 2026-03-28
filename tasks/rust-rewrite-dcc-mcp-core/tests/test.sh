#!/bin/bash
set -e

cd /app

echo "=== Step 1: Verify Rust workspace structure ==="
test -f Cargo.toml || { echo "FAIL: No Cargo.toml"; exit 1; }
test -d crates/dcc-mcp-models || { echo "FAIL: Missing crates/dcc-mcp-models"; exit 1; }
test -d crates/dcc-mcp-actions || { echo "FAIL: Missing crates/dcc-mcp-actions"; exit 1; }
test -d crates/dcc-mcp-protocols || { echo "FAIL: Missing crates/dcc-mcp-protocols"; exit 1; }
test -d crates/dcc-mcp-skills || { echo "FAIL: Missing crates/dcc-mcp-skills"; exit 1; }
test -d crates/dcc-mcp-utils || { echo "FAIL: Missing crates/dcc-mcp-utils"; exit 1; }
echo "PASS: Workspace structure OK"

echo "=== Step 2: cargo check ==="
cargo check --workspace || { echo "FAIL: cargo check"; exit 1; }
echo "PASS: cargo check"

echo "=== Step 3: cargo clippy ==="
cargo clippy --workspace -- -D warnings || { echo "FAIL: cargo clippy"; exit 1; }
echo "PASS: cargo clippy"

echo "=== Step 4: cargo fmt --check ==="
cargo fmt --all -- --check || { echo "FAIL: cargo fmt"; exit 1; }
echo "PASS: cargo fmt"

echo "=== Step 5: cargo test ==="
cargo test --workspace || { echo "FAIL: cargo test"; exit 1; }
echo "PASS: cargo test"

echo "=== Step 6: Python bindings compilation ==="
cargo check --features python-bindings,ext-module,abi3-py38 || { echo "FAIL: python-bindings check"; exit 1; }
echo "PASS: python-bindings compilation"

echo "=== Step 7: Build wheel ==="
maturin build --release --out /tmp/dist --features python-bindings,ext-module,abi3-py38 || { echo "FAIL: maturin build"; exit 1; }
echo "PASS: wheel built"

echo "=== Step 8: Install wheel ==="
pip install --break-system-packages /tmp/dist/*.whl || { echo "FAIL: pip install"; exit 1; }
echo "PASS: wheel installed"

echo "=== Step 9: Zero dependencies check ==="
DEPS=$(pip show dcc-mcp-core 2>/dev/null | grep "^Requires:" | sed 's/Requires: //')
if [ -n "$DEPS" ] && [ "$DEPS" != "None" ]; then
    echo "FAIL: Package has dependencies: $DEPS"
    exit 1
fi
echo "PASS: zero dependencies"

echo "=== Step 10: Import check ==="
python -c "import dcc_mcp_core; print('version:', dcc_mcp_core.__version__); assert dcc_mcp_core.__version__ != '0.0.0-dev'" || { echo "FAIL: import check"; exit 1; }
echo "PASS: import check"

echo "=== Step 11: Python tests ==="
if [ -d /app/tests ] && ls /app/tests/test_*.py 1>/dev/null 2>&1; then
    pytest /app/tests/ -v --tb=short || { echo "FAIL: pytest"; exit 1; }
    echo "PASS: pytest"
else
    echo "WARN: No test files found, skipping pytest"
fi

echo "=== Step 12: Old code removed ==="
if [ -d /app/dcc_mcp_core ]; then
    echo "FAIL: Old dcc_mcp_core/ directory still exists"
    exit 1
fi
echo "PASS: old code removed"

echo "=== Step 13: Python wrapper exists ==="
test -f /app/python/dcc_mcp_core/__init__.py || { echo "FAIL: Missing __init__.py"; exit 1; }
echo "PASS: Python wrapper exists"

echo ""
echo "=========================================="
echo "ALL CHECKS PASSED"
echo "=========================================="

# Write reward
echo 1 > /logs/verifier/reward.txt
