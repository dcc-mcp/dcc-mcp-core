"""Windows UI Control host staging and capture-worker packaging contracts."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

from scripts.packaging.stage_ui_control_host import HOST_NAME
from scripts.packaging.stage_ui_control_host import stage_host

from conftest import REPO_ROOT


def test_py37_lite_excludes_a_locally_staged_host(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "build_py37_pure_wheel.py"
    text = script.read_text(encoding="utf-8")
    assert "WINDOWS_UI_CONTROL_HOST" in text
    assert "continue" in text


def test_maturin_contract_packages_only_host() -> None:
    text = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    assert '{ path = "dcc_mcp_core/bin/dcc-mcp-ui-control-host.exe", format = "wheel" }' in text


def test_stage_ui_control_host_copies_a_windows_pe(tmp_path: Path) -> None:
    source = tmp_path / HOST_NAME
    source.write_bytes(b"MZversion-matched-ui-control-host")
    python_root = tmp_path / "python"

    destination = stage_host(source, python_root)

    assert destination == python_root / "dcc_mcp_core" / "bin" / HOST_NAME
    assert destination.read_bytes() == source.read_bytes()


def test_ui_control_host_is_wheel_only_and_generated_binary_is_ignored() -> None:
    pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    gitignore = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8")
    assert '{ path = "dcc_mcp_core/bin/dcc-mcp-ui-control-host.exe", format = "wheel" }' in pyproject
    assert '{ path = "dcc_mcp_core/bin/dcc-mcp-ui-control-host.exe", format = "sdist" }' in pyproject
    assert "/python/dcc_mcp_core/bin/dcc-mcp-ui-control-host.exe" in gitignore


def test_missing_stage_source_fails_with_actionable_error(tmp_path: Path) -> None:
    result = subprocess.run(
        [
            sys.executable,
            str(REPO_ROOT / "scripts" / "packaging" / "stage_ui_control_host.py"),
            "--source",
            str(tmp_path / HOST_NAME),
            "--python-root",
            str(tmp_path / "python"),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert "UI Control host was not built" in result.stderr
