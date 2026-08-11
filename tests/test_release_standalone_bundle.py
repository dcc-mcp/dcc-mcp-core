"""Release standalone binary bundle packaging tests."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "release" / "build_standalone_bundle.py"


def _build_bundle(tmp_path: Path, binary_name: str, source_name: str) -> zipfile.ZipFile:
    binary = tmp_path / source_name
    binary.write_bytes(binary_name.encode())
    platform = "windows-x86_64" if binary.suffix == ".exe" else "linux-x86_64"

    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--version",
            "0.19.64",
            "--platform",
            platform,
            "--binary-name",
            binary_name,
            "--binary-path",
            str(binary),
            "--out-dir",
            str(tmp_path),
        ],
        check=True,
    )

    return zipfile.ZipFile(tmp_path / f"{binary_name}-0.19.64-{platform}.zip")


def test_build_bundle_packages_unsuffixed_unix_binary(tmp_path: Path) -> None:
    with _build_bundle(tmp_path, "dcc-mcp-cli", "dcc-mcp-cli-linux-x86_64") as bundle:
        assert bundle.namelist() == ["dcc-mcp-cli"]
        assert bundle.read("dcc-mcp-cli") == b"dcc-mcp-cli"


def test_build_bundle_preserves_windows_exe_name(tmp_path: Path) -> None:
    with _build_bundle(tmp_path, "dcc-mcp-cli", "dcc-mcp-cli-windows-x86_64.exe") as bundle:
        assert bundle.namelist() == ["dcc-mcp-cli.exe"]
        assert bundle.read("dcc-mcp-cli.exe") == b"dcc-mcp-cli"
