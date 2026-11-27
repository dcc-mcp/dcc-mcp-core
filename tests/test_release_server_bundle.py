"""Release server bundle packaging tests."""

from __future__ import annotations

import subprocess
import sys
import zipfile

from scripts.release import build_server_bundle

from conftest import REPO_ROOT

SCRIPT = REPO_ROOT / "scripts" / "release" / "build_server_bundle.py"


def _expected_notice_members() -> set[str]:
    members = {"LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt"}
    if (REPO_ROOT / "THIRD_PARTY_LICENSES.txt").is_file():
        members.add("THIRD_PARTY_LICENSES.txt")
    return members


def test_build_server_bundle_packages_unsuffixed_unix_binaries(tmp_path) -> None:
    server = tmp_path / "dcc-mcp-server-linux-x86_64"
    server.write_bytes(b"server")

    out_dir = tmp_path / "dist"
    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--version",
            "0.18.12",
            "--platform",
            "linux-x86_64",
            "--server-bin",
            str(server),
            "--out-dir",
            str(out_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    bundle = out_dir / "dcc-mcp-server-0.18.12-linux-x86_64.zip"
    assert result.stdout.strip().endswith(bundle.as_posix())
    with zipfile.ZipFile(bundle) as zf:
        assert set(zf.namelist()) == _expected_notice_members() | {"dcc-mcp-server"}
        assert zf.read("dcc-mcp-server") == b"server"


def test_build_server_bundle_preserves_windows_exe_names(tmp_path) -> None:
    server = tmp_path / "dcc-mcp-server-windows-x86_64.exe"
    server.write_bytes(b"server")

    out_dir = tmp_path / "dist"
    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--version",
            "0.18.12",
            "--platform",
            "windows-x86_64",
            "--server-bin",
            str(server),
            "--out-dir",
            str(out_dir),
        ],
        check=True,
    )

    bundle = out_dir / "dcc-mcp-server-0.18.12-windows-x86_64.zip"
    with zipfile.ZipFile(bundle) as zf:
        assert set(zf.namelist()) == _expected_notice_members() | {"dcc-mcp-server.exe"}


def test_build_server_bundle_contains_only_server_and_notices(tmp_path) -> None:
    server = tmp_path / "dcc-mcp-server-windows-x86_64.exe"
    server.write_bytes(b"server")

    bundle = build_server_bundle.build_bundle(
        version="0.18.12",
        platform="windows-x86_64",
        server_bin=server,
        out_dir=tmp_path / "dist",
    )

    with zipfile.ZipFile(bundle) as archive:
        assert set(archive.namelist()) == _expected_notice_members() | {"dcc-mcp-server.exe"}
