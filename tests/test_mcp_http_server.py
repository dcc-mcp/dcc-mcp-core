"""E2E tests for McpHttpServer via real HTTP requests (Python MCP client).

These tests start a real McpHttpServer bound to a random port, then connect
to it using the standard ``mcp`` Python SDK (if available) or plain
``urllib`` / ``http.client`` to exercise the full MCP Streamable HTTP
protocol without mocking.

Dependency:
    pip install mcp   # Anthropic's official Python MCP SDK

If ``mcp`` is not installed the SDK tests are skipped; the basic HTTP tests
always run since they only require the standard library.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from threading import Thread
import time
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer

# ── helpers ───────────────────────────────────────────────────────────────


def _post_json(url: str, body: dict[str, Any], headers: dict[str, str] | None = None) -> tuple[int, dict[str, Any]]:
    """POST a JSON-RPC message and return (status_code, response_body)."""
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


def _make_registry() -> ActionRegistry:
    reg = ActionRegistry()
    reg.register(
        "get_scene_info",
        description="Return info about the current scene",
        category="scene",
        tags=["query"],
        dcc="test",
        version="1.0.0",
    )
    reg.register(
        "list_objects",
        description="List all objects in the scene",
        category="scene",
        tags=["query", "list"],
        dcc="test",
        version="1.0.0",
    )
    return reg


# ── fixtures ──────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def running_server():
    """Start a McpHttpServer on a random port; yield (server, handle, url)."""
    reg = _make_registry()
    config = McpHttpConfig(port=0, server_name="e2e-test-server")  # port=0 → random
    server = McpHttpServer(reg, config)
    # Register handlers so tools/call actually executes (Skills-First architecture:
    # metadata is in registry, handlers are registered separately for custom logic)
    server.register_handler("get_scene_info", lambda params: {"scene": "test_scene", "objects": []})
    server.register_handler("list_objects", lambda params: {"objects": ["cube", "sphere"]})
    handle = server.start()
    url = handle.mcp_url()
    yield server, handle, url
    handle.shutdown()


# ── basic HTTP protocol tests (stdlib only) ───────────────────────────────


class TestMcpHttpProtocol:
    """Test the raw MCP Streamable HTTP protocol using stdlib urllib."""

    def test_initialize(self, running_server):
        _, _, url = running_server
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
        assert body["jsonrpc"] == "2.0"
        assert body["id"] == 1
        result = body["result"]
        assert result["protocolVersion"] == "2025-03-26"
        assert result["serverInfo"]["name"] == "e2e-test-server"
        assert "tools" in result["capabilities"]
        # Session ID attached in result
        assert "__session_id" in result

    def test_tools_list(self, running_server):
        _, _, url = running_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
            },
        )
        assert code == 200
        tools = body["result"]["tools"]
        assert isinstance(tools, list)
        # tools/list now always includes 5 core discovery tools plus registered actions
        assert len(tools) >= 2
        names = {t["name"] for t in tools}
        assert "get_scene_info" in names
        assert "list_objects" in names
        # Every tool must have name, description, inputSchema
        for tool in tools:
            assert "name" in tool
            assert "description" in tool
            assert "inputSchema" in tool

    def test_tools_call_known(self, running_server):
        _, _, url = running_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "get_scene_info", "arguments": {}},
            },
        )
        assert code == 200
        result = body["result"]
        # With Skills-First architecture, registered handlers execute properly
        assert result["isError"] is False
        assert len(result["content"]) > 0
        assert result["content"][0]["type"] == "text"
        # Handler returned a dict with scene info
        content_text = result["content"][0]["text"]
        assert "scene" in content_text or "test_scene" in content_text

    def test_tools_call_unknown(self, running_server):
        _, _, url = running_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "does_not_exist", "arguments": {}},
            },
        )
        assert code == 200
        assert body["result"]["isError"] is True

    def test_ping(self, running_server):
        _, _, url = running_server
        code, body = _post_json(url, {"jsonrpc": "2.0", "id": 5, "method": "ping"})
        assert code == 200
        assert body["id"] == 5
        assert body.get("result") is not None

    def test_method_not_found(self, running_server):
        _, _, url = running_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 6,
                "method": "unknown/method",
            },
        )
        assert code == 200
        assert body["error"]["code"] == -32601

    def test_notification_returns_202(self, running_server):
        """Notifications (no id) must return 202, not 200."""
        _, _, url = running_server
        data = json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}).encode()
        req = urllib.request.Request(
            url,
            data=data,
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            assert resp.status == 202

    def test_batch_request(self, running_server):
        """Batch of two requests returns array of two responses."""
        _, _, url = running_server
        code, body = _post_json(
            url,
            [
                {"jsonrpc": "2.0", "id": 10, "method": "ping"},
                {"jsonrpc": "2.0", "id": 11, "method": "tools/list"},
            ],
        )
        assert code == 200
        assert isinstance(body, list)
        assert len(body) == 2
        ids = {r["id"] for r in body}
        assert {10, 11} == ids

    def test_delete_session_not_found(self, running_server):
        """DELETE with unknown session returns 404."""
        _, _, url = running_server
        req = urllib.request.Request(
            url,
            headers={"Mcp-Session-Id": "nonexistent-session"},
            method="DELETE",
        )
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                assert resp.status in (404, 204)  # 204 if accidentally found
        except urllib.error.HTTPError as e:
            assert e.code == 404

    def test_session_lifecycle(self, running_server):
        """Full lifecycle: initialize → tools/list → delete session."""
        _, _, url = running_server

        # 1. Initialize and get session ID
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "lifecycle-test", "version": "1.0"},
                },
            },
        )
        assert code == 200
        session_id = body["result"]["__session_id"]
        assert session_id

        # 2. tools/list with session
        code, body = _post_json(
            url,
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            headers={"Mcp-Session-Id": session_id},
        )
        assert code == 200
        # tools/list includes core discovery tools plus registered actions
        assert len(body["result"]["tools"]) >= 2

        # 3. Delete session
        req = urllib.request.Request(
            url,
            headers={"Mcp-Session-Id": session_id},
            method="DELETE",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            assert resp.status == 204

    def test_concurrent_requests(self, running_server):
        """Multiple concurrent requests from different threads all succeed."""
        _, _, url = running_server
        results = []
        errors = []

        def worker(req_id: int) -> None:
            try:
                code, body = _post_json(
                    url,
                    {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "method": "ping",
                    },
                )
                results.append((req_id, code, body))
            except Exception as e:
                errors.append((req_id, str(e)))

        threads = [Thread(target=worker, args=(i,)) for i in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10)

        assert not errors, f"Concurrent request errors: {errors}"
        assert len(results) == 10
        for req_id, code, body in results:
            assert code == 200, f"req {req_id} got {code}"
            assert body["id"] == req_id


# ── MCP Python SDK tests (skipped if mcp not installed) ──────────────────

try:
    import mcp
    import mcp.client.session
    import mcp.client.streamable_http

    MCP_SDK_AVAILABLE = True
except ImportError:
    MCP_SDK_AVAILABLE = False


@pytest.mark.skipif(not MCP_SDK_AVAILABLE, reason="mcp Python SDK not installed")
class TestMcpSdkClient:
    """Test using the official Anthropic MCP Python SDK client."""

    @pytest.mark.anyio
    async def test_sdk_initialize_and_list_tools(self, running_server):
        """Full MCP handshake via SDK: initialize + tools/list."""
        import mcp.client.session
        import mcp.client.streamable_http

        _, _, url = running_server

        async with mcp.client.streamable_http.streamable_http_client(url) as (
            read,
            write,
            _,
        ), mcp.client.session.ClientSession(read, write) as session:
            result = await session.initialize()
            assert result.serverInfo.name == "e2e-test-server"
            assert result.protocolVersion == "2025-03-26"

            tools = await session.list_tools()
            names = {t.name for t in tools.tools}
            assert "get_scene_info" in names
            assert "list_objects" in names

    @pytest.mark.anyio
    async def test_sdk_call_tool(self, running_server):
        """Call a tool via SDK."""
        import mcp.client.session
        import mcp.client.streamable_http

        _, _, url = running_server

        async with mcp.client.streamable_http.streamable_http_client(url) as (
            read,
            write,
            _,
        ), mcp.client.session.ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("get_scene_info", {})
            assert not result.isError
            assert len(result.content) > 0


# ── McpHttpServer Python API tests ───────────────────────────────────────


class TestMcpHttpServerPythonApi:
    """Unit tests for the Python-facing McpHttpServer API."""

    def test_config_defaults(self):
        cfg = McpHttpConfig()
        assert cfg.port == 8765
        assert cfg.server_name == "dcc-mcp"

    def test_config_custom(self):
        cfg = McpHttpConfig(port=9000, server_name="my-dcc", enable_cors=True)
        assert cfg.port == 9000
        assert cfg.server_name == "my-dcc"

    def test_server_start_stop(self):
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        assert handle.port > 0
        assert "127.0.0.1" in handle.bind_addr
        assert handle.mcp_url().startswith("http://127.0.0.1")
        handle.shutdown()

    def test_server_is_reachable_after_start(self):
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        try:
            url = handle.mcp_url()
            code, _body = _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "ping",
                },
            )
            assert code == 200
        finally:
            handle.shutdown()

    def test_server_repr(self):
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=8765, server_name="test")
        server = McpHttpServer(reg, cfg)
        r = repr(server)
        assert "McpHttpServer" in r
        assert "test" in r
