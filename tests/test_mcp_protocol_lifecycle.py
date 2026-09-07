"""Keep legacy initialize and explicit stateless discovery on separate lifecycles."""

from __future__ import annotations

import json
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


@pytest.fixture(scope="module")
def protocol_server():
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0))
    with server.start() as handle:
        yield handle.mcp_url()


def _request(url, method, params, protocol_header=None):
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    if protocol_header is not None:
        headers["MCP-Protocol-Version"] = protocol_header
    request = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": "lifecycle-check", "method": method, "params": params}).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == 200
        body = json.loads(response.read())
    assert body["id"] == "lifecycle-check"
    return body


@pytest.mark.parametrize(
    ("requested", "expected"),
    [
        ("2025-03-26", "2025-03-26"),
        ("2025-06-18", "2025-06-18"),
        ("2026-07-28", "2025-06-18"),
        ("2099-01-01", "2025-06-18"),
        (None, "2025-06-18"),
    ],
)
def test_headerless_initialize_stays_on_legacy_lifecycle(protocol_server, requested, expected):
    params = {"capabilities": {}, "clientInfo": {"name": "protocol-contract", "version": "1"}}
    if requested is not None:
        params["protocolVersion"] = requested
    response = _request(protocol_server, "initialize", params)
    assert response["result"]["protocolVersion"] == expected


def test_explicit_stateless_header_uses_discover_not_initialize(protocol_server):
    response = _request(protocol_server, "server/discover", {}, "2026-07-28")
    assert response["result"]["protocolVersion"] == "2026-07-28"
    response = _request(
        protocol_server,
        "initialize",
        {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": {"name": "protocol-contract", "version": "1"},
        },
        "2026-07-28",
    )
    assert response["error"]["code"] == -32601
    assert "result" not in response
