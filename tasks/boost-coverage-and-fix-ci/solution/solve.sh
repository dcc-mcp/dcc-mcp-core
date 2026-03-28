#!/bin/bash
# solve.sh — Reference solution for boost-coverage-and-fix-ci task
# This script demonstrates the steps an agent should take to complete the task.
set -euo pipefail

cd /workspace

# ============================================================================
# Step 1: Branch Setup
# ============================================================================
git fetch origin
git checkout -b codecov origin/main 2>/dev/null || {
    git checkout codecov
    git rebase origin/main --no-interactive || {
        git rebase --abort
        git merge origin/main --no-edit
    }
}
git merge origin/feat/skill-system --no-edit

# Re-install after merge (new modules added)
pip install -e ".[yaml]" --quiet

# ============================================================================
# Step 2: Baseline coverage
# ============================================================================
echo "=== Baseline coverage ==="
python -m pytest tests/ --cov=dcc_mcp_core --cov-report=term-missing --tb=short -q --no-header 2>&1 | tail -30

# ============================================================================
# Step 3: Write Phase 1 tests (target ~87%)
# ============================================================================
# Agent should analyze uncovered lines and write tests/test_coverage_boost.py
# covering: actions/base async, protocols, exceptions, result_factory,
# filesystem, pydantic_extensions, template, registry, middleware async, manager
echo "=== Phase 1: Writing coverage boost tests ==="
# (Agent generates test file here based on coverage gaps)

python -m pytest tests/ --cov=dcc_mcp_core --cov-report=term-missing --tb=short -q --no-header 2>&1 | tail -5
git add tests/test_coverage_boost.py
git commit -m "test: add comprehensive coverage boost tests (81% -> 87%)"
git push origin codecov

# ============================================================================
# Step 4: Write Phase 2 tests (target ≥90%)
# ============================================================================
# Agent should write tests/test_coverage_boost_phase2.py covering:
# script_action subprocess, events async, function_adapter, log_config loguru,
# skill loader/scanner edge cases
echo "=== Phase 2: Writing more coverage tests ==="
# (Agent generates second test file here)

python -m pytest tests/ --cov=dcc_mcp_core --cov-report=term-missing --tb=short -q --no-header 2>&1 | tail -5
git add tests/test_coverage_boost_phase2.py
git commit -m "test: add phase 2 coverage tests (87% -> 91%)"
git push origin codecov

# ============================================================================
# Step 5: Fix lint errors
# ============================================================================
echo "=== Fixing lint errors ==="
python -m nox -s lint 2>&1 || true

# Fix source code lint issues:
# - RUF022: sort __all__ alphabetically in __init__.py
# - UP015: remove unnecessary "r" mode in open() calls
# - D401: use imperative mood in docstrings
# - F401: remove unused imports
# - RUF005: use iterable unpacking instead of list concatenation
# (Agent applies targeted edits to source files)

# Fix test file formatting
isort tests/test_coverage_boost.py tests/test_coverage_boost_phase2.py
ruff format tests/test_coverage_boost.py tests/test_coverage_boost_phase2.py

# Verify
python -m nox -s lint
git add -A
git commit -m "style: fix all ruff lint errors to pass nox lint session"
git push origin codecov

# ============================================================================
# Step 6: Fix CI matrix
# ============================================================================
echo "=== Updating CI matrix ==="
# Update .github/workflows/mr-test.yml:
#   python-version: ["3.11", "3.12", "3.13"]
# Update pyproject.toml [tool.nox] python list
# Add Python 3.13 classifier
# Keep python = ">=3.7,<4.0" unchanged
# (Agent applies targeted edits)

git add .github/workflows/mr-test.yml pyproject.toml
git commit -m "ci: update CI matrix to 3.11-3.13, keep code compatible with 3.7+"
git push origin codecov

echo "=== Done ==="
echo "Coverage target reached, lint passes, CI matrix updated."
echo "Agent should now create a PR from codecov -> main."
