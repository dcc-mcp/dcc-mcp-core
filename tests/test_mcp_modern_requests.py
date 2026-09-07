"""Final request ingress through the packaged Python native HTTP binding."""

from __future__ import annotations

import json
from pathlib import Path
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

_FIXTURE = Path(__file__).resolve().parents[1] / "crates/dcc-mcp-jsonrpc/tests/fixtures/modern_request_cases.json"
_CASES = [case for case in json.loads(_FIXTURE.read_text(encoding="utf-8")) if case.get("route") != "legacy"]


@pytest.fixture(scope="module")
def modern_request_server():
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0))
    with server.start() as handle:
        yield handle.mcp_url()


def _post(url, body, headers):
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream", **headers},
        method="POST",
    )
    try:
        response = urllib.request.urlopen(request, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        return response.status, response.headers, json.loads(raw) if raw else None


@pytest.mark.parametrize("case", _CASES, ids=[case["name"] for case in _CASES])
def test_native_request_boundary_matches_final_fixture(modern_request_server, case):
    status, headers, body = _post(modern_request_server, case["body"], case["headers"])
    assert status == case.get("status", 400)
    assert headers.get("Mcp-Session-Id") is None
    if status == 202:
        assert body is None
        return
    assert body["error"]["code"] == case["code"]
    assert "result" not in body
    assert body.get("id") == (None if case["code"] == -32600 else case["body"]["id"])
    if case["code"] == -32022:
        assert body["error"]["data"] == {"supported": ["2026-07-28"], "requested": "2099-01-01"}


@pytest.mark.parametrize("tool, is_error", [("search_tools", False), ("missing-fixture-tool", True)])
def test_optional_client_identity_and_tool_errors_remain_in_band(modern_request_server, tool, is_error):
    body = {
        "jsonrpc": "2.0",
        "id": "call",
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": {"query": "scene"},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        },
    }
    status, headers, response = _post(
        modern_request_server,
        body,
        {
            "MCP-Protocol-Version": "2026-07-28",
            "Mcp-Method": "tools/call",
            "Mcp-Name": tool,
        },
    )
    assert status == 200
    assert headers.get("Mcp-Session-Id") is None
    assert response["result"]["resultType"] == "complete"
    assert bool(response["result"].get("isError")) is is_error
