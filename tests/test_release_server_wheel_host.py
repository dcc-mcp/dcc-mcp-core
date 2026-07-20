"""Server-wheel Windows UI Control host injection tests."""

from __future__ import annotations

import base64
import hashlib
from pathlib import Path
import zipfile

import pytest
from scripts.release.inject_ui_control_host import inject_host
from scripts.release.server_wheel_tags import _validate


def _write_server_wheel(path: Path) -> None:
    dist_info = "dcc_mcp_server-1.0.0.dist-info"
    data = "dcc_mcp_server-1.0.0.data/scripts"
    with zipfile.ZipFile(str(path), "w") as wheel:
        wheel.writestr(
            f"{dist_info}/METADATA",
            "Metadata-Version: 2.1\nName: dcc-mcp-server\nVersion: 1.0.0\nRequires-Python: >=3.7\n",
        )
        wheel.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: py3-none-win_amd64\n",
        )
        wheel.writestr(f"{data}/dcc-mcp-server.exe", b"MZserver")
        # Minimal RECORD so the injector can find one.
        wheel.writestr(
            f"{dist_info}/RECORD",
            "dcc_mcp_server-1.0.0.dist-info/METADATA,,\ndcc_mcp_server-1.0.0.dist-info/WHEEL,,\ndcc_mcp_server-1.0.0.data/scripts/dcc-mcp-server.exe,,\n",
        )


def test_inject_host_places_single_companion_and_rewrites_record(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-cp311-none-win_amd64.whl"
    host = tmp_path / "dcc-mcp-ui-control-host.exe"
    host.write_bytes(b"MZhost")
    _write_server_wheel(wheel)

    inject_host(wheel, host)

    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        assert len(names) == len(set(names))
        assert archive.read("dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-ui-control-host.exe") == b"MZhost"
        record = archive.read("dcc_mcp_server-1.0.0.dist-info/RECORD").decode("utf-8")
        host_digest = base64.urlsafe_b64encode(hashlib.sha256(b"MZhost").digest()).rstrip(b"=").decode("ascii")
        assert (f"dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-ui-control-host.exe,sha256={host_digest},6") in record


def test_inject_host_rejects_wheel_without_server_script(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-cp311-none-win_amd64.whl"
    host = tmp_path / "dcc-mcp-ui-control-host.exe"
    host.write_bytes(b"MZhost")

    with zipfile.ZipFile(str(wheel), "w") as archive:
        archive.writestr("dcc_mcp_server-1.0.0.dist-info/METADATA", "Name: dcc-mcp-server\nVersion: 1.0.0\n")
        archive.writestr(
            "dcc_mcp_server-1.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp311-none-win_amd64\n",
        )
        archive.writestr("dcc_mcp_server-1.0.0.dist-info/RECORD", "dcc_mcp_server-1.0.0.dist-info/METADATA,,\n")

    try:
        inject_host(wheel, host)
    except ValueError as exc:
        assert "expected one bundled dcc-mcp-server.exe" in str(exc)
    else:  # pragma: no cover - assertion guard
        raise AssertionError("wheel without server script was accepted")


def test_inject_host_rejects_removed_capture_helper(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-cp311-none-win_amd64.whl"
    host = tmp_path / "dcc-mcp-ui-control-host.exe"
    host.write_bytes(b"MZhost")
    _write_server_wheel(wheel)
    with zipfile.ZipFile(str(wheel), "a") as archive:
        archive.writestr(
            "dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-capture-helper.exe",
            b"MZremoved",
        )

    try:
        inject_host(wheel, host)
    except ValueError as exc:
        assert "removed capture helper" in str(exc)
    else:  # pragma: no cover - assertion guard
        raise AssertionError("wheel with removed capture helper was accepted")


def test_server_wheel_validation_rejects_removed_capture_helper(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-py3-none-win_amd64.whl"
    _write_server_wheel(wheel)
    with zipfile.ZipFile(str(wheel), "a") as archive:
        archive.writestr(
            "dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-ui-control-host.exe",
            b"MZhost",
        )
        archive.writestr(
            "dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-capture-helper.exe",
            b"MZremoved",
        )

    with pytest.raises(SystemExit):
        _validate(tmp_path)
