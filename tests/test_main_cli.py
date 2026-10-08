"""Tests for the ``dcc-mcp-core`` package-level console entry point.

These cover the gap reported in PIP-4390: a resolved ``dcc_mcp_core`` package
must be able to say how its MCP server starts, and must ship something a Rez
``package.py`` can declare under ``tools``.
"""

from __future__ import annotations

import json
from typing import Any
from typing import Dict
from typing import List

import pytest

import dcc_mcp_core
from dcc_mcp_core import __main__ as core_cli


class _FakeHandle:
    """Stand-in for ``McpServerHandle`` / ``SidecarServerHandle``."""

    def __init__(self, url: str = "http://127.0.0.1:8765/mcp") -> None:
        self._url = url
        self.port = 8765
        self.is_gateway = False
        self.instance_id = "fake-instance"
        self.shutdown_calls = 0

    def mcp_url(self) -> str:
        return self._url

    def shutdown(self) -> None:
        self.shutdown_calls += 1


class _FakeServer:
    """Stand-in for the object returned by ``create_skill_server``."""

    def __init__(self, handle: _FakeHandle) -> None:
        self._handle = handle

    def start(self) -> _FakeHandle:
        return self._handle


class _Recorder:
    """Captures ``create_skill_server`` calls and the handle it returned."""

    def __init__(self) -> None:
        self.calls: List[Dict[str, Any]] = []
        self.handle = _FakeHandle()

    @property
    def call(self) -> Dict[str, Any]:
        return self.calls[-1]


@pytest.fixture
def recorder(monkeypatch: pytest.MonkeyPatch) -> _Recorder:
    """Replace the server factory so no real listener is started."""
    rec = _Recorder()

    def _fake_create_skill_server(app_name: str, config: Any = None, **kwargs: Any) -> _FakeServer:
        rec.calls.append({"app_name": app_name, "config": config, **kwargs})
        return _FakeServer(rec.handle)

    monkeypatch.setattr("dcc_mcp_core.server_base.create_skill_server", _fake_create_skill_server)
    monkeypatch.setattr(core_cli, "_SHUTDOWN_POLL_SECS", 0.01)
    return rec


def test_version_flag_reports_package_version(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as excinfo:
        core_cli.main(["--version"])

    assert excinfo.value.code == 0
    assert dcc_mcp_core.__version__ in capsys.readouterr().out


def test_info_prints_executable_python_and_rez_contract(capsys: pytest.CaptureFixture[str]) -> None:
    assert core_cli.main(["info"]) == 0

    out = capsys.readouterr().out
    assert "dcc-mcp-core serve --dcc <name>" in out
    assert "from dcc_mcp_core import McpHttpConfig, create_skill_server" in out
    assert 'tools = ["dcc-mcp-core"]' in out


def test_serve_starts_and_stops_the_server(recorder: _Recorder) -> None:
    assert core_cli.main(["serve", "--dcc", "maya", "--max-run-secs", "0"]) == 0
    assert recorder.handle.shutdown_calls == 1


def test_serve_forwards_host_port_and_skill_paths(recorder: _Recorder) -> None:
    assert (
        core_cli.main(
            [
                "serve",
                "--dcc",
                "blender",
                "--host",
                "0.0.0.0",
                "--port",
                "9001",
                "--skill-path",
                "/tmp/one",
                "--skill-path",
                "/tmp/two",
                "--max-run-secs",
                "0",
            ]
        )
        == 0
    )

    assert recorder.call["app_name"] == "blender"
    assert recorder.call["config"].host == "0.0.0.0"
    assert recorder.call["config"].port == 9001
    assert recorder.call["extra_paths"] == ["/tmp/one", "/tmp/two"]
    assert recorder.handle.shutdown_calls == 1


def test_serve_can_leave_gateway_election(recorder: _Recorder) -> None:
    assert core_cli.main(["serve", "--dcc", "maya", "--no-gateway", "--max-run-secs", "0"]) == 0

    assert recorder.call["config"].gateway_port == 0


def test_serve_emits_json_endpoint(recorder: _Recorder, capsys: pytest.CaptureFixture[str]) -> None:
    assert core_cli.main(["serve", "--dcc", "maya", "--json", "--max-run-secs", "0"]) == 0

    payload = json.loads(capsys.readouterr().out.splitlines()[0])
    assert payload == {
        "dcc": "maya",
        "mcp_url": "http://127.0.0.1:8765/mcp",
        "port": 8765,
        "gateway": False,
        "instance_id": "fake-instance",
    }


def test_serve_returns_nonzero_when_start_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    def _boom(app_name: str, config: Any = None, **kwargs: Any) -> _FakeServer:
        raise RuntimeError("native core unavailable")

    monkeypatch.setattr("dcc_mcp_core.server_base.create_skill_server", _boom)

    assert core_cli.main(["serve", "--dcc", "maya"]) == 1


def test_top_level_dir_exposes_the_launch_api() -> None:
    names = [name for name in dir(dcc_mcp_core) if not name.startswith("_")]

    assert "create_skill_server" in names
    assert "create_adapter_server" in names
    assert "DccServerBase" in names
    assert "McpHttpConfig" in names


def test_create_adapter_server_is_importable_from_the_package_root() -> None:
    from dcc_mcp_core import create_adapter_server
    from dcc_mcp_core import server

    assert callable(create_adapter_server)
    assert create_adapter_server is server.create_adapter_server
