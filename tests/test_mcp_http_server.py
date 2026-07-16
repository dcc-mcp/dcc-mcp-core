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
import gc
import json
from threading import Thread
import time
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

from conftest import McpClient

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── helpers ───────────────────────────────────────────────────────────────


def _get_json(url: str, headers: dict[str, str] | None = None) -> tuple[int, dict[str, Any]]:
    """GET a JSON endpoint and return (status_code, response_body)."""
    req = urllib.request.Request(
        url,
        headers={"Accept": "application/json, text/event-stream", **(headers or {})},
        method="GET",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _rest_base(mcp_url: str) -> str:
    """Return the HTTP listener base URL for /v1 REST routes."""
    return mcp_url.rsplit("/mcp", 1)[0]


def _post_json_rest(url: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    """POST JSON to a REST endpoint (non-MCP). Used for /v1/* routes."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _wait_unreachable(url: str, timeout: float = 2.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(url, timeout=0.2)
        except Exception:
            return
        time.sleep(0.05)
    raise AssertionError(f"server still reachable at {url}")


def _make_registry() -> ToolRegistry:
    reg = ToolRegistry()
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


# ── handle lifecycle tests ────────────────────────────────────────────────


def test_handle_context_manager_shutdowns_server():
    reg = _make_registry()
    server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="ctx-test"))
    with server.start() as handle:
        url = handle.mcp_url()
        client = McpClient(url)
        code, _ = client.post({"jsonrpc": "2.0", "id": 1, "method": "ping"})
        assert code == 200
    _wait_unreachable(url)


def test_handle_shutdown_on_drop_stops_server():
    reg = _make_registry()
    config = McpHttpConfig(port=0, server_name="drop-test", shutdown_on_drop=True)
    server = McpHttpServer(reg, config)
    handle = server.start()
    url = handle.mcp_url()
    client = McpClient(url)
    code, _ = client.post({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    assert code == 200

    del handle
    gc.collect()

    _wait_unreachable(url)


# ── basic HTTP protocol tests (stdlib only) ───────────────────────────────


class TestMcpHttpProtocol:
    """Test the raw MCP Streamable HTTP protocol using McpClient."""

    def test_initialize(self, running_server):
        _, _, url = running_server
        client = McpClient(url, auto_init=False)
        resp = client.initialize(protocol_version="2025-03-26")
        # rmcp returns the highest protocol version it supports
        assert resp["protocolVersion"] in ("2025-03-26", "2025-06-18", "2025-11-25")
        assert resp["serverInfo"]["name"] == "e2e-test-server"
        assert "tools" in resp["capabilities"]

    def test_tools_list(self, running_server):
        _, _, url = running_server
        client = McpClient(url)
        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
            },
        )
        assert code == 200
        tools = body["result"]["tools"]
        assert isinstance(tools, list)
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
        client = McpClient(url)
        code, body = client.post(
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

    def test_tools_call_passes_dict_to_handler(self):
        reg = ToolRegistry()
        reg.register(
            "echo_args",
            description="Echo args",
            category="test",
            tags=[],
            dcc="test",
            version="1.0.0",
        )
        server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="dict-args-test"))
        received = []
        server.register_handler("echo_args", lambda params: received.append(params) or params)
        handle = server.start()
        try:
            client = McpClient(handle.mcp_url())
            code, body = client.post(
                {
                    "jsonrpc": "2.0",
                    "id": 31,
                    "method": "tools/call",
                    "params": {"name": "echo_args", "arguments": {"count": 2, "label": "cube"}},
                },
            )
            assert code == 200
            assert body["result"]["isError"] is False
            assert received == [{"count": 2, "label": "cube"}]
        finally:
            handle.shutdown()

    def test_rest_routes_are_mounted_on_python_server(self, running_server):
        _, _, url = running_server
        base = _rest_base(url)

        code, body = _get_json(f"{base}/v1/healthz")
        assert code == 200
        assert body["ok"] is True

        code, body = _post_json_rest(f"{base}/v1/search", {"query": "scene", "loaded_only": True})
        assert code == 200
        slugs = {hit["slug"] for hit in body["hits"]}
        assert "test.core.get_scene_info" in slugs

    def test_rest_describe_and_call_use_registered_python_handler(self, running_server):
        _, _, url = running_server
        base = _rest_base(url)
        slug = "test.core.get_scene_info"

        code, body = _post_json_rest(f"{base}/v1/describe", {"tool_slug": slug, "include_schema": True})
        assert code == 200
        assert body["entry"]["slug"] == slug
        assert body["entry"]["action"] == "get_scene_info"
        assert "input_schema" in body

        code, body = _post_json_rest(f"{base}/v1/call", {"tool_slug": slug, "params": {}})
        assert code == 200
        assert body["slug"] == slug
        assert body["output"]["scene"] == "test_scene"

    def test_mcp_http_server_exposes_downstream_reuse_api(self, running_server):
        server, _, _ = running_server
        expected = {
            "register_handler",
            "has_handler",
            "set_in_process_executor",
            "clear_in_process_executor",
            "discover",
            "load_skill",
            "unload_skill",
            "list_skills",
            "search_skills",
            "get_skill_info",
            "is_loaded",
            "loaded_count",
            "start",
        }
        missing = sorted(name for name in expected if not hasattr(server, name))
        assert missing == []
        assert hasattr(server, "registry")
        assert hasattr(server.registry, "get_action")
        assert hasattr(server.registry, "search_actions")
        assert hasattr(server.registry, "list_actions")

    def test_tools_call_unknown(self, running_server):
        _, _, url = running_server
        client = McpClient(url)
        code, body = client.post(
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
        client = McpClient(url)
        code, body = client.post({"jsonrpc": "2.0", "id": 5, "method": "ping"})
        assert code == 200
        assert body["id"] == 5
        assert body.get("result") is not None

    def test_method_not_found(self, running_server):
        _, _, url = running_server
        client = McpClient(url)
        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": 6,
                "method": "unknown/method",
            },
        )
        assert code == 200
        assert body["error"]["code"] == -32601

    def test_malformed_json_returns_parse_error(self, running_server):
        """Malformed JSON must fail with a 4xx error, not hang or 500."""
        _, _, url = running_server
        client = McpClient(url)
        code, _text = client.post_raw(b'{"jsonrpc":"2.0","id":7,"method":')

        # rmcp returns 400 Bad Request for malformed JSON
        assert code in (400, 415)

    def test_tools_call_missing_name_returns_invalid_params(self, running_server):
        """A malformed tools/call request without the name field returns an error."""
        _, _, url = running_server
        client = McpClient(url)
        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {"arguments": {}},
            },
        )

        assert code == 200
        assert body["jsonrpc"] == "2.0"
        assert body["id"] == 8
        # rmcp treats this as a method-not-found because without the "name" field,
        # the request doesn't deserialize into CallToolRequestParams and falls back
        # to a CustomRequest which is dispatched to an unknown method handler.
        assert "error" in body
        assert body["error"]["code"] < 0  # Some negative JSON-RPC error code

    def test_missing_method_returns_invalid_request(self, running_server):
        """A request-like object with id but no method is rejected."""
        _, _, url = running_server
        client = McpClient(url)
        code, _text = client.post_raw(json.dumps({"jsonrpc": "2.0", "id": 9}).encode())

        # rmcp rejects messages without a "method" field as unparseable
        # (returns 415 because it doesn't match the expected JSON-RPC structure)
        assert code in (200, 400, 415)

    def test_empty_batch_returns_invalid_request(self, running_server):
        """JSON-RPC batches are not supported by rmcp (returns 415)."""
        _, _, url = running_server
        client = McpClient(url)
        code, _text = client.post_raw(json.dumps([]).encode())
        # rmcp doesn't support batch requests; returns 415 Unsupported Media Type
        assert code == 415

    def test_client_response_message_is_accepted_without_response(self, running_server):
        """Client responses to server-initiated requests are acknowledgements, not new requests."""
        _, _, url = running_server
        client = McpClient(url)
        code, text = client.post_raw(b'{"jsonrpc":"2.0","id":"roots-1","result":{"roots":[]}}')

        assert code == 202
        assert text == ""

    def test_notification_returns_202(self, running_server):
        """Notifications (no id) must return 202, not 200."""
        _, _, url = running_server
        client = McpClient(url)
        code, _text = client.post_raw(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}).encode())
        assert code == 202

    def test_batch_request(self, running_server):
        """Rmcp does not support JSON-RPC batch requests (returns 415)."""
        _, _, url = running_server
        client = McpClient(url)
        code, _text = client.post_raw(
            json.dumps(
                [
                    {"jsonrpc": "2.0", "id": 10, "method": "ping"},
                    {"jsonrpc": "2.0", "id": 11, "method": "tools/list"},
                ]
            ).encode(),
        )
        # rmcp doesn't support batch requests
        assert code == 415

    def test_delete_session_not_found(self, running_server):
        """DELETE returns 405 in stateless mode (no sessions)."""
        _, _, url = running_server
        req = urllib.request.Request(
            url,
            headers={"Mcp-Session-Id": "nonexistent-session"},
            method="DELETE",
        )
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                # In stateless mode, DELETE is not supported
                assert resp.status in (404, 405)
        except urllib.error.HTTPError as e:
            # 405 Method Not Allowed (stateless) or 404 Not Found (stateful)
            assert e.code in (404, 405)

    def test_session_lifecycle(self, running_server):
        """Stateless mode: initialize → tools/list (no session management)."""
        _, _, url = running_server
        client = McpClient(url)

        # In stateless mode, session_id may be None (no Mcp-Session-Id header)
        # Just verify the server responds to requests

        # tools/list works without session
        code, body = client.post(
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        )
        assert code == 200
        assert len(body["result"]["tools"]) >= 2

    def test_concurrent_requests(self, running_server):
        """Multiple concurrent requests from different threads all succeed."""
        _, _, url = running_server
        results = []
        errors = []

        def worker(req_id: int) -> None:
            try:
                client = McpClient(url)
                code, body = client.post(
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
            assert result.protocolVersion in ("2025-03-26", "2025-06-18", "2025-11-25")

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
        assert cfg.port == 0
        assert cfg.server_name == "dcc-mcp"

    def test_config_custom(self):
        cfg = McpHttpConfig(port=9000, server_name="my-dcc", enable_cors=True)
        assert cfg.port == 9000
        assert cfg.server_name == "my-dcc"

    def test_server_start_stop(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        assert handle.port > 0
        assert "127.0.0.1" in handle.bind_addr
        assert handle.mcp_url().startswith("http://127.0.0.1")
        handle.shutdown()

    def test_server_is_reachable_after_start(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        try:
            url = handle.mcp_url()
            client = McpClient(url)
            code, _body = client.post(
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
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=8765, server_name="test")
        server = McpHttpServer(reg, cfg)
        r = repr(server)
        assert "McpHttpServer" in r
        assert "test" in r
