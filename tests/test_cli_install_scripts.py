from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]
SH_INSTALLER = ROOT / "scripts" / "install-cli.sh"
PS_INSTALLER = ROOT / "scripts" / "install-cli.ps1"
OFFICIAL_RELEASE = "https://github.com/dcc-mcp/dcc-mcp-core/releases/download"
INSTALL_DOCS = (
    ROOT / "README.md",
    ROOT / "README_zh.md",
    ROOT / "AI_AGENT_GUIDE.md",
    ROOT / "docs" / "guide" / "getting-started.md",
    ROOT / "docs" / "zh" / "guide" / "getting-started.md",
    ROOT / "docs" / "guide" / "cli-reference.md",
    ROOT / "docs" / "zh" / "guide" / "cli-reference.md",
)


def _shell() -> str:
    executable = shutil.which("sh")
    if executable:
        return executable
    git_sh = Path(r"C:\Program Files\Git\bin\sh.exe")
    if not git_sh.is_file():
        pytest.skip("sh is unavailable")
    return str(git_sh)


def _shell_path(path: Path) -> str:
    value = path.resolve().as_posix()
    if os.name == "nt" and len(value) > 2 and value[1] == ":":
        return f"/{value[0].lower()}{value[2:]}"
    return value


def _write_fake_unix_tools(bin_dir: Path) -> None:
    bin_dir.mkdir()
    curl = bin_dir / "curl"
    with curl.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(
            """#!/bin/sh
url=""
out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        http*) url="$1"; shift ;;
        *) shift ;;
    esac
done
printf '%s\n' "$url" >> "$REQUEST_LOG"
case "$url" in
    *dcc-mcp-update-manifest-*) cp "$FIXTURE_MANIFEST" "$out" ;;
    *dcc-mcp-cli-*) cp "$FIXTURE_BINARY" "$out" ;;
    *) exit 22 ;;
esac
"""
        )
    uname = bin_dir / "uname"
    with uname.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(
            """#!/bin/sh
case "$1" in
    -s) echo Linux ;;
    -m) echo x86_64 ;;
    *) exit 1 ;;
esac
"""
        )
    curl.chmod(0o755)
    uname.chmod(0o755)


def _run_sh_installer(tmp_path: Path, manifest: dict, binary: bytes) -> subprocess.CompletedProcess[str]:
    fake_bin = tmp_path / "bin"
    _write_fake_unix_tools(fake_bin)
    manifest_path = tmp_path / "manifest.json"
    binary_path = tmp_path / "cli.bin"
    request_log = tmp_path / "requests.log"
    install_dir = tmp_path / "install"
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    binary_path.write_bytes(binary)
    env = os.environ.copy()
    env.update(
        {
            "DCC_MCP_VERSION": "latest",
            "DCC_MCP_INSTALL_DIR": _shell_path(install_dir),
            "FIXTURE_MANIFEST": _shell_path(manifest_path),
            "FIXTURE_BINARY": _shell_path(binary_path),
            "REQUEST_LOG": _shell_path(request_log),
        }
    )
    return subprocess.run(
        [
            _shell(),
            "-c",
            'PATH="$1:/usr/bin:/bin"; export PATH; exec "$2"',
            "installer-test",
            _shell_path(fake_bin),
            _shell_path(SH_INSTALLER),
        ],
        capture_output=True,
        env=env,
        text=True,
        timeout=30,
        check=False,
    )


def _powershell() -> str:
    executable = shutil.which("pwsh") or shutil.which("powershell")
    if not executable:
        pytest.skip("PowerShell is unavailable")
    return executable


def _run_ps_installer(tmp_path: Path, manifest: dict, binary: bytes) -> subprocess.CompletedProcess[str]:
    manifest_path = tmp_path / "manifest.json"
    binary_path = tmp_path / "cli.exe"
    request_log = tmp_path / "requests.log"
    install_dir = tmp_path / "install"
    wrapper = tmp_path / "invoke-installer.ps1"
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    binary_path.write_bytes(binary)
    wrapper.write_text(
        """$ErrorActionPreference = "Stop"
$originalUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
function Invoke-WebRequest {
    param([string] $Uri, [string] $OutFile)
    Add-Content -LiteralPath $env:REQUEST_LOG -Value $Uri
    if ($Uri -like "*dcc-mcp-update-manifest-*") {
        Copy-Item -LiteralPath $env:FIXTURE_MANIFEST -Destination $OutFile
    } elseif ($Uri -like "*dcc-mcp-cli-*") {
        Copy-Item -LiteralPath $env:FIXTURE_BINARY -Destination $OutFile
    } else {
        throw "Unexpected URL: $Uri"
    }
}
try {
    & $env:INSTALL_SCRIPT -Version latest -InstallDir $env:TEST_INSTALL_DIR
} finally {
    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentUserPath -cne $originalUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $originalUserPath, "User")
    }
}
""",
        encoding="utf-8",
    )
    env = os.environ.copy()
    env.update(
        {
            "INSTALL_SCRIPT": str(PS_INSTALLER),
            "TEST_INSTALL_DIR": str(install_dir),
            "FIXTURE_MANIFEST": str(manifest_path),
            "FIXTURE_BINARY": str(binary_path),
            "REQUEST_LOG": str(request_log),
        }
    )
    return subprocess.run(
        [_powershell(), "-NoLogo", "-NoProfile", "-File", str(wrapper)],
        capture_output=True,
        env=env,
        text=True,
        timeout=30,
        check=False,
    )


def _manifest(*, asset: str, binary: bytes, url=None, sha256=None) -> dict:
    return {
        "dcc-mcp-cli": {
            "version": "0.19.63",
            "url": url or f"{OFFICIAL_RELEASE}/v0.19.63/{asset}",
            "sha256": sha256 or hashlib.sha256(binary).hexdigest(),
        }
    }


def test_installers_do_not_recommend_remote_pipe_execution() -> None:
    forbidden = re.compile(
        r"(?:curl|irm|invoke-restmethod)[^\r\n|]*\|[^\r\n]*(?:sh|bash|iex|invoke-expression)|ExecutionPolicy\s+Bypass",
        re.IGNORECASE,
    )
    for path in (SH_INSTALLER, PS_INSTALLER):
        assert forbidden.search(path.read_text(encoding="utf-8")) is None, path


def test_install_docs_do_not_recommend_remote_pipe_or_policy_bypass() -> None:
    forbidden = re.compile(
        r"(?:curl|irm|invoke-restmethod)[^\r\n|]*\|[^\r\n]*(?:sh|bash|iex|invoke-expression)|ExecutionPolicy\s+Bypass",
        re.IGNORECASE,
    )
    for path in INSTALL_DOCS:
        assert forbidden.search(path.read_text(encoding="utf-8")) is None, path


def test_installers_use_fixed_official_update_manifests() -> None:
    sh_source = SH_INSTALLER.read_text(encoding="utf-8")
    ps_source = PS_INSTALLER.read_text(encoding="utf-8")
    for source in (sh_source, ps_source):
        assert "DCC_MCP_REPO" not in source
        assert "api.github.com" not in source
        assert "dcc-mcp/dcc-mcp-core/releases" in source
        assert "dcc-mcp-update-manifest-" in source
        assert "sha256" in source.lower()
    assert "DCC_MCP_RELEASE_FALLBACK" not in sh_source
    assert "[string] $Repo" not in ps_source


def test_sh_latest_uses_manifest_pinned_url_and_installs_verified_binary(tmp_path: Path) -> None:
    binary = b"verified linux cli"
    result = _run_sh_installer(
        tmp_path,
        _manifest(asset="dcc-mcp-cli-linux-x86_64", binary=binary),
        binary,
    )

    assert result.returncode == 0, result.stderr
    assert (tmp_path / "install" / "dcc-mcp-cli").read_bytes() == binary
    requests = (tmp_path / "requests.log").read_text(encoding="utf-8").splitlines()
    assert requests == [
        "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-linux-x86_64.json",
        f"{OFFICIAL_RELEASE}/v0.19.63/dcc-mcp-cli-linux-x86_64",
    ]


def test_sh_checksum_mismatch_preserves_existing_binary(tmp_path: Path) -> None:
    install_dir = tmp_path / "install"
    install_dir.mkdir()
    target = install_dir / "dcc-mcp-cli"
    target.write_bytes(b"known good")
    binary = b"tampered"
    result = _run_sh_installer(
        tmp_path,
        _manifest(asset="dcc-mcp-cli-linux-x86_64", binary=binary, sha256="0" * 64),
        binary,
    )

    assert result.returncode != 0
    assert "SHA-256" in result.stderr
    assert target.read_bytes() == b"known good"


def test_sh_rejects_non_official_asset_before_download(tmp_path: Path) -> None:
    binary = b"must not download"
    result = _run_sh_installer(
        tmp_path,
        _manifest(
            asset="dcc-mcp-cli-linux-x86_64",
            binary=binary,
            url="https://example.invalid/dcc-mcp-cli-linux-x86_64",
        ),
        binary,
    )

    assert result.returncode != 0
    assert "official" in result.stderr.lower()
    requests = (tmp_path / "requests.log").read_text(encoding="utf-8").splitlines()
    assert requests == [
        "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-linux-x86_64.json"
    ]
    assert not (tmp_path / "install" / "dcc-mcp-cli").exists()


def test_powershell_latest_uses_manifest_pinned_url_and_installs_verified_binary(
    tmp_path: Path,
) -> None:
    binary = b"verified windows cli"
    result = _run_ps_installer(
        tmp_path,
        _manifest(asset="dcc-mcp-cli-windows-x86_64.exe", binary=binary),
        binary,
    )

    assert result.returncode == 0, result.stderr
    assert (tmp_path / "install" / "dcc-mcp-cli.exe").read_bytes() == binary
    requests = (tmp_path / "requests.log").read_text(encoding="utf-8").splitlines()
    assert requests == [
        "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.json",
        f"{OFFICIAL_RELEASE}/v0.19.63/dcc-mcp-cli-windows-x86_64.exe",
    ]


def test_powershell_checksum_mismatch_preserves_existing_binary(tmp_path: Path) -> None:
    install_dir = tmp_path / "install"
    install_dir.mkdir()
    target = install_dir / "dcc-mcp-cli.exe"
    target.write_bytes(b"known good")
    binary = b"tampered"
    result = _run_ps_installer(
        tmp_path,
        _manifest(asset="dcc-mcp-cli-windows-x86_64.exe", binary=binary, sha256="0" * 64),
        binary,
    )

    assert result.returncode != 0
    assert "SHA-256" in result.stderr
    assert target.read_bytes() == b"known good"


def test_powershell_rejects_non_official_asset_before_download(tmp_path: Path) -> None:
    binary = b"must not download"
    result = _run_ps_installer(
        tmp_path,
        _manifest(
            asset="dcc-mcp-cli-windows-x86_64.exe",
            binary=binary,
            url="https://example.invalid/dcc-mcp-cli-windows-x86_64.exe",
        ),
        binary,
    )

    assert result.returncode != 0
    assert "official release" in result.stderr.lower()
    requests = (tmp_path / "requests.log").read_text(encoding="utf-8").splitlines()
    assert requests == [
        "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.json"
    ]
    assert not (tmp_path / "install" / "dcc-mcp-cli.exe").exists()
