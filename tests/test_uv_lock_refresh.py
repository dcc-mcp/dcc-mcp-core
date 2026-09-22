"""Scheduled uv.lock refresh guard contract tests."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "uv_lock_refresh.py"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "uv-lock-refresh.yml"
# Security boundary owned by release-please-lock-sync.yml; the refresh workflow
# must never execute or redefine it.
PINNED_VALIDATOR_REF = "b8e294e8a64abba426871b1e86eb106e75aab075"


def _load_module():
    spec = importlib.util.spec_from_file_location("uv_lock_refresh", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _wheel(package: str, version: str, tag: str) -> str:
    return f"https://files.pythonhosted.org/packages/ab/cd/{package}-{version}-{tag}.whl"


def _semantic_wheels(version: str, *, cp37: bool = True) -> list:
    wheels = []
    if cp37:
        wheels.append(_wheel("dcc_mcp_core_semantic", version, "cp37-cp37m-manylinux_2_28_x86_64"))
    wheels.append(_wheel("dcc_mcp_core_semantic", version, "cp38-abi3-manylinux_2_28_x86_64"))
    return wheels


def _server_wheels(version: str) -> list:
    return [_wheel("dcc_mcp_server", version, "py3-none-manylinux2014_x86_64")]


def _parse(text: str) -> dict:
    try:
        import tomllib
    except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
        import tomli as tomllib

    return tomllib.loads(text)


def _lock_text(*, requires_python: str = ">=3.7", packages: list) -> str:
    lines = ["version = 1", "revision = 3", f'requires-python = "{requires_python}"', ""]
    for name, version, wheels in packages:
        lines.append("[[package]]")
        lines.append(f'name = "{name}"')
        lines.append(f'version = "{version}"')
        if wheels:
            lines.append("wheels = [")
            for url in wheels:
                lines.append(f'    {{ url = "{url}", hash = "sha256:deadbeef" }},')
            lines.append("]")
        lines.append("")
    return "\n".join(lines)


def _baseline():
    return [
        ("dcc-mcp-core-semantic", "0.20.33", _semantic_wheels("0.20.33")),
        ("dcc-mcp-server", "0.20.33", _server_wheels("0.20.33")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")]),
    ]


def _refreshed():
    return [
        ("dcc-mcp-core-semantic", "0.20.34", _semantic_wheels("0.20.34")),
        ("dcc-mcp-server", "0.20.34", _server_wheels("0.20.34")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")]),
    ]


def test_unchanged_lock_passes() -> None:
    module = _load_module()
    lock = _parse(_lock_text(packages=_baseline()))

    assert module.verify_refresh(lock, lock) == []


def test_allowed_package_bump_passes() -> None:
    module = _load_module()

    assert (
        module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=_refreshed()))) == []
    )


def test_unmanaged_package_bump_fails() -> None:
    module = _load_module()
    drifted = _refreshed()
    drifted[2] = ("zipp", "3.21.0", [_wheel("zipp", "3.21.0", "py3-none-any")])

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=drifted)))

    assert len(errors) == 1
    assert "zipp" in errors[0]
    assert "outside the allowed refresh set" in errors[0]


def test_dropped_cp37_wheel_fails() -> None:
    module = _load_module()
    without_cp37 = _refreshed()
    without_cp37[0] = ("dcc-mcp-core-semantic", "0.20.34", _semantic_wheels("0.20.34", cp37=False))

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=without_cp37)))

    assert any("cp37" in error and "dcc-mcp-core-semantic" in error for error in errors)


def test_narrowed_requires_python_fails() -> None:
    module = _load_module()
    before = _parse(_lock_text(packages=_baseline()))
    after = _parse(_lock_text(requires_python=">=3.9", packages=_baseline()))

    errors = module.verify_refresh(before, after)

    assert any("requires-python" in error for error in errors)


def test_removed_allowed_pin_fails() -> None:
    module = _load_module()
    trimmed = [entry for entry in _refreshed() if entry[0] != "dcc-mcp-server"]

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=trimmed)))

    assert any("dcc-mcp-server" in error and "removed" in error for error in errors)


def test_cli_verify_accepts_a_scoped_refresh(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(packages=_refreshed()), encoding="utf-8")

    assert module.main(["verify", str(before), str(after)]) == 0


def test_cli_verify_rejects_an_unscoped_refresh(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(requires_python=">=3.10", packages=_baseline()), encoding="utf-8")

    assert module.main(["verify", str(before), str(after)]) == 1


def test_cli_verify_honours_a_package_override(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    moved = _baseline()
    moved[2] = ("zipp", "3.21.0", [_wheel("zipp", "3.21.0", "py3-none-any")])
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(packages=moved), encoding="utf-8")

    assert module.main(["verify", str(before), str(after), "--package", "zipp"]) == 0
    assert module.main(["verify", str(before), str(after)]) == 1


@pytest.mark.parametrize(
    "expected",
    [
        "uv lock --upgrade-package dcc-mcp-core-semantic --upgrade-package dcc-mcp-server",
        "scripts/ci/uv_lock_refresh.py verify",
        "scripts/ci/check_lock_versions.py",
        "scripts/ci/check_uv_lock.py",
        "uv lock --check",
        "chore(lock):",
        "bot/uv-lock-refresh",
        "git diff --quiet -- uv.lock",
    ],
)
def test_workflow_carries_the_refresh_contract(expected: str) -> None:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    assert expected in workflow


def _workflow_body() -> str:
    """Return the workflow with its explanatory comments stripped."""
    lines = [
        line for line in WORKFLOW_PATH.read_text(encoding="utf-8").splitlines() if not line.lstrip().startswith("#")
    ]
    return "\n".join(lines)


def test_workflow_never_touches_the_pinned_validator_boundary() -> None:
    workflow = _workflow_body()

    assert PINNED_VALIDATOR_REF not in workflow
    assert "pull_request_target" not in workflow
    assert "generated_lock_sync.py" not in workflow


def test_workflow_schedule_avoids_the_hour_and_the_release_window() -> None:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    cron_lines = [line.strip() for line in workflow.splitlines() if line.strip().startswith("- cron:")]
    assert cron_lines, "the refresh workflow must declare a schedule"

    for line in cron_lines:
        expression = line.split("cron:", 1)[1].strip().strip('"')
        minute, hour = expression.split()[0], expression.split()[1]
        assert minute != "0"
        assert minute != "00"
        assert not (hour == "1" and minute == "13")
        assert hour == "*" or int(hour) != 1
