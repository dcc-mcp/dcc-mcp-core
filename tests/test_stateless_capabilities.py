"""Advertise only executable capabilities on the staged stateless endpoint."""

from __future__ import annotations

import json
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _request(url, method, params, modern=False):
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    if modern:
        headers.update({"MCP-Protocol-Version": "2026-07-28", "Mcp-Method": method})
        params = dict(params)
        params["_meta"] = {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name": "capability-contract", "version": "1"},
        }
    request = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": "capability-check", "method": method, "params": params}).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == 200
        body = json.loads(response.read())
    assert body["id"] == "capability-check"
    return body


@pytest.mark.parametrize("enable_resources", [False, True])
@pytest.mark.parametrize("enable_prompts", [False, True])
def test_stateless_discovery_advertises_only_implemented_capabilities(enable_resources, enable_prompts):
    config = McpHttpConfig(port=0)
    config.enable_resources = enable_resources
    config.enable_prompts = enable_prompts
    server = McpHttpServer(ToolRegistry(), config)
    with server.start() as handle:
        response = _request(handle.mcp_url(), "server/discover", {}, modern=True)
        legacy = _request(
            handle.mcp_url(),
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "capability-contract", "version": "1"},
            },
        )
    expected = {"tools": {"listChanged": False}}
    if enable_resources:
        expected["resources"] = {"subscribe": False, "listChanged": False}
        # The skills extension is served over the Resources primitive, so a
        # server that advertises resources also advertises the extension.
        # It is implemented, so `advertises_only_implemented_capabilities`
        # must list it.
        expected["extensions"] = {
            "io.modelcontextprotocol/skills": {"directoryRead": True}
        }
    if enable_prompts:
        expected["prompts"] = {"listChanged": False}
    assert response["result"]["capabilities"] == expected
    # The extension is stateless-only; the legacy lifecycle never sees it.
    assert "extensions" not in legacy["result"]["capabilities"]
    assert ("resources" in legacy["result"]["capabilities"]) is enable_resources
    assert ("prompts" in legacy["result"]["capabilities"]) is enable_prompts
    assert legacy["result"]["capabilities"]["tools"]["listChanged"] is True
