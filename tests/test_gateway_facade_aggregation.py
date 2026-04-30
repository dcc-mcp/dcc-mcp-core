"""Integration tests for the aggregating-facade gateway.

The facade gateway exposes a single ``/mcp`` endpoint that merges tools from
every live DCC backend into one namespaced list. These tests start two real
``McpHttpServer`` instances sharing a common ``FileRegistry`` directory and a
gateway port, then drive the gateway's MCP endpoint directly to verify:

* The first server to bind the gateway port wins the election and serves the
  aggregated endpoint; the second registers as a plain backend.
* ``tools/list`` on the gateway returns the 3 discovery meta-tools, the 6
  skill-management tools, plus every backend tool namespaced with an 8-char
  instance prefix (``<short>__<original>``).
* Tool-name collisions across DCCs never surface — each backend tool carries
  ``_instance_id`` / ``_dcc_type`` annotations so agents can disambiguate.
* ``initialize`` advertises ``tools.listChanged: true`` and
  ``resources.listChanged: true`` so clients know to subscribe to SSE.

These tests run on real TCP so they need short timeouts and an unused port
range; we pick a fixed port far above the default (9765) to avoid collisions
with concurrent development instances.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
from pathlib import Path
import socket
import time
import urllib.request

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── helpers ───────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    """Return a port that is currently free on 127.0.0.1.

    We bind to port 0, read the assigned port, close, and return.  Small race
    window is acceptable for tests — the gateway/backend binds again inside
    ``start()`` and pytest runs serially per module.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _list_all_tools(url: str) -> list[dict]:
    """Collect every tools/list page from a paginated gateway response."""
    tools: list[dict] = []
    cursor: str | None = None
    rpc_id = 1
    while True:
        params = {"cursor": cursor} if cursor is not None else None
        resp = _post_mcp(url, "tools/list", params=params, rpc_id=rpc_id)
        result = resp["result"]
        tools.extend(result["tools"])
        cursor = result.get("nextCursor")
        if cursor is None:
            return tools
        rpc_id += 1


def _split_gateway_prefixed_tool(name: str) -> tuple[str, str] | None:
    """Return ``(instance_prefix, tool_name)`` for ``<id8>.<tool>`` names."""
    if name.startswith("__"):
        return None
    prefix, sep, suffix = name.partition(".")
    if not sep:
        return None
    if len(prefix) != 8 or not all(ch.isascii() and ch in "0123456789abcdef" for ch in prefix):
        return None
    return prefix, suffix


def _make_backend(dcc: str, tool_names: list[str], registry_dir: Path, gw_port: int) -> tuple[McpHttpServer, object]:
    """Start a backend McpHttpServer registered in ``registry_dir``.

    Each backend registers one action per name so the gateway's aggregated
    ``tools/list`` has something to merge.  Returns ``(server, handle)``.
    """
    reg = ToolRegistry()
    for name in tool_names:
        reg.register(name=name, description=f"{dcc}:{name}", dcc=dcc, version="1.0.0")

    cfg = McpHttpConfig(port=0, server_name=f"{dcc}-test")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = dcc
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(reg, cfg)
    handle = server.start()
    return server, handle


# ── fixture ───────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def facade_cluster(tmp_path_factory):
    """Spin up 2 backends + gateway, yield the gateway URL + handles."""
    registry_dir = tmp_path_factory.mktemp("facade-registry")
    gw_port = _pick_free_port()

    # First server wins the gateway election and hosts the facade /mcp.
    # The server reference is retained inside the handle; we keep the local
    # binding alive via ``_server_a`` so the background tasks don't drop early.
    _server_a, handle_a = _make_backend("maya", ["create_sphere", "create_cube", "delete_node"], registry_dir, gw_port)

    # Give the gateway a moment to bind and write the sentinel before the
    # second server tries to register and loses the election.
    time.sleep(0.25)

    _server_b, handle_b = _make_backend("blender", ["create_cube", "add_material"], registry_dir, gw_port)

    # Let the gateway's 2-second instance watcher see both registrations.
    time.sleep(2.2)

    gateway_url = f"http://127.0.0.1:{gw_port}/mcp"

    try:
        yield {
            "gateway_url": gateway_url,
            "gateway_port": gw_port,
            "handle_a": handle_a,
            "handle_b": handle_b,
        }
    finally:
        for h in (handle_b, handle_a):
            with contextlib.suppress(Exception):
                h.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────────


class TestFacadeInitialize:
    """The gateway ``initialize`` response advertises the facade capabilities."""

    def test_initialize_reports_list_changed(self, facade_cluster):
        resp = _post_mcp(
            facade_cluster["gateway_url"],
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "facade-test", "version": "0.1"},
            },
        )
        result = resp["result"]
        # The facade's tool list changes every time a skill loads on any backend.
        assert result["capabilities"]["tools"]["listChanged"] is True
        # Resource list changes as DCC instances join/leave.
        assert result["capabilities"]["resources"]["listChanged"] is True
        # Server identity is the gateway-flavoured name.
        assert "gateway" in result["serverInfo"]["name"].lower()


class TestFacadeToolsAggregation:
    """``tools/list`` on the gateway merges every backend's tools into one list."""

    def test_aggregated_list_contains_local_and_backend_tools(self, facade_cluster):
        tools = _list_all_tools(facade_cluster["gateway_url"])
        names = {t["name"] for t in tools}

        # Tier 1 — gateway discovery meta-tools.
        for meta in ("list_dcc_instances", "get_dcc_instance", "connect_to_dcc"):
            assert meta in names, f"missing meta-tool {meta!r}"

        # Tier 2 — skill-management tools (one canonical set gateway-side).
        for mgmt in ("list_skills", "search_skills", "get_skill_info", "load_skill", "unload_skill"):
            assert mgmt in names, f"missing skill-management tool {mgmt!r}"

        # Tier 3 — backend tools, each prefixed with an 8-char instance id.
        # We expect at least one namespaced tool whose suffix matches each
        # original backend name. Colliding names (``create_cube`` registered
        # on both maya and blender) must survive as two distinct entries.
        prefixed = [t for t in tools if _split_gateway_prefixed_tool(t["name"]) is not None]
        suffixes = [_split_gateway_prefixed_tool(t["name"])[1] for t in prefixed]

        assert "create_sphere" in suffixes, "maya.create_sphere missing from aggregated list"
        assert "delete_node" in suffixes, "maya.delete_node missing from aggregated list"
        assert "add_material" in suffixes, "blender.add_material missing from aggregated list"

        # create_cube lives on BOTH backends — it MUST appear twice with
        # different prefixes, otherwise the namespace scheme broke.
        assert suffixes.count("create_cube") == 2, (
            f"create_cube should appear once per backend, got {suffixes.count('create_cube')}"
        )

    def test_backend_tools_carry_instance_metadata(self, facade_cluster):
        tools = _list_all_tools(facade_cluster["gateway_url"])
        backend_tools = [t for t in tools if _split_gateway_prefixed_tool(t["name"]) is not None]
        assert backend_tools, "no namespaced backend tools were aggregated"

        for tool in backend_tools:
            assert "_instance_id" in tool, f"tool {tool['name']!r} missing _instance_id annotation"
            assert "_instance_short" in tool, f"tool {tool['name']!r} missing _instance_short annotation"
            assert "_dcc_type" in tool, f"tool {tool['name']!r} missing _dcc_type annotation"
            # Prefix in the name matches the short instance id.
            prefix = _split_gateway_prefixed_tool(tool["name"])[0]
            assert tool["_instance_short"] == prefix, (
                f"prefix {prefix!r} doesn't match _instance_short {tool['_instance_short']!r}"
            )


class TestFacadeDiscovery:
    """The ``list_dcc_instances`` meta-tool returns the same set the facade aggregates."""

    def test_list_dcc_instances_reports_both_backends(self, facade_cluster):
        resp = _post_mcp(
            facade_cluster["gateway_url"],
            "tools/call",
            {"name": "list_dcc_instances", "arguments": {}},
        )
        text = resp["result"]["content"][0]["text"]
        data = json.loads(text)
        dccs = {entry["dcc_type"] for entry in data.get("instances", [])}
        assert "maya" in dccs, f"maya backend not visible through gateway: {dccs}"
        assert "blender" in dccs, f"blender backend not visible through gateway: {dccs}"
