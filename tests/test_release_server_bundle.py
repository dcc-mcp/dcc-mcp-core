"""Release server bundle packaging tests."""

from __future__ import annotations

import subprocess
import sys
import zipfile

from conftest import REPO_ROOT

SCRIPT = REPO_ROOT / "scripts" / "release" / "build_server_bundle.py"


def test_build_server_bundle_packages_unsuffixed_unix_binaries(tmp_path) -> None:
    server = tmp_path / "dcc-mcp-server-linux-x86_64"
    cli = tmp_path / "dcc-mcp-cli-linux-x86_64"
    server.write_bytes(b"server")
    cli.write_bytes(b"cli")

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
            "--cli-bin",
            str(cli),
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
        assert set(zf.namelist()) == {"dcc-mcp-server", "dcc-mcp-cli"}
        assert zf.read("dcc-mcp-server") == b"server"
        assert zf.read("dcc-mcp-cli") == b"cli"


def test_build_server_bundle_preserves_windows_exe_names(tmp_path) -> None:
    server = tmp_path / "dcc-mcp-server-windows-x86_64.exe"
    cli = tmp_path / "dcc-mcp-cli-windows-x86_64.exe"
    server.write_bytes(b"server")
    cli.write_bytes(b"cli")

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
            "--cli-bin",
            str(cli),
            "--out-dir",
            str(out_dir),
        ],
        check=True,
    )

    bundle = out_dir / "dcc-mcp-server-0.18.12-windows-x86_64.zip"
    with zipfile.ZipFile(bundle) as zf:
        assert set(zf.namelist()) == {"dcc-mcp-server.exe", "dcc-mcp-cli.exe"}
