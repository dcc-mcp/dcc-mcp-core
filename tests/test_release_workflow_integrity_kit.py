"""Tests for the shared release workflow integrity kit.

The kit in ``scripts/ci/release_workflow_integrity/`` is the upstream copy every
repository installs from. Two things can go wrong with a kit like that, and both
are tested here:

* A repository edits its installed copy locally and the kit silently forks. The
  byte-for-byte comparison against the templates catches that.
* The installer produces a repository that cannot actually enforce the check --
  a copy that passes on the day it lands but no longer binds the workflow. The
  installer is therefore exercised end to end against a throwaway repository,
  and the check it produces is proven to fail closed on a real change.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

import pytest

# Same guard as the sibling suite, for the same reason: this module drives the
# checker and the installer in subprocesses, so it needs PyYAML too. Skipping
# keeps a lane without PyYAML green instead of failing collection.
yaml = pytest.importorskip("yaml", reason="the release workflow digest checker needs PyYAML")

# Derived from this file rather than imported from conftest so the module runs
# under `--noconftest`, which is how the release-workflow-integrity job invokes
# it: that job installs only pytest and PyYAML, and tests/conftest.py imports
# the compiled dcc_mcp_core extension at module scope.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

KIT_DIR = REPO_ROOT / "scripts" / "ci" / "release_workflow_integrity"
INSTALLER = KIT_DIR / "install_release_workflow_integrity.py"
TEMPLATE_CHECKER = KIT_DIR / "check_release_workflow_digest.py"
TEMPLATE_TEST = KIT_DIR / "test_release_workflow_integrity.py"
INSTALLED_CHECKER = REPO_ROOT / "scripts" / "ci" / "check_release_workflow_digest.py"
INSTALLED_TEST = REPO_ROOT / "tests" / "test_release_workflow_integrity.py"

# Minimal but structurally real: one job, one publish permission, one step.
MINIMAL_WORKFLOW = """name: Release

on:
  push:
    branches: [main]

permissions: {}

jobs:
  publish:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
    steps:
      - run: echo publish
"""


def _make_repository(tmp_path: pathlib.Path) -> pathlib.Path:
    target = tmp_path / "repository"
    workflow = target / ".github" / "workflows"
    workflow.mkdir(parents=True)
    (workflow / "release.yml").write_text(MINIMAL_WORKFLOW, encoding="utf-8")
    return target


def _install(target: pathlib.Path, *extra: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(INSTALLER), "--target", str(target), *extra],
        capture_output=True,
        text=True,
    )


def _check(target: pathlib.Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(target / "scripts" / "ci" / "check_release_workflow_digest.py")],
        capture_output=True,
        text=True,
    )


def test_installed_kit_files_match_the_templates() -> None:
    """A local edit to one repository's copy must not fork the kit silently."""
    """A local edit to one repository's copy must not fork the kit silently."""
    assert INSTALLED_CHECKER.read_bytes() == TEMPLATE_CHECKER.read_bytes()
    assert INSTALLED_TEST.read_bytes() == TEMPLATE_TEST.read_bytes()


def test_installer_populates_an_empty_repository(tmp_path: pathlib.Path) -> None:
    target = _make_repository(tmp_path)

    result = _install(target)

    assert result.returncode == 0, result.stderr
    assert (target / "scripts" / "ci" / "check_release_workflow_digest.py").is_file()
    assert (target / "tests" / "test_release_workflow_integrity.py").is_file()
    assert (target / "scripts" / "ci" / "approved_release_workflow.yml").is_file()
    assert "verified" in result.stdout

    check = _check(target)
    assert check.returncode == 0, check.stderr
    assert "integrity ok" in check.stdout


def test_installed_check_fails_closed_on_a_real_change(tmp_path: pathlib.Path) -> None:
    """The ported check has to bind the workflow, not merely agree with it once."""
    target = _make_repository(tmp_path)
    assert _install(target).returncode == 0

    workflow = target / ".github" / "workflows" / "release.yml"
    workflow.write_text(
        MINIMAL_WORKFLOW.replace("permissions: {}", "permissions:\n  contents: write"),
        encoding="utf-8",
    )

    check = _check(target)
    assert check.returncode == 1
    assert "drifted" in check.stderr


def test_installed_check_ignores_cosmetic_drift(tmp_path: pathlib.Path) -> None:
    """A reformat must not page a reviewer: only meaning moves the digest."""
    target = _make_repository(tmp_path)
    assert _install(target).returncode == 0

    workflow = target / ".github" / "workflows" / "release.yml"
    workflow.write_bytes(
        b"""# a leading comment

permissions: {}
name: Release
jobs:
  publish:
    permissions:
      id-token: write
    runs-on: ubuntu-latest
    steps:
      - run: echo publish
on:
  push:
    branches: ['main']
"""
    )

    check = _check(target)
    assert check.returncode == 0, check.stderr


def test_installer_keeps_existing_files_without_force(tmp_path: pathlib.Path) -> None:
    target = _make_repository(tmp_path)
    assert _install(target).returncode == 0

    installed_checker = target / "scripts" / "ci" / "check_release_workflow_digest.py"
    installed_checker.write_text("# locally adjusted\n", encoding="utf-8")

    result = _install(target)

    assert result.returncode == 0, result.stderr
    assert installed_checker.read_text(encoding="utf-8") == "# locally adjusted\n"
    assert "kept" in result.stdout


def test_installer_force_restores_the_kit_copy(tmp_path: pathlib.Path) -> None:
    target = _make_repository(tmp_path)
    assert _install(target).returncode == 0

    installed_checker = target / "scripts" / "ci" / "check_release_workflow_digest.py"
    installed_checker.write_text("# locally adjusted\n", encoding="utf-8")

    assert _install(target, "--force").returncode == 0
    assert installed_checker.read_bytes() == TEMPLATE_CHECKER.read_bytes()


def test_installer_refuses_a_target_without_a_release_workflow(tmp_path: pathlib.Path) -> None:
    target = tmp_path / "empty"
    target.mkdir()

    result = _install(target)

    assert result.returncode == 1
    assert "no release workflow" in result.stderr


@pytest.mark.skipif(sys.version_info < (3, 8), reason="the installer reports paths with the 3.8+ repr")
def test_installer_accepts_a_custom_workflow_path(tmp_path: pathlib.Path) -> None:
    """A non-default workflow path is snapshotable; binding it needs one edit.

    The installed checker resolves ``RELEASE_WORKFLOW`` from its own location,
    so a repository that keeps its release workflow somewhere else also has to
    change that one constant. This test pins the installer's half of that
    contract: the snapshot it writes comes from the requested path.
    """
    target = tmp_path / "repository"
    workflow = target / "ci"
    workflow.mkdir(parents=True)
    (workflow / "publish.yml").write_text(MINIMAL_WORKFLOW, encoding="utf-8")

    result = _install(target, "--workflow", str(pathlib.Path("ci") / "publish.yml"))

    assert result.returncode == 0, result.stderr
    snapshot = target / "scripts" / "ci" / "approved_release_workflow.yml"
    assert snapshot.is_file()
    assert snapshot.read_bytes() == (workflow / "publish.yml").read_bytes()
