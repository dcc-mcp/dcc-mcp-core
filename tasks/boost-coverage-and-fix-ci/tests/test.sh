#!/bin/bash
# test.sh — Verification entry point for Harbor
# Validates all success criteria for the boost-coverage-and-fix-ci task.
set -uo pipefail

cd /workspace

PASS=0
FAIL=0
TOTAL=5

log() { echo "[VERIFY] $*"; }
pass() { log "PASS: $1"; PASS=$((PASS + 1)); }
fail() { log "FAIL: $1"; FAIL=$((FAIL + 1)); }

# --------------------------------------------------------------------------
# 1. Coverage >= 90%
# --------------------------------------------------------------------------
log "Checking code coverage..."
COV_OUTPUT=$(python -m pytest tests/ --cov=dcc_mcp_core --cov-report=term --tb=no -q --no-header 2>&1)
# Extract TOTAL line: e.g. "TOTAL   1234   123   90%"
TOTAL_COV=$(echo "$COV_OUTPUT" | grep "^TOTAL" | awk '{print $NF}' | tr -d '%')
if [ -n "$TOTAL_COV" ] && [ "$TOTAL_COV" -ge 90 ] 2>/dev/null; then
    pass "Coverage is ${TOTAL_COV}% (>= 90%)"
else
    fail "Coverage is ${TOTAL_COV:-unknown}% (< 90%)"
fi

# --------------------------------------------------------------------------
# 2. nox lint passes
# --------------------------------------------------------------------------
log "Checking nox lint..."
if python -m nox -s lint 2>&1 | tail -1 | grep -q "successful"; then
    pass "nox -s lint passes"
else
    fail "nox -s lint failed"
fi

# --------------------------------------------------------------------------
# 3. isort check passes
# --------------------------------------------------------------------------
log "Checking isort..."
if isort --check-only dcc_mcp_core tests nox_actions 2>&1; then
    pass "isort check passes"
else
    fail "isort check failed"
fi

# --------------------------------------------------------------------------
# 4. Python version compatibility: pyproject.toml still declares >=3.7
# --------------------------------------------------------------------------
log "Checking Python version declaration..."
if grep -q 'python = ">=3.7' pyproject.toml; then
    pass "pyproject.toml declares python >= 3.7"
else
    fail "pyproject.toml does not declare python >= 3.7"
fi

# --------------------------------------------------------------------------
# 5. CI matrix only includes 3.11+
# --------------------------------------------------------------------------
log "Checking CI matrix..."
if grep -q '"3.11"' .github/workflows/mr-test.yml && \
   ! grep -q '"3.8"' .github/workflows/mr-test.yml && \
   ! grep -q '"3.9"' .github/workflows/mr-test.yml; then
    pass "CI matrix correctly uses 3.11+"
else
    fail "CI matrix still includes old Python versions"
fi

# --------------------------------------------------------------------------
# Summary
# --------------------------------------------------------------------------
echo ""
log "Results: ${PASS}/${TOTAL} passed, ${FAIL}/${TOTAL} failed"

if [ "$FAIL" -eq 0 ]; then
    echo "1" > /logs/verifier/reward.txt
    log "ALL CHECKS PASSED"
    exit 0
else
    echo "0" > /logs/verifier/reward.txt
    log "SOME CHECKS FAILED"
    exit 1
fi
