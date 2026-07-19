"""Capture-helper wheel staging and source-distribution contracts."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

from scripts.packaging.stage_capture_helper import HELPER_NAME
from scripts.packaging.stage_capture_helper import stage_helper
from scripts.packaging.stage_ui_control_host import HOST_NAME
from scripts.packaging.stage_ui_control_host import stage_host

from conftest import REPO_ROOT


def test_stage_capture_helper_copies_a_windows_pe(tmp_path: Path) -> None:
    source = tmp_path / HELPER_NAME
    source.write_bytes(b"MZversion-matched-helper")
    python_root = tmp_path / "python"

    destination = stage_helper(source, python_root)

    assert destination == python_root / "dcc_mcp_core" / "bin" / HELPER_NAME
    assert destination.read_bytes() == source.read_bytes()


def test_stage_capture_helper_rejects_non_pe_input(tmp_path: Path) -> None:
    source = tmp_path / HELPER_NAME
    source.write_bytes(b"not-a-pe")

    try:
        stage_helper(source, tmp_path / "python")
    except ValueError as exc:
        assert "not a Windows PE" in str(exc)
    else:  # pragma: no cover - assertion guard
        raise AssertionError("non-PE helper was accepted")


def test_py37_lite_excludes_a_locally_staged_helper(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "build_py37_pure_wheel.py"
    text = script.read_text(encoding="utf-8")
    assert "WINDOWS_CAPTURE_HELPER" in text
    assert "continue" in text


def test_maturin_contract_keeps_helper_out_of_sdist() -> None:
    text = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    assert '{ path = "dcc_mcp_core/bin/dcc-mcp-capture-helper.exe", format = "wheel" }' in text
    assert '{ path = "dcc_mcp_core/bin/dcc-mcp-capture-helper.exe", format = "sdist" }' in text


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
            str(REPO_ROOT / "scripts" / "packaging" / "stage_capture_helper.py"),
            "--source",
            str(tmp_path / HELPER_NAME),
            "--python-root",
            str(tmp_path / "python"),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert "capture helper was not built" in result.stderr
