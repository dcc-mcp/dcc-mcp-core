"""Integration tests for the aggregating-facade gateway.

The facade gateway exposes a single ``/mcp`` endpoint that merges tools from
every live DCC backend into one namespaced list. These tests start two real
``McpHttpServer`` instances sharing a common ``FileRegistry`` directory and a
gateway port, then drive the gateway's MCP endpoint directly to verify:

* The first server to bind the gateway port wins the election and serves the
  aggregated endpoint; the second registers as a plain backend.
* ``tools/list`` on the gateway returns the two advertised discovery tools
  (search, describe).  Execution, lease, lifecycle, and legacy aliases like
  ``search_tools`` are hidden from the listing but still route via
  ``tools/call`` for compatibility.
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

from conftest import McpClient

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
    client = McpClient(url)
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    _, resp = client.post(body)
    return resp


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


def _unescape_cursor_safe(escaped: str) -> str | None:
    """Inverse of the Cursor-safe escape table introduced in #656.

    Returns ``None`` on malformed input so the caller can treat it as
    "not a cursor-safe gateway tool" instead of routing a corrupted
    backend name.
    """
    out: list[str] = []
    i = 0
    n = len(escaped)
    while i < n:
        ch = escaped[i]
        if ch == "_":
            # Every ``_`` opens a 3-byte ``_?_`` envelope; anything else
            # is a bug in the emitter.
            if i + 2 >= n or escaped[i + 2] != "_":
                return None
            mapped = {"U": "_", "D": ".", "H": "-"}.get(escaped[i + 1])
            if mapped is None:
                return None
            out.append(mapped)
            i += 3
        elif ch.isascii() and ch.isalnum():
            out.append(ch)
            i += 1
        else:
            return None
    return "".join(out)


def _split_gateway_prefixed_tool(name: str) -> tuple[str, str] | None:
    """Return ``(instance_prefix, tool_name)`` for the gateway-safe wire form.

    Returns ``None`` for anything else so the caller can keep its existing
    "is this a gateway-encoded tool?" filter semantics.
    """
    if name.startswith("__"):
        return None

    # Cursor-safe form (#656): ``i_<id8>__<escaped>``.
    if name.startswith("i_"):
        rest = name[len("i_") :]
        sep_idx = rest.find("__")
        if sep_idx == 8:
            prefix = rest[:8]
            escaped = rest[10:]
            if all(ch in "0123456789abcdef" for ch in prefix):
                decoded = _unescape_cursor_safe(escaped)
                if decoded is not None:
                    return prefix, decoded
    return None


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

    gateway_url = f"http://127.0.0.1:{gw_port}/mcp"

    # Poll until the gateway's instance watcher sees both registrations.
    # Fixed sleeps are fragile on slower CI runners (e.g. macOS arm64).
    deadline = time.monotonic() + 10  # generous 10-second timeout
    while True:
        resp = _post_mcp(gateway_url, "resources/read", {"uri": "gateway://instances"})
        text = resp["result"]["contents"][0]["text"]
        data = json.loads(text)
        visible = {e["dcc_type"] for e in data.get("instances", [])}
        if "maya" in visible and "blender" in visible:
            break
        if time.monotonic() > deadline:
            raise RuntimeError(
                f"Gateway instance watcher did not discover both backends "
                f"within timeout. Visible: {visible}"
            )
        time.sleep(0.5)

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
    """Gateway ``tools/list`` exposes only the canonical MCP workflow surface.

    The gateway publishes exactly four tools: ``search``, ``describe``,
    ``load_skill``, and ``call``. Lease, lifecycle, and legacy aliases remain
    callable through ``tools/call`` but are hidden from ``tools/list``. Backend
    per-action tools are NOT fanned out — agents discover them via ``search`` →
    ``describe``, optionally activate skills via ``load_skill``, then execute
    through ``call``.
    """

    def test_aggregated_list_contains_local_and_backend_tools(self, facade_cluster):
        tools = _list_all_tools(facade_cluster["gateway_url"])
        names = {t["name"] for t in tools}

        # Tier 1 — gateway discovery meta-tools were promoted to MCP resources
        # in #813 phase 1+2. Verify they are absent from tools/list.
        for removed in ("list_dcc_instances", "get_dcc_instance", "connect_to_dcc"):
            assert removed not in names, (
                f"meta-tool {removed!r} should have been removed from tools/list "
                "(promoted to gateway://instances resource in #813)"
            )

        # Tier 2 — advertised gateway canonical workflow tools.
        for mgmt in ("search", "describe", "load_skill", "call"):
            assert mgmt in names, f"missing skill-management tool {mgmt!r}"
        assert names == {"search", "describe", "load_skill", "call"}, (
            f"gateway tools/list should advertise only canonical workflow tools; got {sorted(names)!r}"
        )

        # Tier 3 — hidden compatibility and state-changing routes remain
        # callable via tools/call, but are not advertised in tools/list.
        for alias in (
            "lease",
            "unload_skill",
            "search_tools",
            "describe_tool",
            "call_tool",
        ):
            assert alias not in names, f"route {alias!r} should be hidden from tools/list"

        # Per-action backend tools must NOT be fanned out anymore.
        prefixed = [t for t in tools if _split_gateway_prefixed_tool(t["name"]) is not None]
        assert prefixed == [], (
            f"gateway must not fan out backend tools; discover via search/call. got {[t['name'] for t in prefixed]}"
        )

    def test_backend_tools_carry_instance_metadata(self, facade_cluster):
        """Backend per-action tools are discoverable via search_tools, carrying instance metadata."""
        client = McpClient(facade_cluster["gateway_url"])
        _, payload = client.post(
            {
                "jsonrpc": "2.0",
                "id": "search",
                "method": "tools/call",
                "params": {"name": "search_tools", "arguments": {"query": "create"}},
            }
        )
        # search_tools must return a non-empty hit list that references the
        # colliding maya+blender create_cube tools by instance id.
        content = payload["result"]["content"]
        text = "".join(c.get("text", "") for c in content if c.get("type") == "text")
        assert "create_sphere" in text, f"search_tools must surface backend tools; got {text!r}"
        assert "create_cube" in text


class TestFacadeDiscovery:
    """The ``gateway://instances`` MCP resource exposes the same set the facade aggregates.

    In #813 phase 1 the ``list_dcc_instances`` tool was removed and replaced by
    the ``gateway://instances`` MCP resource (``resources/read`` method).
    """

    def test_gateway_instances_resource_reports_both_backends(self, facade_cluster):
        resp = _post_mcp(
            facade_cluster["gateway_url"],
            "resources/read",
            {"uri": "gateway://instances"},
        )
        text = resp["result"]["contents"][0]["text"]
        data = json.loads(text)
        dccs = {entry["dcc_type"] for entry in data.get("instances", [])}
        assert "maya" in dccs, f"maya backend not visible through gateway://instances: {dccs}"
        assert "blender" in dccs, f"blender backend not visible through gateway://instances: {dccs}"
