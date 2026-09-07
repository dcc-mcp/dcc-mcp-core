"""Schema-driven parameter mirrors through the packaged native Python server."""

from __future__ import annotations

import base64
import json
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

_SCHEMA = {
    "type": "object",
    "properties": {
        "tenant": {"type": "string", "x-mcp-header": "Tenant"},
        "optional": {"anyOf": [{"type": "string"}, {"type": "null"}]},
    },
}


@pytest.fixture
def parameter_server():
    registry = ToolRegistry()
    registry.register("parameter_probe", input_schema=json.dumps(_SCHEMA))
    registry.register(
        "invalid_definition",
        input_schema=json.dumps(
            {
                "type": "object",
                "properties": {
                    "tenant": {"type": ["string", "null"], "x-mcp-header": "Tenant"},
                },
            }
        ),
    )
    server = McpHttpServer(registry, McpHttpConfig(port=0))
    calls = []

    def handler(arguments):
        calls.append(arguments)
        return {"accepted": True}

    server.register_handler("parameter_probe", handler, thread_affinity="any")
    server.register_handler("invalid_definition", handler, thread_affinity="any")
    with server.start() as handle:
        yield handle.mcp_url(), calls


def _post(url, method, params, extra_headers=None):
    params = dict(
        params,
        _meta={
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        },
    )
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "MCP-Protocol-Version": "2026-07-28",
        "Mcp-Method": method,
    }
    if "name" in params:
        headers["Mcp-Name"] = params["name"]
    headers.update(extra_headers or {})
    request = urllib.request.Request(
        url,
        method="POST",
        headers=headers,
        data=json.dumps(
            {
                "jsonrpc": "2.0",
                "id": "parameter-python",
                "method": method,
                "params": params,
            }
        ).encode(),
    )
    try:
        response = urllib.request.urlopen(request, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, json.loads(response.read())


@pytest.mark.parametrize("header", [None, "secret-other", "=?base64?/w==?="])
def test_header_fault_never_invokes_python_handler(parameter_server, header):
    url, calls = parameter_server
    status, body = _post(
        url,
        "tools/call",
        {"name": "parameter_probe", "arguments": {"tenant": "secret-value"}},
        {} if header is None else {"Mcp-Param-Tenant": header},
    )
    assert status == 400
    assert body["id"] == "parameter-python"
    assert body["error"]["code"] == -32020
    assert "secret" not in json.dumps(body)
    assert calls == []


@pytest.mark.parametrize("value", ["café", " leading ", "=?base64?ZA==?="])
def test_valid_encoded_header_invokes_python_handler_once(parameter_server, value):
    url, calls = parameter_server
    encoded = "=?base64?" + base64.b64encode(value.encode()).decode() + "?="
    status, body = _post(
        url, "tools/call", {"name": "parameter_probe", "arguments": {"tenant": value}}, {"mcp-param-tenant": encoded}
    )
    assert status == 200
    assert body["result"].get("isError") is not True
    assert calls == [{"tenant": value}]


def test_source_projection_and_invalid_definition_fail_closed(parameter_server):
    url, calls = parameter_server
    status, body = _post(url, "tools/list", {})
    assert status == 200
    tools = {tool["name"]: tool for tool in body["result"]["tools"]}
    assert tools["parameter_probe"]["inputSchema"] == _SCHEMA
    assert "invalid_definition" not in tools
    status, body = _post(url, "tools/call", {"name": "invalid_definition", "arguments": {}})
    assert status == 200
    assert body["error"]["code"] == -32603
    assert calls == []
