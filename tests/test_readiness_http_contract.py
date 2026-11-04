"""Real isolated loopback HTTP coverage; this is not native DCC acceptance."""

from __future__ import annotations

import json
import urllib.error
import urllib.request

from conftest import McpClient
from dcc_mcp_core import AdapterReadinessBinder
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _request(base: str, path: str, body=None) -> tuple[int, dict]:
    request = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
    )
    try:
        response = urllib.request.urlopen(request, timeout=5)
    except urllib.error.HTTPError as exc:
        response = exc
    with response:
        return response.status, json.loads(response.read().decode("utf-8"))


def _mcp_call(client: McpClient, tool: str) -> dict:
    status, result = client.post(
        {"jsonrpc": "2.0", "id": tool, "method": "tools/call", "params": {"name": tool, "arguments": {}}}
    )
    assert status == 200
    return result


def test_shared_probe_ready_blocked_ready_and_recovery_boundary() -> None:
    state = {"ready": True, "raises": False}
    calls = []

    def dcc_ready() -> bool:
        if state["raises"]:
            raise RuntimeError("host probe unavailable")
        return state["ready"]

    registry = ToolRegistry()
    for tool in ("scene_ping", "recover_host"):
        registry.register(tool, description="Readiness contract test", dcc="python")
    server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="readiness-http-contract"))
    binder = AdapterReadinessBinder.bind_inline(server, dcc_ready_probe=dcc_ready)

    def scene_ping(_params):
        calls.append("scene_ping")
        return {"success": True}

    def recover_host(_params):
        calls.append("recover_host")
        state["ready"] = True
        binder.refresh_dcc_ready()
        return {"success": True}

    server.register_handler("scene_ping", scene_ping)
    server.register_handler("recover_host", recover_host)
    assert binder.published is True
    handle = server.start()
    base = handle.mcp_url().rsplit("/mcp", 1)[0]
    try:
        client = McpClient(handle.mcp_url())
        for ready in (True, False, True):
            state["ready"] = ready
            assert binder.refresh_dcc_ready() is ready
            status, report = _request(base, "/v1/readyz")
            assert status == (200 if ready else 503)
            assert report["dcc"] is ready
            assert report["dispatcher"] is True
            assert report["host_execution_bridge"] is True
            assert report["main_thread_executor"] is True
            count_before = len(calls)
            for tool in ("scene_ping", "recover_host"):
                rest_status, _ = _request(base, "/v1/call", {"tool_slug": tool, "arguments": {}})
                mcp = _mcp_call(client, tool)
                if ready:
                    assert rest_status == 200
                    assert mcp["result"]["isError"] is False
                else:
                    assert rest_status == 503
                    assert mcp["error"]["code"] == -32002
            # Even a recovery handler cannot execute behind the red gate.
            assert len(calls) == count_before + (4 if ready else 0)

        # A callback is sampled explicitly, not on every HTTP request.
        state["raises"] = True
        assert _request(base, "/v1/readyz")[0] == 200
        assert binder.refresh_dcc_ready() is False
        assert _request(base, "/v1/readyz")[0] == 503
        count_before = len(calls)
        assert _request(base, "/v1/call", {"tool_slug": "recover_host", "arguments": {}})[0] == 503
        assert _mcp_call(client, "recover_host")["error"]["code"] == -32002
        assert len(calls) == count_before
        state["raises"] = False
        assert binder.refresh_dcc_ready() is True
        assert _request(base, "/v1/readyz")[0] == 200

        # Execution bits describe main-thread capability, not base admission.
        binder.mark_execution_ready(False)
        status, report = _request(base, "/v1/readyz")
        assert status == 200
        assert report["host_execution_bridge"] is False
        assert report["main_thread_executor"] is False
        assert _request(base, "/v1/call", {"tool_slug": "scene_ping", "arguments": {}})[0] == 200
        assert _mcp_call(client, "scene_ping")["result"]["isError"] is False

        binder.mark_dispatcher_ready(False)
        assert _request(base, "/v1/readyz")[0] == 503
        assert _request(base, "/v1/call", {"tool_slug": "scene_ping", "arguments": {}})[0] == 503
        assert _mcp_call(client, "scene_ping")["error"]["code"] == -32002
    finally:
        handle.shutdown()
