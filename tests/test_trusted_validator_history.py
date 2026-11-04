"""Require trusted workflow pins to survive rebase merge and fresh main clones."""

from __future__ import annotations

from pathlib import Path
import subprocess

import pytest


def _git(root: Path, *args: str) -> subprocess.CompletedProcess:
    """Run a bounded local Git operation with commit signing and hooks disabled."""
    return subprocess.run(
        ["git", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=", *args],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )


def assert_landed_validator_pin(root: Path, ref: str) -> None:
    """Fail full-history gates unless the immutable validator is already on main."""
    shallow = _git(root, "rev-parse", "--is-shallow-repository")
    if shallow.returncode == 0 and shallow.stdout.strip() == "true":
        pytest.skip("validator ancestry cannot be established in a shallow checkout")
    if shallow.returncode != 0 or shallow.stdout.strip() != "false":
        pytest.fail("cannot establish full Git history for trusted validator ancestry")
    result = _git(root, "merge-base", "--is-ancestor", ref, "refs/remotes/origin/main")
    if result.returncode != 0:
        pytest.fail(f"trusted validator {ref} must already be reachable from origin/main before use")


def _checked_git(root: Path, *args: str) -> str:
    """Run a fixture operation and return its output, failing on any Git error."""
    result = _git(root, *args)
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


@pytest.fixture
def repository(tmp_path: Path) -> Path:
    """Create a disposable full-history main/feature repository with local identity."""
    root = tmp_path / "repository"
    root.mkdir()
    _checked_git(root, "init", "-q", "-b", "main")
    _checked_git(root, "config", "user.name", "Validator History Fixture")
    _checked_git(root, "config", "user.email", "validator-history@example.invalid")
    (root / "README").write_text("base\n", encoding="utf-8")
    _checked_git(root, "add", "README")
    _checked_git(root, "commit", "-qm", "fixture baseline")
    _checked_git(root, "update-ref", "refs/remotes/origin/main", "HEAD")
    return root


def test_pr_local_pin_is_rejected_even_when_its_object_is_available(repository: Path) -> None:
    """A green feature checkout cannot approve its own unpublished trust anchor."""
    _checked_git(repository, "switch", "-c", "feature")
    (repository / "validator.py").write_text("print('trusted fixture')\n", encoding="utf-8")
    _checked_git(repository, "add", "validator.py")
    _checked_git(repository, "commit", "-qm", "fixture validator")
    candidate = _checked_git(repository, "rev-parse", "HEAD")
    _checked_git(repository, "cat-file", "-e", candidate)
    with pytest.raises(pytest.fail.Exception, match="must already be reachable from origin/main"):
        assert_landed_validator_pin(repository, candidate)


def test_rebased_pin_is_accepted_and_old_pr_id_is_rejected(repository: Path, tmp_path: Path) -> None:
    """Rebase keeps source bytes but changes IDs; only the landed ID survives cloning."""
    _checked_git(repository, "switch", "-c", "feature")
    (repository / "validator.py").write_text("print('trusted fixture')\n", encoding="utf-8")
    _checked_git(repository, "add", "validator.py")
    _checked_git(repository, "commit", "-qm", "fixture validator")
    original = _checked_git(repository, "rev-parse", "HEAD")
    _checked_git(repository, "switch", "main")
    (repository / "README").write_text("main advanced\n", encoding="utf-8")
    _checked_git(repository, "add", "README")
    _checked_git(repository, "commit", "-qm", "fixture advance main")
    _checked_git(repository, "switch", "feature")
    _checked_git(repository, "rebase", "main")
    landed = _checked_git(repository, "rev-parse", "HEAD")
    assert landed != original
    assert _checked_git(repository, "rev-parse", original + ":validator.py") == _checked_git(
        repository, "rev-parse", landed + ":validator.py"
    )
    _checked_git(repository, "switch", "main")
    _checked_git(repository, "merge", "--ff-only", "feature")
    _checked_git(repository, "update-ref", "refs/remotes/origin/main", "HEAD")
    assert_landed_validator_pin(repository, landed)
    with pytest.raises(pytest.fail.Exception, match="must already be reachable from origin/main"):
        assert_landed_validator_pin(repository, original)
    clone = tmp_path / "main-clone"
    _checked_git(repository, "clone", "--no-local", "--single-branch", "--branch", "main", str(repository), str(clone))
    assert _checked_git(clone, "rev-parse", "--is-shallow-repository") == "false"
    assert _git(clone, "cat-file", "-e", original).returncode != 0
    assert_landed_validator_pin(clone, landed)


def test_full_checkout_without_default_branch_ref_fails_closed(repository: Path) -> None:
    """Missing ancestry evidence is a failure, not permission to bless a pin."""
    landed = _checked_git(repository, "rev-parse", "HEAD")
    _checked_git(repository, "update-ref", "-d", "refs/remotes/origin/main")
    with pytest.raises(pytest.fail.Exception, match="must already be reachable from origin/main"):
        assert_landed_validator_pin(repository, landed)
