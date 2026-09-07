"""Exercise the generated-lock allowlist against real Git worktree states."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "ci" / "generated_lock_sync.py"


def _git(root, *args):
    return subprocess.run(
        ["git", "-c", "user.name=loonghao", "-c", "user.email=hal.long@outlook.com", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )


def _write(root, name, content):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _repository(root, tracked=("Cargo.lock",)):
    _git(root, "init", "-q")
    _write(root, "README.md", "fixture\n")
    for name in tracked:
        _write(root, name, "baseline\n")
    _git(root, "add", ".")
    _git(root, "commit", "-qm", "test: seed generated lock fixture")


def _verify(root):
    return subprocess.run(
        [sys.executable, str(SCRIPT), "verify-diff", "--root", str(root)],
        capture_output=True,
        text=True,
        timeout=30,
    )


@pytest.mark.parametrize("name", ["Cargo.lock", "uv.lock", "crates/workspace-hack/Cargo.toml"])
@pytest.mark.parametrize("state", ["unstaged", "staged", "untracked"])
def test_generated_lock_paths_are_accepted(tmp_path, name, state):
    _repository(tmp_path, tracked=() if state == "untracked" else (name,))
    _write(tmp_path, name, "regenerated\n")
    if state == "staged":
        _git(tmp_path, "add", name)

    result = _verify(tmp_path)

    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("name", ["evil.py", "Cargo.lock.bak", "space name.txt"])
@pytest.mark.parametrize("state", ["unstaged", "staged", "untracked"])
def test_unexpected_paths_are_still_rejected(tmp_path, name, state):
    _repository(tmp_path, tracked=() if state == "untracked" else (name,))
    _write(tmp_path, name, "unexpected\n")
    if state == "staged":
        _git(tmp_path, "add", name)

    result = _verify(tmp_path)

    assert result.returncode != 0
    assert "unexpected generated-lock diff paths" in result.stderr
    assert name in result.stderr


@pytest.mark.parametrize("source,target", [("Cargo.lock", "evil.py"), ("evil.py", "Cargo.lock")])
def test_renames_check_both_paths(tmp_path, source, target):
    _repository(tmp_path, tracked=(source,))
    _git(tmp_path, "mv", source, target)

    result = _verify(tmp_path)

    assert result.returncode != 0
    assert "evil.py" in result.stderr


def test_multiple_generated_changes_keep_the_first_path_intact(tmp_path):
    _repository(tmp_path, tracked=("Cargo.lock", "uv.lock"))
    _write(tmp_path, "Cargo.lock", "regenerated\n")
    _write(tmp_path, "uv.lock", "regenerated\n")

    result = _verify(tmp_path)

    assert result.returncode == 0, result.stdout + result.stderr
