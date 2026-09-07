"""Real HTTP provider parity for the explicitly selected stateless path."""

from __future__ import annotations

import json
from pathlib import Path
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import create_skill_server


def _request(url, method, params=None, session=None, modern=True, expected_status=200):
    params = dict(params or {})
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    if modern:
        params["_meta"] = {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "provider-contract", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {},
        }
        headers.update({"MCP-Protocol-Version": "2026-07-28", "Mcp-Method": method})
        if "name" in params or "uri" in params:
            headers["Mcp-Name"] = str(params.get("name", params.get("uri")))
    elif session:
        headers["Mcp-Session-Id"] = session
    request = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": method, "method": method, "params": params}).encode(),
        headers=headers,
    )
    try:
        response = urllib.request.urlopen(request, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        assert response.status == expected_status
        body = json.loads(response.read())
        assert body["id"] == method
        if modern:
            assert response.headers.get("Mcp-Session-Id") is None
            if "result" in body:
                result = body["result"]
                assert result["resultType"] == "complete"
                assert isinstance(result["_meta"]["io.modelcontextprotocol/serverInfo"], dict)
                if method in ("server/discover", "resources/list", "resources/read", "prompts/list"):
                    assert result["ttlMs"] == 0
                    assert result["cacheScope"] == "private"
                else:
                    assert "ttlMs" not in result
        elif "result" in body:
            assert "resultType" not in body["result"]
        return body, response.headers.get("Mcp-Session-Id")


@pytest.mark.parametrize(
    ("method", "params"),
    [
        ("resources/list", {}),
        ("resources/read", {"uri": "scene://current"}),
        ("prompts/list", {}),
        ("prompts/get", {"name": "missing"}),
    ],
)
def test_disabled_stateless_provider_methods_return_http_not_found(method, params):
    config = McpHttpConfig(port=0)
    config.enable_resources = False
    config.enable_prompts = False
    server = McpHttpServer(ToolRegistry(), config)
    with server.start() as handle:
        body, _ = _request(handle.mcp_url(), method, params, expected_status=404)
    assert body["error"]["code"] == -32601
    assert "result" not in body


@pytest.fixture(params=["blender", "maya"])
def provider_server(request, tmp_path, monkeypatch):
    monkeypatch.setenv("DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS", "1")
    config = McpHttpConfig(port=0, server_name="stateless-provider-contract")
    config.gateway_port = 0
    config.registry_dir = str(tmp_path / "registry")
    dcc = request.param
    root = Path(__file__).parent / "fixtures" / "prompts_skills" / (dcc + "-only")
    server = create_skill_server(dcc, config, extra_paths=[str(root)])
    server.load_skill(dcc + "-prompts-demo")
    server.resources().set_scene({"dcc": dcc, "fixture": "provider-parity"})
    with server.start() as handle:
        url = handle.mcp_url()
        _, session = _request(
            url,
            "initialize",
            {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}},
            modern=False,
        )
        yield url, session


def test_stateless_discovery_advertises_wired_providers_without_notifications(provider_server):
    response, _ = _request(provider_server[0], "server/discover")
    capabilities = response["result"]["capabilities"]
    assert capabilities["resources"] == {"subscribe": False, "listChanged": False}
    assert capabilities["prompts"] == {"listChanged": False}
    assert capabilities["tools"]["listChanged"] is False
    assert "tasks" not in capabilities


def test_stateless_resource_list_and_read_use_the_registered_provider(provider_server):
    url, session = provider_server
    legacy, _ = _request(url, "resources/list", session=session, modern=False)
    modern, _ = _request(url, "resources/list")
    assert {row["uri"] for row in modern["result"]["resources"]} == {
        row["uri"] for row in legacy["result"]["resources"]
    }
    assert any(row["uri"] == "scene://current" for row in modern["result"]["resources"])
    legacy, _ = _request(url, "resources/read", {"uri": "scene://current"}, session, modern=False)
    modern, _ = _request(url, "resources/read", {"uri": "scene://current"})
    assert modern["result"]["contents"] == legacy["result"]["contents"]
    assert "provider-parity" in modern["result"]["contents"][0]["text"]


def test_stateless_prompt_list_and_get_use_the_registered_provider(provider_server):
    url, session = provider_server
    legacy, _ = _request(url, "prompts/list", session=session, modern=False)
    modern, _ = _request(url, "prompts/list")
    assert modern["result"]["prompts"] == legacy["result"]["prompts"]
    assert modern["result"]["prompts"]
    prompt = modern["result"]["prompts"][0]
    params = {
        "name": prompt["name"],
        "arguments": {row["name"]: "contract-value" for row in prompt.get("arguments", [])},
    }
    legacy, _ = _request(url, "prompts/get", params, session, modern=False)
    modern, _ = _request(url, "prompts/get", params)
    assert modern["result"]["messages"] == legacy["result"]["messages"]
    assert "contract-value" in json.dumps(modern["result"]["messages"])


@pytest.mark.parametrize(
    ("method", "params"),
    [
        ("resources/read", {}),
        ("prompts/get", {"name": "missing", "arguments": []}),
        ("resources/list", {"cursor": "bad-cursor"}),
    ],
)
def test_stateless_provider_invalid_params_are_not_empty_success(provider_server, method, params):
    body, _ = _request(provider_server[0], method, params)
    assert body["error"]["code"] == -32602
    assert "result" not in body
