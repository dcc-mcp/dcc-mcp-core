"""End-to-end tests for the MCP Resources primitive (issue #350).

Boots a real ``McpHttpServer`` with a dummy ``ToolRegistry``, then hits it
over HTTP using ``urllib`` to exercise the resources JSON-RPC surface.
"""

from __future__ import annotations

import hashlib
import json
from typing import Any
import urllib.error
import urllib.request

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core._server.options import DccServerOptions
from dcc_mcp_core.server_base import DccServerBase


def _post_json(
    url: str,
    body: dict[str, Any],
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, Any]]:
    """POST a JSON-RPC message using McpClient and return (status_code, response_body)."""
    params = body.get("params")
    request_meta = params.get("_meta", {}) if isinstance(params, dict) else {}
    is_modern = request_meta.get("io.modelcontextprotocol/protocolVersion")
    client = McpClient(
        url,
        auto_init=body.get("method") != "initialize" and not is_modern,
    )
    if is_modern:
        client.protocol_version = request_meta["io.modelcontextprotocol/protocolVersion"]
    if body.get("method") == "initialize":
        client.protocol_version = body.get("params", {}).get(
            "protocolVersion",
            "2025-11-25",
        )
    request_headers = dict(headers or {})
    if client.protocol_version >= "2026-07-28":
        request_headers.setdefault("Mcp-Method", body.get("method", ""))
        method = body.get("method")
        name_key = (
            "uri"
            if method
            in {
                "resources/read",
                "resources/subscribe",
                "resources/unsubscribe",
            }
            else "name"
            if method in {"tools/call", "prompts/get"}
            else None
        )
        if name_key and isinstance(params, dict) and params.get(name_key):
            request_headers.setdefault("Mcp-Name", params[name_key])
    code, resp = client.post(body, extra_headers=request_headers)
    return code, resp


def _modern_meta() -> dict[str, Any]:
    return {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "pytest-modern", "version": "1.0"},
    }


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
        client = McpClient(url)
        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": 11,
                "method": "resources/subscribe",
                "params": {"uri": "scene://current"},
            },
        )
        assert code == 200
        assert body.get("error") is None

        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": 12,
                "method": "resources/unsubscribe",
                "params": {"uri": "scene://current"},
            },
        )
        assert code == 200
        assert body.get("error") is None


def test_dcc_server_base_registers_custom_resource_producer(tmp_path):
    """Adapters can publish resources through DccServerBase without private access."""

    class AdapterServer(DccServerBase):
        pass

    skills_dir = tmp_path / "skills"
    skills_dir.mkdir()
    opts = DccServerOptions.from_env(
        "resource-test",
        skills_dir,
        port=0,
        enable_gateway_failover=False,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    server = AdapterServer(opts)
    server.register_resource_producer(
        "adapter-docs://status",
        lambda uri: {"mimeType": "text/plain", "text": f"resource: {uri}"},
    )

    handle = server.start(install_atexit_hook=False)
    try:
        code, body = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 20,
                "method": "resources/read",
                "params": {"uri": "adapter-docs://status"},
            },
        )
        assert code == 200
        assert "error" not in body
        item = body["result"]["contents"][0]
        assert item["uri"] == "adapter-docs://status"
        assert item["mimeType"] == "text/plain"
        assert item["text"] == "resource: adapter-docs://status"
    finally:
        server.stop()


def test_skills_extension_lists_and_reads_a_catalogued_skill(tmp_path):
    """The Skills extension uses the catalog without loading the skill tools."""

    class AdapterServer(DccServerBase):
        pass

    skills_root = tmp_path / "skills"
    skill_dir = skills_root / "demo"
    references_dir = skill_dir / "references"
    references_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(
        "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
        encoding="utf-8",
    )
    guide = b"# Guide\n"
    (references_dir / "guide.md").write_bytes(guide)

    opts = DccServerOptions.from_env(
        "skills-extension-test",
        skills_root,
        port=0,
        enable_gateway_failover=False,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    server = AdapterServer(opts)
    server.register_builtin_actions(
        extra_skill_paths=[str(skills_root)],
        include_bundled=False,
    )
    handle = server.start(install_atexit_hook=False)
    try:
        code, discovered = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 29,
                "method": "server/discover",
                "params": {"_meta": _modern_meta()},
            },
        )
        assert code == 200, discovered
        assert "2026-07-28" in discovered["result"]["supportedVersions"]
        assert (
            discovered["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/skills"]["directoryRead"]
            is True
        )

        code, listed = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 30,
                "method": "skills/list",
                "params": {"_meta": _modern_meta()},
            },
        )
        assert code == 200, listed
        assert listed["result"]["resultType"] == "complete"
        assert listed["result"]["cacheScope"] == "private"
        assert listed["result"]["ttlMs"] == 0
        skill = next(item for item in listed["result"]["skills"] if item["uri"] == "skill://demo/SKILL.md")
        guide_entry = next(item for item in skill["resources"] if item["uri"] == "skill://demo/references/guide.md")
        assert guide_entry["size"] == len(guide)
        assert guide_entry["digest"] == f"sha256:{hashlib.sha256(guide).hexdigest()}"

        code, fetched = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 31,
                "method": "skills/get",
                "params": {
                    "uri": "skill://demo/SKILL.md",
                    "_meta": _modern_meta(),
                },
            },
        )
        assert code == 200, fetched
        assert fetched["result"]["resultType"] == "complete"
        assert fetched["result"]["skill"]["uri"] == "skill://demo/SKILL.md"
        assert fetched["result"]["skill"]["frontmatter"]["license"] == "MIT"

        code, read = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 32,
                "method": "resources/read",
                "params": {
                    "uri": "skill://demo/references/guide.md",
                    "_meta": _modern_meta(),
                },
            },
        )
        assert code == 200, read
        assert read["result"]["contents"][0]["text"] == guide.decode()

        code, directory = _post_json(
            handle.mcp_url(),
            {
                "jsonrpc": "2.0",
                "id": 33,
                "method": "resources/directory/read",
                "params": {
                    "uri": "skill://demo/references",
                    "_meta": _modern_meta(),
                },
            },
        )
        assert code == 200
        assert directory["result"]["resources"][0]["uri"] == "skill://demo/references/guide.md"

        legacy_code, legacy = _post_json(
            handle.mcp_url(),
            {"jsonrpc": "2.0", "id": 34, "method": "skills/list"},
        )
        assert legacy_code == 200
        assert legacy["result"]["skills"][0]["uri"] == "skill://demo/SKILL.md"
        assert "resultType" not in legacy["result"]
    finally:
        server.stop()


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
