"""Server-wheel capture-helper injection tests."""

from __future__ import annotations

import base64
import hashlib
from pathlib import Path
import zipfile

from scripts.release.inject_capture_helper import inject_helper


def _write_server_wheel(path: Path) -> None:
    dist_info = "dcc_mcp_server-1.0.0.dist-info"
    data = "dcc_mcp_server-1.0.0.data/scripts"
    with zipfile.ZipFile(str(path), "w") as wheel:
        wheel.writestr(f"{dist_info}/METADATA", "Metadata-Version: 2.1\nName: dcc-mcp-server\nVersion: 1.0.0\n")
        wheel.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp311-none-win_amd64\n",
        )
        wheel.writestr(f"{data}/dcc-mcp-server.exe", b"MZserver")
        # Minimal RECORD so inject_helper can find one.
        wheel.writestr(
            f"{dist_info}/RECORD",
            "dcc_mcp_server-1.0.0.dist-info/METADATA,,\ndcc_mcp_server-1.0.0.dist-info/WHEEL,,\ndcc_mcp_server-1.0.0.data/scripts/dcc-mcp-server.exe,,\n",
        )


def test_inject_helper_places_it_beside_server_and_rewrites_record(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-cp311-none-win_amd64.whl"
    helper = tmp_path / "dcc-mcp-capture-helper.exe"
    helper.write_bytes(b"MZhelper")
    _write_server_wheel(wheel)

    inject_helper(wheel, helper)

    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        assert "dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-capture-helper.exe" in names
        assert archive.read("dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-capture-helper.exe") == b"MZhelper"
        record = archive.read("dcc_mcp_server-1.0.0.dist-info/RECORD").decode("utf-8")
        digest = base64.urlsafe_b64encode(hashlib.sha256(b"MZhelper").digest()).rstrip(b"=").decode("ascii")
        assert (f"dcc_mcp_server-1.0.0.data/scripts/dcc-mcp-capture-helper.exe,sha256={digest},8") in record


def test_inject_helper_rejects_wheel_without_server_script(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_server-1.0.0-cp311-none-win_amd64.whl"
    helper = tmp_path / "dcc-mcp-capture-helper.exe"
    helper.write_bytes(b"MZhelper")

    with zipfile.ZipFile(str(wheel), "w") as archive:
        archive.writestr("dcc_mcp_server-1.0.0.dist-info/METADATA", "Name: dcc-mcp-server\nVersion: 1.0.0\n")
        archive.writestr(
            "dcc_mcp_server-1.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp311-none-win_amd64\n",
        )
        archive.writestr("dcc_mcp_server-1.0.0.dist-info/RECORD", "dcc_mcp_server-1.0.0.dist-info/METADATA,,\n")

    try:
        inject_helper(wheel, helper)
    except ValueError as exc:
        assert "expected one bundled dcc-mcp-server.exe" in str(exc)
    else:  # pragma: no cover - assertion guard
        raise AssertionError("wheel without server script was accepted")
