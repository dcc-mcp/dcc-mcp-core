"""Final-revision response projection through the native Python HTTP binding."""

from __future__ import annotations

import json
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


@pytest.fixture(scope="module")
def modern_response_server():
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0))
    with server.start() as handle:
        yield handle.mcp_url()


def _request(url, method, params):
    params = dict(params)
    params["_meta"] = {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "response-contract", "version": "1"},
    }
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "MCP-Protocol-Version": "2026-07-28",
        "Mcp-Method": method,
    }
    if "name" in params:
        headers["Mcp-Name"] = params["name"]
    request = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": method, "method": method, "params": params}).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == 200
        assert response.headers.get("Mcp-Session-Id") is None
        body = json.loads(response.read())
    assert body["id"] == method
    return body


@pytest.mark.parametrize(
    ("method", "params", "cacheable"),
    [
        ("server/discover", {}, True),
        ("tools/list", {}, True),
        ("tools/call", {"name": "search_tools", "arguments": {"query": "blender scene"}}, False),
        ("tools/call", {"name": "search_tools", "arguments": {"query": "maya scene"}}, False),
        ("ping", {}, False),
    ],
)
def test_modern_success_response_fields(modern_response_server, method, params, cacheable):
    body = _request(modern_response_server, method, params)
    assert "error" not in body
    result = body["result"]
    assert result["resultType"] == "complete"
    identity = result["_meta"]["io.modelcontextprotocol/serverInfo"]
    assert isinstance(identity["name"], str)
    assert isinstance(identity["version"], str)
    if cacheable:
        assert result["ttlMs"] == 0
        assert result["cacheScope"] == "private"
    else:
        assert "ttlMs" not in result
        assert "cacheScope" not in result
    if method == "server/discover":
        assert result["supportedVersions"][0] == "2026-07-28"
        assert "protocolVersion" not in result
        assert "serverInfo" not in result
    elif method == "tools/call":
        assert result.get("isError") is not True
        assert isinstance(result["content"], list)


def test_json_rpc_errors_are_not_stamped_as_success(modern_response_server):
    body = _request(modern_response_server, "tools/call", {"arguments": {}})
    assert body["error"]["code"] == -32602
    assert "result" not in body
    assert "resultType" not in body
