"""Lockfile version drift gate contract tests."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_lock_versions.py"
VERSION_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "version-consistency.yml"


def _load_checker_module():
    spec = importlib.util.spec_from_file_location("check_lock_versions", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _write_cargo_workspace(
    root: Path,
    *,
    members: list[str],
    workspace_version: str,
    member_versions: dict,
) -> None:
    (root / "Cargo.toml").write_text(
        f'[workspace]\nmembers = {members!r}\nresolver = "2"\n\n[workspace.package]\nversion = "{workspace_version}"\n',
        encoding="utf-8",
    )
    for member, version in member_versions.items():
        declared = "version.workspace = true" if version is None else f'version = "{version}"'
        (root / member).mkdir(parents=True, exist_ok=True)
        (root / member / "Cargo.toml").write_text(
            f'[package]\nname = "{Path(member).name}"\n{declared}\n',
            encoding="utf-8",
        )


def _write_cargo_lock(root: Path, pinned: dict) -> None:
    entries = [f'[[package]]\nname = "{name}"\nversion = "{version}"\n' for name, version in pinned.items()]
    (root / "Cargo.lock").write_text("version = 4\n\n" + "\n".join(entries), encoding="utf-8")


def _write_uv_pkg(
    root: Path,
    *,
    pkg_dir: str,
    name: str,
    declared: str,
    lock_version: str,
) -> None:
    (root / "pkg" / pkg_dir).mkdir(parents=True, exist_ok=True)
    (root / "pkg" / pkg_dir / "pyproject.toml").write_text(
        f'[project]\nname = "{name}"\nversion = "{declared}"\n',
        encoding="utf-8",
    )
    lock_path = root / "uv.lock"
    existing = lock_path.read_text(encoding="utf-8") if lock_path.exists() else ""
    existing += (
        "[[package]]\n"
        f'name = "{name}"\n'
        f'version = "{lock_version}"\n'
        'source = { registry = "https://pypi.org/simple" }\n\n'
    )
    lock_path.write_text(existing, encoding="utf-8")


def _write_changelog(root: Path, versions: list[str]) -> None:
    lines = ["# Changelog\n"]
    lines += [f"## [{version}](https://example.com/compare/v{version}) (2026-09-22)\n" for version in versions]
    (root / "CHANGELOG.md").write_text("\n".join(lines), encoding="utf-8")


def test_cargo_lock_matching_versions_passes(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_cargo_workspace(
        tmp_path,
        members=["crates/a", "crates/hack"],
        workspace_version="0.20.34",
        member_versions={"crates/a": None, "crates/hack": "0.1.0"},
    )
    _write_cargo_lock(tmp_path, {"a": "0.20.34", "hack": "0.1.0"})

    assert checker.check_cargo_lock_versions(tmp_path) == []


def test_cargo_lock_drifted_crate_is_reported(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_cargo_workspace(
        tmp_path,
        members=["crates/a"],
        workspace_version="0.20.34",
        member_versions={"crates/a": None},
    )
    _write_cargo_lock(tmp_path, {"a": "0.20.33"})

    errors = checker.check_cargo_lock_versions(tmp_path)

    assert len(errors) == 1
    assert "0.20.33" in errors[0]
    assert "0.20.34" in errors[0]


def test_cargo_lock_missing_member_is_reported(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_cargo_workspace(
        tmp_path,
        members=["crates/a"],
        workspace_version="0.20.34",
        member_versions={"crates/a": None},
    )
    _write_cargo_lock(tmp_path, {})

    assert any("missing workspace member" in error for error in checker.check_cargo_lock_versions(tmp_path))


def test_uv_lock_at_last_released_version_passes(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_changelog(tmp_path, ["0.20.33", "0.20.34"])
    # The in-flight 0.20.34 is excluded, so pinning the last published
    # release is the expected state during a release-please PR.
    _write_uv_pkg(
        tmp_path,
        pkg_dir="dcc-mcp-server-bin",
        name="dcc-mcp-server",
        declared="0.20.34",
        lock_version="0.20.33",
    )

    assert checker.check_uv_lock_published_versions(tmp_path) == []


def test_uv_lock_behind_last_released_version_fails(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_changelog(tmp_path, ["0.20.33", "0.20.34"])
    _write_uv_pkg(
        tmp_path,
        pkg_dir="dcc-mcp-server-bin",
        name="dcc-mcp-server",
        declared="0.20.34",
        lock_version="0.20.12",
    )

    errors = checker.check_uv_lock_published_versions(tmp_path)

    assert len(errors) == 1
    assert "0.20.12" in errors[0]
    assert "0.20.33" in errors[0]
    assert "uv lock --upgrade-package dcc-mcp-server" in errors[0]


def test_uv_lock_without_earlier_release_is_skipped(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_changelog(tmp_path, ["0.20.34"])
    _write_uv_pkg(
        tmp_path,
        pkg_dir="dcc-mcp-server-bin",
        name="dcc-mcp-server",
        declared="0.20.34",
        lock_version="0.20.12",
    )

    assert checker.check_uv_lock_published_versions(tmp_path) == []


def test_uv_lock_package_absent_from_lock_is_skipped(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_changelog(tmp_path, ["0.20.33", "0.20.34"])
    (tmp_path / "pkg" / "unresolved").mkdir(parents=True)
    (tmp_path / "pkg" / "unresolved" / "pyproject.toml").write_text(
        '[project]\nname = "not-in-lock"\nversion = "0.20.34"\n',
        encoding="utf-8",
    )

    assert checker.check_uv_lock_published_versions(tmp_path) == []


@pytest.mark.parametrize(
    ("versions", "expected"),
    [
        # Numeric ordering, not lexicographic: 0.10.0 > 0.9.0 and 0.20.34 > 0.20.9.
        (["0.9.0", "0.10.0"], "0.10.0"),
        (["0.20.9", "0.20.34", "0.20.33"], "0.20.34"),
    ],
)
def test_version_key_orders_numeric_parts(tmp_path: Path, versions: list[str], expected: str) -> None:
    checker = _load_checker_module()

    newest = max(versions, key=checker._version_key)

    assert newest == expected


@pytest.mark.parametrize(
    ("released", "in_flight", "expected"),
    [
        # The in-flight version is skipped, so the floor is the last release.
        (["0.20.33", "0.20.34"], "0.20.34", "0.20.33"),
        # Once the in-flight version is published it is still excluded, so the
        # floor stays one release behind and the gate does not fire on main.
        (["0.20.33", "0.20.34"], "0.20.35", "0.20.34"),
        # Lexicographic traps must not win.
        (["0.20.9", "0.20.33"], "0.20.34", "0.20.33"),
    ],
)
def test_newest_released_except_skips_in_flight_version(
    tmp_path: Path, released: list[str], in_flight: str, expected: str
) -> None:
    checker = _load_checker_module()

    assert checker._newest_released_except(released, in_flight) == expected


def test_newest_released_except_without_candidates(tmp_path: Path) -> None:
    checker = _load_checker_module()

    assert checker._newest_released_except(["0.20.34"], "0.20.34") is None


def test_main_reports_errors_and_returns_failure(tmp_path: Path, capsys) -> None:
    checker = _load_checker_module()
    _write_changelog(tmp_path, ["0.20.33", "0.20.34"])
    _write_uv_pkg(
        tmp_path,
        pkg_dir="dcc-mcp-core-semantic",
        name="dcc-mcp-core-semantic",
        declared="0.20.34",
        lock_version="0.20.12",
    )

    assert checker.main([str(tmp_path)]) == 1
    assert "::error::" in capsys.readouterr().err


def test_main_succeeds_on_clean_repository() -> None:
    checker = _load_checker_module()

    assert checker.main([str(REPO_ROOT)]) == 0


def test_version_workflow_runs_the_lock_version_gate() -> None:
    workflow = VERSION_WORKFLOW.read_text(encoding="utf-8")

    assert "scripts/ci/check_lock_versions.py" in workflow
