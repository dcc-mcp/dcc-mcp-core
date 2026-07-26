"""Release CLI bundle packaging tests."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "release" / "build_cli_bundle.py"


def _build_bundle(tmp_path: Path, binary_name: str) -> zipfile.ZipFile:
    cli = tmp_path / binary_name
    cli.write_bytes(b"cli")

    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--version",
            "0.19.64",
            "--platform",
            "windows-x86_64" if cli.suffix == ".exe" else "linux-x86_64",
            "--cli-bin",
            str(cli),
            "--out-dir",
            str(tmp_path),
        ],
        check=True,
    )

    return zipfile.ZipFile(next(tmp_path.glob("dcc-mcp-cli-*.zip")))


def test_build_cli_bundle_packages_unsuffixed_unix_binary(tmp_path: Path) -> None:
    with _build_bundle(tmp_path, "dcc-mcp-cli-linux-x86_64") as bundle:
        assert bundle.namelist() == ["dcc-mcp-cli"]
        assert bundle.read("dcc-mcp-cli") == b"cli"


def test_build_cli_bundle_preserves_windows_exe_name(tmp_path: Path) -> None:
    with _build_bundle(tmp_path, "dcc-mcp-cli-windows-x86_64.exe") as bundle:
        assert bundle.namelist() == ["dcc-mcp-cli.exe"]
        assert bundle.read("dcc-mcp-cli.exe") == b"cli"
