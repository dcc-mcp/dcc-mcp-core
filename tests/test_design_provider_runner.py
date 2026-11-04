"""Host-free CLI outcome contracts; these fixtures establish no native acceptance."""

from __future__ import annotations

from contextlib import asynccontextmanager
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import sys
from types import ModuleType

import pytest

_RUNNER = Path(__file__).resolve().parents[1] / "examples" / "design-provider-bridge" / "runner.py"
_TOOL = "synthetic_operation"


class HostFreeSession:
    """Observe one injected session without opening an SDK transport or host."""

    def __init__(self, result):
        self.result = result
        self.calls = []
        self.closed = False

    async def list_tools(self, cursor=None):
        assert cursor is None
        return {"tools": [{"name": _TOOL, "inputSchema": {"type": "object"}}]}

    async def call_tool(self, name, arguments=None):
        self.calls.append((name, deepcopy(arguments)))
        return deepcopy(self.result)

    async def close(self):
        self.closed = True


@pytest.fixture
def runner():
    spec = importlib.util.spec_from_file_location("design_provider_runner_contract", _RUNNER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def bind_session(runner, monkeypatch, provider, result, *arguments):
    """Keep the real bridge while replacing only its caller-owned transport."""
    session = HostFreeSession(result)
    bridges = []
    lifecycle = []
    bridge_type = runner.DesignSessionBridge

    def create_bridge(selected_provider):
        bridge = bridge_type(selected_provider)
        disconnect = bridge.disconnect

        def observed_disconnect():
            lifecycle.append("disconnect")
            return disconnect()

        bridge.disconnect = observed_disconnect
        bridges.append(bridge)
        return bridge

    @asynccontextmanager
    async def upstream_session(args):
        assert args.provider == provider
        try:
            yield session
        finally:
            assert bridges[0].status()["state"] == "disconnected"
            await session.close()
            lifecycle.append("session_closed")

    monkeypatch.setattr(runner, "DesignSessionBridge", create_bridge)
    monkeypatch.setattr(runner, "upstream_session", upstream_session)
    monkeypatch.setattr(sys, "argv", [str(_RUNNER), "--provider", provider, *arguments])
    return session, bridges, lifecycle


@pytest.mark.parametrize("provider", ["figma", "penpot"])
@pytest.mark.parametrize(
    ("upstream_result", "exit_code", "tool_error"),
    [
        ({"content": [{"type": "text", "text": "Synthetic rejected operation."}], "isError": True}, 1, True),
        ({"content": [], "structuredContent": {"synthetic_value": 1}, "isError": False}, 0, False),
        ({"content": [{"type": "text", "text": "Synthetic unverified response."}], "isError": False}, 0, None),
        ({"content": [{"type": "text", "text": "Synthetic response without an error flag."}]}, 0, None),
    ],
    ids=["explicit-tool-error", "structured-result", "text-only-unverified", "missing-error-flag"],
)
def test_main_reports_tool_error_in_exit_status_once(
    runner, monkeypatch, capsys, provider, upstream_result, exit_code, tool_error
):
    session, bridges, lifecycle = bind_session(runner, monkeypatch, provider, upstream_result, "--call", _TOOL)

    with pytest.raises(SystemExit) as stopped:
        runner.main()

    assert stopped.value.code == exit_code
    captured = capsys.readouterr()
    assert captured.err == ""
    envelope = json.loads(captured.out)
    assert envelope["success"] is (tool_error is not True)
    context = envelope["context"]
    assert context["provider"] == provider
    assert context["tool_error"] is tool_error
    assert context["transport_success"] is True
    assert context["upstream_result"] == upstream_result
    assert context["native_acceptance"] == "unverified"
    assert context["outcome"] == ("upstream_tool_error" if tool_error is True else "unverified")
    assert session.calls == [(_TOOL, {})]
    assert session.closed is True
    assert bridges[0].status()["state"] == "disconnected"
    assert lifecycle == ["disconnect", "session_closed"]


@pytest.mark.parametrize("provider", ["figma", "penpot"])
def test_main_discovery_exits_zero_without_invoking_a_tool(runner, monkeypatch, capsys, provider):
    session, bridges, lifecycle = bind_session(runner, monkeypatch, provider, None)

    with pytest.raises(SystemExit) as stopped:
        runner.main()

    assert stopped.value.code == 0
    captured = capsys.readouterr()
    assert captured.err == ""
    catalogue = json.loads(captured.out)
    assert catalogue["provider"] == provider
    assert catalogue["tools"] == [{"name": _TOOL, "inputSchema": {"type": "object"}}]
    assert catalogue["native_acceptance"] == "unverified"
    assert session.calls == []
    assert session.closed is True
    assert bridges[0].status()["state"] == "disconnected"
    assert lifecycle == ["disconnect", "session_closed"]


@pytest.mark.parametrize("provider", ["figma", "penpot"])
def test_main_serve_exits_zero_after_owned_session_cleanup(runner, monkeypatch, capsys, provider):
    session, bridges, lifecycle = bind_session(runner, monkeypatch, provider, None, "--serve")
    facade = ModuleType("facade")

    async def serve_bridge(bridge, *, port, registry_dir):
        assert bridge is bridges[0]
        assert bridge.status()["state"] == "ready"
        assert port == 0
        assert registry_dir is None
        lifecycle.append("serve_returned")

    facade.serve_bridge = serve_bridge
    monkeypatch.setitem(sys.modules, "facade", facade)

    with pytest.raises(SystemExit) as stopped:
        runner.main()

    assert stopped.value.code == 0
    captured = capsys.readouterr()
    assert captured.out == captured.err == ""
    assert session.calls == []
    assert session.closed is True
    assert bridges[0].status()["state"] == "disconnected"
    assert lifecycle == ["serve_returned", "disconnect", "session_closed"]
