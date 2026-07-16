"""Regression coverage for #1133: app_ui through gateway search/describe/call."""

from __future__ import annotations

import contextlib
import json
from pathlib import Path
import socket
import sys
import time
from typing import Any
import urllib.error
import urllib.request

import pytest

from conftest import McpClient
from conftest import allocate_gateway_port
from conftest import wait_tcp_reachable
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor

REPO_ROOT = Path(__file__).resolve().parents[1]
BUNDLED_SKILLS_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills"


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    return McpClient(url).post(body)[1]


def _tool_text(resp: dict) -> str:
    content = resp["result"]["content"]
    return "".join(item.get("text", "") for item in content if item.get("type") == "text")


def _post_json(url: str, body: dict) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={"Accept": "application/json", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=10.0) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        payload = exc.read().decode("utf-8", errors="replace")
        raise AssertionError(f"POST {url} failed with HTTP {exc.code}: {payload}") from exc


@pytest.fixture()
def app_ui_gateway(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()
    state_dir = tmp_path / "app-ui-state"
    monkeypatch.setenv("DCC_MCP_APP_UI_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_APP_UI_STATE_DIR", str(state_dir))
    monkeypatch.setenv("DCC_MCP_PYTHON_EXECUTABLE", sys.executable)
    monkeypatch.setenv("DCC_MCP_PYTHON_SKILL_PATHS", str(BUNDLED_SKILLS_DIR))

    gateway_port = allocate_gateway_port()
    cfg = McpHttpConfig(port=0, server_name="app-ui-backend")
    cfg.gateway_port = gateway_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "python"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10
    server = create_skill_server("python", cfg)
    server.set_in_process_executor(build_inprocess_executor(None))
    handle = server.start()

    assert wait_tcp_reachable("127.0.0.1", handle.port), f"backend port {handle.port} unreachable"
    if handle.is_gateway:
        assert wait_tcp_reachable("127.0.0.1", gateway_port), f"gateway port {gateway_port} unreachable"

    try:
        yield {
            "gateway_mcp_url": f"http://127.0.0.1:{gateway_port}/mcp",
            "gateway_rest_url": f"http://127.0.0.1:{gateway_port}",
            "handle": handle,
        }
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


def _load_app_ui(gateway_mcp_url: str) -> None:
    resp = _post_mcp(
        gateway_mcp_url,
        "tools/call",
        {"name": "load_skill", "arguments": {"skill_name": "app-ui"}},
    )
    assert "error" not in resp, resp.get("error")
    body = json.loads(_tool_text(resp))
    assert body["loaded"] is True, body


def _find_app_ui_slug(gateway_rest_url: str, query: str = "app_ui snapshot") -> str:
    deadline = time.time() + 8.0
    last = None
    while time.time() < deadline:
        body = _post_json(
            f"{gateway_rest_url}/v1/search",
            {"query": query, "dcc_type": "python", "limit": 10, "response_format": "json"},
        )
        last = body
        for hit in body.get("hits", []):
            if hit.get("backend_tool") == "app_ui__snapshot":
                return str(hit["tool_slug"])
        time.sleep(0.25)
    raise AssertionError(f"app_ui__snapshot not found in gateway search: {last!r}")


def test_app_ui_gateway_rest_and_mcp_discovery_describe_call(app_ui_gateway: dict) -> None:
    gateway_mcp_url = app_ui_gateway["gateway_mcp_url"]
    gateway_rest_url = app_ui_gateway["gateway_rest_url"]
    _load_app_ui(gateway_mcp_url)

    search_resp = _post_mcp(
        gateway_mcp_url,
        "tools/call",
        {"name": "search_tools", "arguments": {"query": "app_ui snapshot", "dcc_type": "python"}},
    )
    assert "error" not in search_resp, search_resp.get("error")
    assert "app_ui__snapshot" in _tool_text(search_resp)

    slug = _find_app_ui_slug(gateway_rest_url)
    describe = _post_json(f"{gateway_rest_url}/v1/describe", {"tool_slug": slug, "response_format": "json"})
    assert describe["tool"]["annotations"]["readOnlyHint"] is False
    assert describe["tool"]["_meta"]["dcc"]["affinity"] == "any"
    assert describe["tool"]["_meta"]["dcc"]["execution"] == "sync"
    assert "timeoutHintSecs" not in describe["tool"]["_meta"]["dcc"]
    assert describe["tool"]["_meta"]["dcc"]["risk"] == "mutation"

    rest_call = _post_json(
        f"{gateway_rest_url}/v1/call",
        {"tool_slug": slug, "arguments": {"session_id": "core-1133-rest"}, "response_format": "json"},
    )
    assert rest_call["output"]["success"] is True
    assert rest_call["output"]["context"]["snapshot"]["root"]["role"] == "window"

    mcp_call = _post_mcp(
        gateway_mcp_url,
        "tools/call",
        {
            "name": "call_tool",
            "arguments": {"tool_slug": slug, "arguments": {"session_id": "core-1133-mcp"}},
        },
    )
    assert "error" not in mcp_call, mcp_call.get("error")
    payload = json.loads(_tool_text(mcp_call))
    assert payload["output"]["success"] is True
    assert payload["output"]["context"]["snapshot"]["root"]["role"] == "window"
