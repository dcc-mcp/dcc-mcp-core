from __future__ import annotations

import os
from pathlib import Path
import sys
import time
from typing import List

import pytest

from dcc_mcp_core.cua_cli import CuaCliBridge
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.cua_cli import _read_shared_image
from dcc_mcp_core.cua_cli import inspect_cua_contract
from dcc_mcp_core.cua_cli import resolve_cua_command

_FAKE_CLI = r"""from __future__ import print_function
import json
import os
from pathlib import Path
import sys

command = sys.argv[1]
if command == "manifest":
    print(json.dumps({
        "schema_version": 1,
        "name": "dcc-cua",
        "version": "0.4.0",
        "host": {
            "protocol_version": 1,
            "ensure_command": ["host-ensure"],
            "snapshot_transports": ["binary_frame", "shared_memory"],
            "capabilities": ["parallel_discovery_requests"],
        },
        "core_bridge": {
            "command": ["host-jsonl"],
        },
        "runtime": {
            "backend": "cua-driver-sdk",
            "separate_driver_required": False,
        },
    }))
elif command == "host-ensure":
    print(json.dumps({
        "type": "host_ready",
        "status": "existing",
        "protocol_version": 1,
        "host_version": "0.4.0",
    }))
elif command == "host-jsonl":
    output_dir = None
    if "--output-dir" in sys.argv:
        output_dir = Path(sys.argv[sys.argv.index("--output-dir") + 1])
    try:
        for line in sys.stdin:
            request = json.loads(line)
            if request["method"] == "fail":
                response = {
                    "type": "error",
                    "code": "denied",
                    "message": "blocked by fake host",
                    "request_id": request["request_id"],
                }
            elif request["method"] == "image":
                data = b"png"
                output = output_dir / "response-0.bin"
                output.write_bytes(data)
                response = {
                    "type": "snapshot",
                    "request_id": request["request_id"],
                    "image": {"encoding": "binary_frame", "length": len(data)},
                    "_dcc_cua_binary_output": str(output),
                }
            else:
                response = {
                    "type": "pong",
                    "request_id": request["request_id"],
                    "echo": request["params"],
                }
            print(json.dumps(response), flush=True)
    finally:
        marker = os.environ.get("FAKE_CUA_CLOSED")
        if marker:
            Path(marker).write_text("closed", encoding="utf-8")
else:
    raise SystemExit(2)
"""


def _fake_command(tmp_path: Path) -> List[str]:
    script = tmp_path / "fake_cua.py"
    script.write_text(_FAKE_CLI, encoding="utf-8")
    return [sys.executable, str(script)]


def test_manifest_drives_the_core_bridge_commands(tmp_path: Path) -> None:
    contract = inspect_cua_contract(_fake_command(tmp_path))
    assert contract.protocol_version == 1
    assert contract.ensure_command == ("host-ensure",)
    assert contract.bridge_command == ("host-jsonl",)
    assert contract.snapshot_transports == ("binary_frame", "shared_memory")
    assert contract.capabilities == ("parallel_discovery_requests",)
    assert contract.parallel_discovery is True


def test_manifest_rejects_a_separate_driver_dependency(tmp_path: Path) -> None:
    script = tmp_path / "driver_dependent_cua.py"
    script.write_text(
        _FAKE_CLI.replace('"separate_driver_required": False', '"separate_driver_required": True'),
        encoding="utf-8",
    )
    with pytest.raises(CuaCliError, match="must embed its CUA runtime"):
        inspect_cua_contract([sys.executable, str(script)])


def test_manifest_requires_current_stable_cua_runtime(tmp_path: Path) -> None:
    script = tmp_path / "old_cua.py"
    script.write_text(_FAKE_CLI.replace('"version": "0.4.0"', '"version": "0.3.1"'), encoding="utf-8")
    with pytest.raises(CuaCliError, match=r"dcc-cua 0[.]4[.]0 or newer is required"):
        inspect_cua_contract([sys.executable, str(script)])


def test_persistent_jsonl_bridge_correlates_errors_and_closes_on_eof(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    marker = tmp_path / "closed.txt"
    monkeypatch.setenv("FAKE_CUA_CLOSED", str(marker))
    bridge = CuaCliBridge(_fake_command(tmp_path))
    try:
        response = bridge.call("ping", {"agent": "maya"}, request_id="task-42")
        assert response == {"type": "pong", "request_id": "task-42", "echo": {"agent": "maya"}}
        with pytest.raises(CuaCliError, match="blocked by fake host") as failure:
            bridge.call("fail")
        assert failure.value.code == "denied"
    finally:
        bridge.close()
    assert marker.read_text(encoding="utf-8") == "closed"


def test_configured_binary_must_be_absolute(tmp_path: Path) -> None:
    with pytest.raises(CuaCliError, match="absolute path"):
        resolve_cua_command("relative/dcc-cua")
    missing = tmp_path / "missing-dcc-cua"
    with pytest.raises(CuaCliError, match="does not name a file"):
        resolve_cua_command(str(missing.resolve()))


def test_cli_sibling_precedes_an_unrelated_path_cua(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    cli = tmp_path / ("dcc-mcp-cli.exe" if os.name == "nt" else "dcc-mcp-cli")
    sibling = tmp_path / ("dcc-cua.exe" if os.name == "nt" else "dcc-cua")
    other = tmp_path / "other" / ("dcc-cua.exe" if os.name == "nt" else "dcc-cua")
    other.parent.mkdir()
    for path in (cli, sibling, other):
        path.write_bytes(b"test")

    def fake_which(name: str) -> str | None:
        return str(cli) if name == "dcc-mcp-cli" else str(other)

    monkeypatch.setattr("dcc_mcp_core.cua_cli.shutil.which", fake_which)
    assert resolve_cua_command() == [str(sibling.resolve())]


def test_official_install_dir_is_probed_without_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    binary = tmp_path / ("dcc-cua.exe" if os.name == "nt" else "dcc-cua")
    binary.write_bytes(b"test")
    monkeypatch.delenv("DCC_MCP_CUA_BINARY", raising=False)
    monkeypatch.setenv("DCC_MCP_INSTALL_DIR", str(tmp_path))
    monkeypatch.setattr("dcc_mcp_core.cua_cli.shutil.which", lambda _name: None)

    assert resolve_cua_command() == [str(binary.resolve())]


@pytest.mark.skipif(os.name != "nt", reason="Windows installer default")
def test_windows_default_dcc_mcp_bin_is_probed_without_path(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    binary = tmp_path / "dcc-mcp" / "bin" / "dcc-cua.exe"
    binary.parent.mkdir(parents=True)
    binary.write_bytes(b"test")
    monkeypatch.delenv("DCC_MCP_CUA_BINARY", raising=False)
    monkeypatch.delenv("DCC_MCP_INSTALL_DIR", raising=False)
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))
    monkeypatch.setattr("dcc_mcp_core.cua_cli.shutil.which", lambda _name: None)

    assert resolve_cua_command() == [str(binary.resolve())]


def test_latest_stable_versioned_install_is_probed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    executable_name = "dcc-cua.exe" if os.name == "nt" else "dcc-cua"
    versions = tmp_path / "versions"
    for version in ("1.0.2", "1.1.0", "1.2.0-beta.1", "not-a-version"):
        path = versions / version / executable_name
        path.parent.mkdir(parents=True)
        path.write_bytes(b"test")
    monkeypatch.delenv("DCC_MCP_CUA_BINARY", raising=False)
    monkeypatch.delenv("DCC_MCP_INSTALL_DIR", raising=False)
    monkeypatch.setattr("dcc_mcp_core.cua_cli._standard_cua_candidates", lambda: ())
    monkeypatch.setattr("dcc_mcp_core.cua_cli._standalone_cua_versions_dir", lambda: versions)
    monkeypatch.setattr("dcc_mcp_core.cua_cli.shutil.which", lambda _name: None)

    assert resolve_cua_command() == [str((versions / "1.1.0" / executable_name).resolve())]


def test_binary_frame_fallback_reads_and_removes_cli_output(tmp_path: Path) -> None:
    bridge = CuaCliBridge(_fake_command(tmp_path), snapshot_transport="binary_frame")
    try:
        response = bridge.call("image")
        output = Path(response["_dcc_cua_binary_output"])
        assert bridge.read_image(response) == b"png"
        assert not output.exists()
    finally:
        bridge.close()


def test_binary_frame_fallback_accepts_the_legacy_core_output_field(tmp_path: Path) -> None:
    script = tmp_path / "fake_legacy_cua.py"
    script.write_text(
        _FAKE_CLI.replace('"_dcc_cua_binary_output"', '"_dcc_mcp_binary_output"'),
        encoding="utf-8",
    )
    bridge = CuaCliBridge([sys.executable, str(script)], snapshot_transport="binary_frame")
    try:
        response = bridge.call("image")
        output = Path(response["_dcc_mcp_binary_output"])
        assert bridge.read_image(response) == b"png"
        assert not output.exists()
    finally:
        bridge.close()


@pytest.mark.skipif(sys.version_info < (3, 8), reason="stdlib shared-memory fixture requires Python 3.8+")
def test_cua_shared_image_reader_opens_the_cross_platform_named_mapping() -> None:
    from multiprocessing import shared_memory

    image_id = os.urandom(8).hex()
    pixels = b"png"
    now = int(time.time())
    fields = (0x4355_4100_5348_4D01, len(pixels), len(pixels), now, 60, 0)
    memory = bytearray().join(value.to_bytes(8, byteorder=sys.byteorder) for value in fields) + pixels
    mapping = shared_memory.SharedMemory(name=f"cua_{image_id}", create=True, size=len(memory))
    try:
        mapping.buf[: len(memory)] = memory
        assert _read_shared_image({"name": mapping.name, "id": image_id, "length": len(pixels)}) == pixels
    finally:
        mapping.close()
        mapping.unlink()


@pytest.mark.skipif(not os.environ.get("DCC_MCP_CUA_E2E_BINARY"), reason="standalone CUA binary not configured")
def test_real_standalone_cua_process_boundary() -> None:
    binary = str(Path(os.environ["DCC_MCP_CUA_E2E_BINARY"]).resolve())
    with CuaCliBridge([binary], shared_host=False) as bridge:
        assert bridge.contract.version == "0.4.0"
        assert "session_health" in bridge.contract.capabilities
        assert "session_input_state_events" in bridge.contract.capabilities
        assert "session_target_state_events" in bridge.contract.capabilities
        response = bridge.call("ping", timeout=10.0)
    assert response["type"] == "pong"
    assert response["protocol_version"] == 1
