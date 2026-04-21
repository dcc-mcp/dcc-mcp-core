"""End-to-end tests for the MCP Resources primitive (issue #350).

Boots a real ``McpHttpServer`` with a dummy ``ToolRegistry``, then hits it
over HTTP using ``urllib`` to exercise the resources JSON-RPC surface.
"""

from __future__ import annotations

import json
from typing import Any
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _post_json(
    url: str,
    body: dict[str, Any],
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, Any]]:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            **(headers or {}),
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


@pytest.fixture(scope="module")
def resource_server():
    reg = ToolRegistry()
    reg.register(
        "noop",
        description="placeholder",
        category="test",
        tags=[],
        dcc="test",
        version="1.0.0",
    )
    cfg = McpHttpConfig(port=0, server_name="resources-e2e")
    assert cfg.enable_resources is True
    assert cfg.enable_artefact_resources is False
    server = McpHttpServer(reg, cfg)
    handle = server.start()
    yield server, handle, handle.mcp_url()
    handle.shutdown()


class TestResourcesCapability:
    def test_initialize_advertises_resources(self, resource_server):
        _, _, url = resource_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "pytest", "version": "1.0"},
                },
            },
        )
        assert code == 200
        caps = body["result"]["capabilities"]
        assert "resources" in caps
        assert caps["resources"]["subscribe"] is True
        assert caps["resources"]["listChanged"] is True

    def test_resources_list_includes_expected_schemes(self, resource_server):
        _, _, url = resource_server
        code, body = _post_json(
            url,
            {"jsonrpc": "2.0", "id": 2, "method": "resources/list"},
        )
        assert code == 200
        resources = body["result"]["resources"]
        uris = {r["uri"] for r in resources}
        assert "scene://current" in uris
        assert "audit://recent" in uris
        # capture:// only shown when a real window backend is available;
        # on CI (and generally in Mock mode) it is hidden. artefact://
        # is hidden when enable_artefact_resources=False.
        assert not any(u.startswith("artefact://") for u in uris)
        for r in resources:
            assert "name" in r
            assert "uri" in r

    def test_resources_read_audit_returns_json_contents(self, resource_server):
        _, _, url = resource_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "resources/read",
                "params": {"uri": "audit://recent?limit=5"},
            },
        )
        assert code == 200
        contents = body["result"]["contents"]
        assert len(contents) == 1
        item = contents[0]
        assert item["uri"] == "audit://recent?limit=5"
        assert item["mimeType"] == "application/json"
        payload = json.loads(item["text"])
        assert payload["limit"] == 5
        assert "entries" in payload

    def test_resources_read_scene_placeholder_without_snapshot(self, resource_server):
        _, _, url = resource_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "resources/read",
                "params": {"uri": "scene://current"},
            },
        )
        assert code == 200
        item = body["result"]["contents"][0]
        assert item["mimeType"] == "application/json"
        assert "no_scene_published" in item["text"]

    def test_resources_read_artefact_returns_not_enabled_error(self, resource_server):
        _, _, url = resource_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 5,
                "method": "resources/read",
                "params": {"uri": "artefact://sha256/zzz"},
            },
        )
        assert code == 200
        assert "error" in body
        assert body["error"]["code"] == -32002
        assert "artefact" in body["error"]["message"].lower()

    def test_resources_subscribe_and_unsubscribe(self, resource_server):
        _, _, url = resource_server
        # initialize to pick up a session id
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 10,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "sub", "version": "1.0"},
                },
            },
        )
        assert code == 200
        session_id = body["result"]["__session_id"]

        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 11,
                "method": "resources/subscribe",
                "params": {"uri": "scene://current"},
            },
            headers={"Mcp-Session-Id": session_id},
        )
        assert code == 200
        assert body.get("error") is None

        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 12,
                "method": "resources/unsubscribe",
                "params": {"uri": "scene://current"},
            },
            headers={"Mcp-Session-Id": session_id},
        )
        assert code == 200
        assert body.get("error") is None


class TestResourcesDisabled:
    def test_disabled_config_hides_capability_and_methods(self):
        reg = ToolRegistry()
        reg.register(
            "noop",
            description="placeholder",
            category="test",
            tags=[],
            dcc="test",
            version="1.0.0",
        )
        cfg = McpHttpConfig(port=0, server_name="resources-disabled")
        cfg.enable_resources = False
        assert cfg.enable_resources is False
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        try:
            url = handle.mcp_url()
            _, init_body = _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {},
                        "clientInfo": {"name": "pytest", "version": "1.0"},
                    },
                },
            )
            caps = init_body["result"]["capabilities"]
            assert "resources" not in caps or caps.get("resources") is None

            _, list_body = _post_json(
                url,
                {"jsonrpc": "2.0", "id": 2, "method": "resources/list"},
            )
            assert "error" in list_body
            assert list_body["error"]["code"] == -32601  # method not found
        finally:
            handle.shutdown()
