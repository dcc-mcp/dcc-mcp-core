"""Release update-manifest component contracts."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "release" / "generate_update_manifest.py"


def _run(tmp_path: Path, platform: str, *, with_host: bool) -> subprocess.CompletedProcess[str]:
    server = tmp_path / ("dcc-mcp-server.exe" if platform.startswith("windows-") else "dcc-mcp-server")
    cli = tmp_path / ("dcc-mcp-cli.exe" if platform.startswith("windows-") else "dcc-mcp-cli")
    server.write_bytes(b"server")
    cli.write_bytes(b"cli")
    command = [
        sys.executable,
        str(SCRIPT),
        "--version",
        "1.2.3",
        "--platform",
        platform,
        "--release-tag",
        "v1.2.3",
        "--repo",
        "dcc-mcp/dcc-mcp-core",
        "--server-bin",
        str(server),
        "--cli-bin",
        str(cli),
        "--out-dir",
        str(tmp_path / "dist"),
    ]
    if with_host:
        host = tmp_path / "dcc-mcp-ui-control-host-windows-x86_64.exe"
        host.write_bytes(b"MZhost")
        command.extend(["--ui-control-host", str(host)])
    return subprocess.run(command, capture_output=True, text=True, check=False)


def test_windows_manifest_publishes_version_matched_host_with_hash(tmp_path: Path) -> None:
    result = _run(tmp_path, "windows-x86_64", with_host=True)
    assert result.returncode == 0, result.stderr
    manifest = json.loads((tmp_path / "dist" / "dcc-mcp-update-manifest-windows-x86_64.json").read_text())
    assert set(manifest) == {"dcc-mcp-server", "dcc-mcp-cli", "dcc-mcp-ui-control-host"}
    host = manifest["dcc-mcp-ui-control-host"]
    assert host["version"] == "1.2.3"
    assert host["url"].endswith("/dcc-mcp-ui-control-host-windows-x86_64.exe")
    assert len(host["sha256"]) == 64


def test_windows_manifest_fails_closed_without_host(tmp_path: Path) -> None:
    result = _run(tmp_path, "windows-x86_64", with_host=False)
    assert result.returncode == 1
    assert "require --ui-control-host" in result.stderr


def test_non_windows_manifest_never_carries_windows_host(tmp_path: Path) -> None:
    result = _run(tmp_path, "linux-x86_64", with_host=False)
    assert result.returncode == 0, result.stderr
    manifest = json.loads((tmp_path / "dist" / "dcc-mcp-update-manifest-linux-x86_64.json").read_text())
    assert set(manifest) == {"dcc-mcp-server", "dcc-mcp-cli"}

    rejected = _run(tmp_path, "linux-x86_64", with_host=True)
    assert rejected.returncode == 1
    assert "must not include --ui-control-host" in rejected.stderr
