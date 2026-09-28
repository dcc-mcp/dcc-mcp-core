"""Guards for repository agent instruction files.

`AGENTS.md` is the **single hand-written agent contract file** at the repo root
(plan C, PIP-3736). Vendor-named entry points (`CLAUDE.md`, `GEMINI.md`,
`COPILOT.md`, `CODEBUDDY.md`, `CURSOR.md`, `ANTHROPIC.md`, `OPENAI.md`) and root
rule files (`.cursorrules`, `.clinerules`, `.windsurfrules`) are deliberately
absent so guidance cannot drift across N hand-maintained copies.

These guards must fail loudly. A guard that silently skips is indistinguishable
from a guard that passes, so every guard here asserts on a real result instead
of returning early when its input is unavailable.
"""

from __future__ import annotations

import os
from pathlib import Path
import subprocess

REPO_ROOT = Path(__file__).resolve().parents[1]
# `AGENTS.md` is the single agent contract file at the repository root. Every
# mainstream agent runtime reads it natively (Claude Code falls back to it when
# no CLAUDE.md exists), so vendor-specific files must not come back.
AGENT_ENTRYPOINTS = ("AGENTS.md",)
DEPRECATED_AGENT_ENTRYPOINTS = (
    "CLAUDE.md",
    "GEMINI.md",
    "COPILOT.md",
    "CODEBUDDY.md",
    "CURSOR.md",
    "ANTHROPIC.md",
    "OPENAI.md",
    "AI_AGENT_GUIDE.md",
    ".cursorrules",
    ".clinerules",
    ".windsurfrules",
)

FORBIDDEN_MARKERS = (
    "BEGIN MULTICA-RUNTIME",
    "END MULTICA-RUNTIME",
    "Multica Agent Runtime",
)
FORBIDDEN_TRACKED_PREFIXES = (
    ".multica/",
    ".agent_context/",
)
# An inherited GIT_DIR / GIT_WORK_TREE / GIT_INDEX_FILE would redirect
# `git ls-files` at another repository and quietly empty this guard's input.
_GIT_LOCATION_ENV_VARS = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE")


def _git_env() -> dict[str, str]:
    """Environment for `git ls-files`, with repo-pointing overrides stripped."""
    return {key: value for key, value in os.environ.items() if key not in _GIT_LOCATION_ENV_VARS}


def _tracked_files() -> list[str]:
    """Return the repository's git-tracked paths, normalised to forward slashes.

    Unlike a working-tree walk, `git ls-files` reads the index, so it also sees
    staged additions and files that a sparse checkout left out of the tree. The
    trade-off is that an untracked stray is invisible here; that is acceptable
    because a stray that is never staged can never reach `main` either.
    """
    result = subprocess.run(
        ["git", "ls-files"],
        cwd=REPO_ROOT,
        text=True,
        encoding="utf-8",
        capture_output=True,
        env=_git_env(),
    )
    # Fail loudly instead of returning []: an empty list would satisfy every
    # assertion below and turn all three guards into no-ops.
    assert result.returncode == 0, (
        "`git ls-files` failed, so the agent contract guards have no input to check. "
        f"exit={result.returncode} stderr={result.stderr.strip()}"
    )
    return [line.strip().replace("\\", "/") for line in result.stdout.splitlines() if line.strip()]


def test_agent_entrypoints_do_not_include_multica_runtime_context() -> None:
    tracked = set(_tracked_files())
    for relative_path in AGENT_ENTRYPOINTS:
        assert relative_path in tracked, f"{relative_path} is the single agent contract file and must stay tracked"
        path = REPO_ROOT / relative_path
        assert path.is_file(), (
            f"{relative_path} is tracked but missing from the working tree; restore it so this "
            "guard can read its contents"
        )
        text = path.read_text(encoding="utf-8")
        for marker in FORBIDDEN_MARKERS:
            assert marker not in text, f"{relative_path} contains generated Multica marker {marker!r}"


def test_agents_md_is_the_only_agent_contract_file() -> None:
    tracked = set(_tracked_files())
    assert "AGENTS.md" in tracked, "AGENTS.md is the single agent contract file and must stay tracked"
    offenders = sorted(name for name in DEPRECATED_AGENT_ENTRYPOINTS if name in tracked)
    assert offenders == [], (
        "AGENTS.md is the single source of agent guidance; remove these vendor files and "
        f"fold their unique content into AGENTS.md: {offenders}"
    )


def test_multica_runtime_artifacts_are_not_tracked() -> None:
    tracked = _tracked_files()
    offenders = [
        path
        for path in tracked
        if any(path == prefix.rstrip("/") or path.startswith(prefix) for prefix in FORBIDDEN_TRACKED_PREFIXES)
    ]
    assert offenders == []
