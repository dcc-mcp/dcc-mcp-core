# Task: Boost Code Coverage to 90% and Fix CI Pipeline

## Objective

You are working on the `dcc-mcp-core` Python project — a foundational library for the DCC Model Context Protocol (MCP) ecosystem. Your task has three parts:

1. **Merge a feature branch and boost code coverage from 81% to ≥90%**
2. **Fix all lint errors so the CI pipeline passes**
3. **Update the CI matrix to only test Python versions that can actually install**

## Repository

- **URL**: `https://github.com/loonghao/dcc-mcp-core`
- **Starting branch**: `main`
- **Target branch for PR**: `main`
- **Working branch**: `codecov` (create from `origin/main`)

## Context

- The project uses `poetry` for packaging, `ruff` + `isort` for linting, `pytest` for testing, and `nox` for task automation.
- A feature branch `feat/skill-system` adds protocols, skills, scanner, loader, and script_action modules. It should be merged into the working branch before writing tests.
- The code must remain compatible with **Python >=3.7**, but CI can skip Python <3.11 because dev dependencies (pydantic >=2.x, etc.) no longer support older versions.
- CI runs via GitHub Actions: `vx prek --all-files` → `vx uvx nox -s lint` → `vx uvx nox -s pytest`.

## Steps

### Step 1 — Branch Setup

1. Fetch `origin` and create branch `codecov` from `origin/main`. If it already exists, rebase onto `origin/main` (non-interactive). If rebase fails, merge instead.
2. Merge `origin/feat/skill-system` into `codecov`.

### Step 2 — Boost Coverage (81% → 90%+)

1. Run `pytest --cov=dcc_mcp_core --cov-report=term-missing` to establish the baseline (~81%).
2. Identify modules with low coverage (e.g. `script_action.py`, `manager.py`, `server.py`, `events.py`, `function_adapter.py`, `log_config.py`, `scanner.py`, `loader.py`).
3. Write targeted test files (`tests/test_coverage_boost.py`, `tests/test_coverage_boost_phase2.py`) covering the missing lines.
4. After reaching ~87% (Phase 1), commit and push. After reaching ≥90% (Phase 2), commit and push again.

### Step 3 — Fix Lint Errors

1. Run `python -m nox -s lint` (which installs fresh `ruff` + `isort` in a virtualenv) to discover lint errors.
2. Fix all errors in source code:
   - **RUF022**: Sort `__all__` alphabetically.
   - **UP015**: Remove unnecessary `"r"` mode in `open()` calls.
   - **D401**: Use imperative mood in docstrings.
   - **F401**: Remove unused imports.
   - **RUF005**: Use iterable unpacking `[..., *args]` instead of `list + list`.
3. Run `isort` and `ruff format` on test files.
4. Update `pyproject.toml` `[tool.ruff.lint.per-file-ignores]` for tests if needed (e.g. add `D103`, `D106`, `F841`).
5. Commit and push. Verify all `nox -s lint` and `vx prek --all-files` pass.

### Step 4 — Fix CI Matrix

1. Update `.github/workflows/mr-test.yml`: change `python-version` matrix from `["3.8", "3.9", "3.10", "3.11", "3.12"]` to `["3.11", "3.12", "3.13"]`.
2. Add a comment explaining that code targets `>=3.7` but CI tests `3.11+` due to dev dependency constraints.
3. Update `[tool.nox]` `python` list in `pyproject.toml` accordingly.
4. Add `"Programming Language :: Python :: 3.13"` to classifiers.
5. Keep `python = ">=3.7,<4.0"`, `target-version = "py37"`, `python_version = "3.7"` unchanged (code compatibility).
6. Commit and push.

### Step 5 — Create PR and Verify

1. Create a PR from `codecov` → `main`.
2. All CI checks (python-check 3.11/3.12/3.13 × 3 OS, Code Coverage, codecov/patch) must pass.

## Success Criteria

- [ ] Code coverage ≥ 90% (measured by `pytest --cov=dcc_mcp_core`)
- [ ] `vx prek --all-files` passes (pre-commit hooks)
- [ ] `python -m nox -s lint` passes (ruff + isort in isolated venv)
- [ ] All CI jobs pass (python-check on 3.11, 3.12, 3.13 × ubuntu, windows, macos)
- [ ] `pyproject.toml` still declares `python = ">=3.7,<4.0"`
- [ ] PR is created and all checks are green

## Constraints

- Do NOT use Python 3.10+ syntax (e.g. `match/case`, `X | Y` union types, lowercase generics like `list[str]`). The source code must parse on Python 3.7.
- Do NOT modify existing source code behavior — only add tests and fix lint issues.
- Commit at each major milestone (Phase 1 coverage, Phase 2 coverage, lint fix, CI matrix fix).
