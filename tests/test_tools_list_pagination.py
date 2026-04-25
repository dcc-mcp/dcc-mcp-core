"""Tests for tools/list pagination and delta tools notifications (issue #234).

These tests verify:
  1. tools/list cursor pagination (page size = 32, nextCursor opaque token)
  2. Delta tools notification capability negotiation during initialize
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── helpers ───────────────────────────────────────────────────────────────


def _post(url: str, body: Any, headers: dict[str, str] | None = None) -> tuple[int, Any]:
    """POST JSON and return (status, parsed_body)."""
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


def _make_big_registry(n: int = 40) -> ToolRegistry:
    """Return a registry with `n` tools so the list spans multiple pages."""
    reg = ToolRegistry()
    for i in range(n):
        reg.register(
            f"tool_{i:03d}",
            description=f"Test tool {i}",
            category="test",
            tags=[],
            dcc="test",
            version="1.0.0",
        )
    return reg


# ── fixtures ──────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def big_server():
    """Server with 40+ tools so tools/list requires pagination."""
    reg = _make_big_registry(40)
    server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="pagination-test"))
    handle = server.start()
    yield handle.mcp_url()
    handle.shutdown()


@pytest.fixture(scope="module")
def small_server():
    """Server with fewer tools than one page."""
    reg = ToolRegistry()
    reg.register("alpha", description="A", category="test", tags=[], dcc="test", version="1.0.0")
    reg.register("beta", description="B", category="test", tags=[], dcc="test", version="1.0.0")
    server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="small-server"))
    handle = server.start()
    yield handle.mcp_url()
    handle.shutdown()


# ── pagination tests ──────────────────────────────────────────────────────


class TestToolsListPagination:
    """End-to-end pagination tests via raw HTTP."""

    PAGE_SIZE = 32  # must match TOOLS_LIST_PAGE_SIZE in Rust

    def test_small_list_has_no_next_cursor(self, small_server):
        """A list smaller than PAGE_SIZE must not include nextCursor."""
        code, body = _post(small_server, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        assert code == 200
        result = body["result"]
        tools = result["tools"]
        assert len(tools) <= self.PAGE_SIZE
        assert result.get("nextCursor") is None, f"Unexpected nextCursor for small list: {result.get('nextCursor')}"

    def test_large_list_first_page_has_next_cursor(self, big_server):
        """First page of a large list must return exactly PAGE_SIZE tools and a nextCursor."""
        code, body = _post(big_server, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        assert code == 200
        result = body["result"]
        tools = result["tools"]
        assert len(tools) == self.PAGE_SIZE, f"Expected first page to have {self.PAGE_SIZE} tools, got {len(tools)}"
        assert result.get("nextCursor") is not None, "First page must include nextCursor"

    def test_all_pages_cover_all_tools_exactly_once(self, big_server):
        """Walking all pages must return every tool exactly once."""
        all_names: list[str] = []
        cursor: str | None = None

        while True:
            params: dict[str, Any] = {}
            if cursor is not None:
                params["cursor"] = cursor

            code, body = _post(
                big_server,
                {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": params},
            )
            assert code == 200
            result = body["result"]
            all_names.extend(t["name"] for t in result["tools"])
            cursor = result.get("nextCursor")
            if cursor is None:
                break

        # 11 core (incl. jobs.get_status #319 + jobs.cleanup #328) + 40 registered = 51 total
        assert len(all_names) == 51, f"Expected 51 tools across all pages, got {len(all_names)}"
        unique = set(all_names)
        assert len(unique) == len(all_names), "Pages must not return duplicate tool names"

    def test_last_page_has_no_next_cursor(self, big_server):
        """Last page must not return nextCursor."""
        # Page 1
        _, body1 = _post(big_server, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        cursor = body1["result"]["nextCursor"]
        assert cursor is not None

        # Page 2 (last)
        code, body2 = _post(
            big_server,
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {"cursor": cursor}},
        )
        assert code == 200
        result2 = body2["result"]
        assert len(result2["tools"]) == 51 - self.PAGE_SIZE
        assert result2.get("nextCursor") is None, "Last page must not have nextCursor"


# ── delta notification capability negotiation ─────────────────────────────

DELTA_CAP_KEY = "dcc_mcp_core/deltaToolsUpdate"


class TestDeltaNotificationCapability:
    """Tests for the vendored delta-tools capability negotiation."""

    def test_server_does_not_advertise_delta_by_default(self, small_server):
        """Without client opt-in, initialize must not include the delta cap."""
        code, body = _post(
            small_server,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "plain-client", "version": "1.0"},
                },
            },
        )
        assert code == 200
        experimental = body["result"]["capabilities"].get("experimental")
        assert experimental is None or DELTA_CAP_KEY not in (experimental or {}), (
            f"Server must not advertise delta cap without client opt-in, got: {experimental}"
        )

    def test_server_echoes_delta_capability_when_client_opts_in(self, small_server):
        """When client opts in, server must echo the delta capability back."""
        code, body = _post(
            small_server,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {
                        "experimental": {
                            DELTA_CAP_KEY: {"enabled": True},
                        }
                    },
                    "clientInfo": {"name": "delta-client", "version": "1.0"},
                },
            },
        )
        assert code == 200
        experimental = body["result"]["capabilities"].get("experimental") or {}
        assert DELTA_CAP_KEY in experimental, (
            f"Server must echo {DELTA_CAP_KEY} when client opts in, got experimental={experimental}"
        )
        assert experimental[DELTA_CAP_KEY].get("enabled") is True

    def test_session_id_returned_after_delta_init(self, small_server):
        """Session ID must still be present in initialize response with delta opt-in."""
        code, body = _post(
            small_server,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"experimental": {DELTA_CAP_KEY: {"enabled": True}}},
                    "clientInfo": {"name": "delta-client", "version": "1.0"},
                },
            },
        )
        assert code == 200
        session_id = body["result"].get("__session_id")
        assert session_id and len(session_id) > 0
