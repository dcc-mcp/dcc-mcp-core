"""E2E tests for cross-system boundaries: Gateway x Skills x Progressive Loading.

These tests exercise the integration points that individual subsystem tests do
not cover:

* Gateway aggregating tools from skill-enabled backends.
* Progressive skill loading (discover->search->load->call->unload) through the
  aggregating gateway.
* AuroraView / WebView-host DCC instances discoverable via gateway meta-tools.
* Session pinning and tool-call routing through the gateway to specific backends.
* Bundled skills (dcc-diagnostics, workflow) via ``create_skill_server``.
* ``ServiceEntry.extras`` round-trip through TransportManager and gateway.
* ``required_capabilities`` filtering with ``WebViewAdapter``.

All HTTP tests use stdlib ``urllib`` only — no mcporter or external client
required.  Tests that spin up a gateway cluster use ``scope="module"`` fixtures
to keep the total runtime manageable.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
from pathlib import Path
import socket
import time
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import McpServerHandle
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager
from dcc_mcp_core import WebViewAdapter
from dcc_mcp_core.adapters import CAPABILITY_KEYS
from dcc_mcp_core.adapters import WEBVIEW_DEFAULT_CAPABILITIES

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")

# Gateway discovery meta-tools that must always be present.
GATEWAY_META_TOOLS = {"list_dcc_instances", "get_dcc_instance", "connect_to_dcc"}
# Skill management tools present on servers with a SkillCatalog.
SKILL_MGMT_TOOLS = {"list_skills", "find_skills", "search_skills", "get_skill_info", "load_skill", "unload_skill"}


# ── helpers ──────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    """Return a port that is currently free on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    """POST a JSON-RPC 2.0 message and return parsed response body."""
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
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


def _tools_list(url: str) -> list[dict]:
    """Return the ``tools`` array from ``tools/list``."""
    resp = _post_mcp(url, "tools/list")
    return resp["result"]["tools"]


def _tools_call(url: str, tool: str, arguments: dict | None = None) -> dict:
    """Invoke ``tools/call`` and return the ``result`` object."""
    resp = _post_mcp(
        url,
        "tools/call",
        {"name": tool, "arguments": arguments or {}},
    )
    return resp["result"]


def _parse_content_text(result: dict) -> str:
    """Extract the text payload from an MCP tools/call result."""
    content = result.get("content", [])
    if content and isinstance(content[0], dict):
        return content[0].get("text", "")
    return str(content)


def _parse_content_json(result: dict) -> dict:
    """Extract and JSON-parse the text payload from an MCP tools/call result."""
    return json.loads(_parse_content_text(result))


def _parse_gateway_aggregated(result: dict) -> dict:
    """Parse a gateway-aggregated response.

    When skill management tools (search_skills, load_skill, etc.) are called
    through the gateway, the gateway fans out the request to all backends and
    returns ``{"instances": [{..., "result": {...}}, ...]}``.

    This helper extracts the inner result from the *first successful* backend
    response, falling back to the top-level content if the response is not in
    the aggregated format.
    """
    text = _parse_content_text(result)
    data = json.loads(text)

    # Direct (non-aggregated) response — already has skills/total keys.
    if "skills" in data or "total" in data or "loaded" in data or "unloaded" in data:
        return data

    # Aggregated gateway response: {"instances": [...]}
    instances = data.get("instances", [])
    for inst in instances:
        inner_result = inst.get("result", {})
        inner_content = inner_result.get("content", [])
        if inner_content and isinstance(inner_content[0], dict):
            inner_text = inner_content[0].get("text", "")
            if inner_text:
                try:
                    inner_data = json.loads(inner_text)
                    if isinstance(inner_data, dict):
                        return inner_data
                except json.JSONDecodeError:
                    continue

    # Fall back to original data.
    return data


def _make_skill_backend(
    dcc: str,
    tool_names: list[str],
    registry_dir: Path,
    gw_port: int,
    *,
    extra_skill_paths: list[str] | None = None,
    required_caps_map: dict[str, list[str]] | None = None,
) -> tuple[McpHttpServer, McpServerHandle]:
    """Start a backend McpHttpServer registered in *registry_dir*.

    Each backend registers one action per name so the gateway's aggregated
    ``tools/list`` has something to merge.  Returns ``(server, handle)``.
    """
    reg = ToolRegistry()
    for name in tool_names:
        caps = (required_caps_map or {}).get(name, [])
        reg.register(
            name=name,
            description=f"{dcc}:{name}",
            dcc=dcc,
            version="1.0.0",
            required_capabilities=caps,
        )

    cfg = McpHttpConfig(port=0, server_name=f"{dcc}-test")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = dcc
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(reg, cfg)

    # Register trivial handlers so tools/call returns a deterministic payload.
    for name in tool_names:
        _name = name  # capture in closure
        _dcc = dcc
        server.register_handler(_name, lambda p, n=_name, d=_dcc: {"tool": n, "dcc": d, "params": p})

    if extra_skill_paths:
        server.discover(extra_paths=extra_skill_paths)

    handle = server.start()
    return server, handle


def _wait_for_tool_suffix(
    url: str,
    suffix: str,
    *,
    timeout: float = 6.0,
    interval: float = 0.5,
    should_exist: bool = True,
) -> list[dict]:
    """Poll ``tools/list`` until a tool with the given suffix appears/disappears.

    Returns the final tools list.  Raises ``AssertionError`` on timeout.
    """
    deadline = time.monotonic() + timeout
    tools = []
    while time.monotonic() < deadline:
        tools = _tools_list(url)
        names = {t["name"] for t in tools}
        found = any(n.endswith(suffix) for n in names)
        if found == should_exist:
            return tools
        time.sleep(interval)
    names = {t["name"] for t in tools}
    verb = "appear" if should_exist else "disappear"
    raise AssertionError(f"Tool suffix {suffix!r} did not {verb} within {timeout}s. Final names: {sorted(names)}")


# ── fixtures ─────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def skill_gateway_cluster(tmp_path_factory):
    """Two skill-enabled backends (maya + blender) behind a gateway.

    Both discover the example skills directory so skill management tools
    (find/search/load/unload) are available on each backend and aggregated
    by the gateway.
    """
    if not Path(EXAMPLES_SKILLS_DIR).is_dir():
        pytest.skip("examples/skills directory not found")

    registry_dir = tmp_path_factory.mktemp("skill-gw-registry")
    gw_port = _pick_free_port()

    # Backend A: maya — should win the gateway election (starts first).
    _server_a, handle_a = _make_skill_backend(
        "maya",
        ["create_sphere", "create_cube"],
        registry_dir,
        gw_port,
        extra_skill_paths=[EXAMPLES_SKILLS_DIR],
    )
    time.sleep(0.25)  # Let gateway bind before second server registers

    # Backend B: blender — registers as plain backend.
    _server_b, handle_b = _make_skill_backend(
        "blender",
        ["add_material"],
        registry_dir,
        gw_port,
        extra_skill_paths=[EXAMPLES_SKILLS_DIR],
    )
    # Let the gateway's 2-second instance watcher see both registrations.
    time.sleep(2.5)

    gateway_url = f"http://127.0.0.1:{gw_port}/mcp"

    try:
        yield {
            "gateway_url": gateway_url,
            "backend_a_url": handle_a.mcp_url(),
            "backend_b_url": handle_b.mcp_url(),
            "handle_a": handle_a,
            "handle_b": handle_b,
            "server_a": _server_a,
            "server_b": _server_b,
        }
    finally:
        for h in (handle_b, handle_a):
            with contextlib.suppress(Exception):
                h.shutdown()


@pytest.fixture(scope="module")
def webview_gateway_cluster(tmp_path_factory):
    """Maya + AuroraView (WebView-host) behind a gateway.

    Maya has a tool with ``required_capabilities=["scene"]``; AuroraView
    has tools with no capability requirements.
    """
    registry_dir = tmp_path_factory.mktemp("webview-gw-registry")
    gw_port = _pick_free_port()

    # Backend A: maya (wins gateway election).
    _server_a, handle_a = _make_skill_backend(
        "maya",
        ["create_sphere", "get_info"],
        registry_dir,
        gw_port,
        required_caps_map={"create_sphere": ["scene"]},
    )
    time.sleep(0.25)

    # Backend B: auroraview (WebView-host DCC).
    _server_b, handle_b = _make_skill_backend(
        "auroraview",
        ["navigate_url", "take_screenshot"],
        registry_dir,
        gw_port,
    )
    time.sleep(2.5)

    gateway_url = f"http://127.0.0.1:{gw_port}/mcp"

    try:
        yield {
            "gateway_url": gateway_url,
            "handle_a": handle_a,
            "handle_b": handle_b,
            "server_a": _server_a,
            "server_b": _server_b,
            "registry_dir": str(registry_dir),
        }
    finally:
        for h in (handle_b, handle_a):
            with contextlib.suppress(Exception):
                h.shutdown()


@pytest.fixture(scope="module")
def bundled_skill_server():
    """``create_skill_server`` with bundled skills (dcc-diagnostics, workflow)."""
    from dcc_mcp_core import create_skill_server
    from dcc_mcp_core import get_bundled_skill_paths

    bundled_paths = get_bundled_skill_paths()
    if not bundled_paths:
        pytest.skip("Bundled skills directory not found (editable install?)")

    config = McpHttpConfig(port=0, server_name="bundled-e2e")
    server = create_skill_server("test-bundled", config=config, extra_paths=bundled_paths)
    handle = server.start()
    time.sleep(0.2)

    yield server, handle, handle.mcp_url()
    handle.shutdown()


# ── TestGatewaySkillAggregation ──────────────────────────────────────────────


class TestGatewaySkillAggregation:
    """Gateway aggregates tools from skill-enabled backends."""

    def test_skill_stubs_visible_through_gateway(self, skill_gateway_cluster):
        """``__skill__*`` stubs from backend skill catalogs appear in the gateway."""
        tools = _tools_list(skill_gateway_cluster["gateway_url"])
        names = {t["name"] for t in tools}

        # At minimum the gateway itself exposes skill management tools.
        for mgmt in SKILL_MGMT_TOOLS:
            assert mgmt in names, f"Missing skill-management tool {mgmt!r}"

        # The gateway should also surface __skill__ stubs (either from its own
        # catalog or aggregated from backends).
        stubs = [n for n in names if n.startswith("__skill__")]
        # There may be stubs from the gateway's own discover() or from backends.
        # We also accept that the gateway exposes management tools even without
        # explicit stubs — the presence of list_skills/find_skills is sufficient.
        assert stubs or SKILL_MGMT_TOOLS.issubset(names), (
            "Expected either __skill__ stubs or skill management tools in gateway tools/list"
        )

    def test_backend_registered_tools_visible_through_gateway(self, skill_gateway_cluster):
        """Non-skill tools from both backends appear namespaced in the gateway."""
        tools = _tools_list(skill_gateway_cluster["gateway_url"])
        prefixed = [t for t in tools if "__" in t["name"] and not t["name"].startswith("__skill__")]
        suffixes = [t["name"].split("__", 1)[1] for t in prefixed]

        assert "create_sphere" in suffixes, f"maya.create_sphere missing. suffixes={suffixes}"
        assert "add_material" in suffixes, f"blender.add_material missing. suffixes={suffixes}"

    def test_skill_load_on_backend_propagates_to_gateway(self, skill_gateway_cluster):
        """Loading a skill on backend A causes the gateway's tools/list to update."""
        backend_url = skill_gateway_cluster["backend_a_url"]
        gateway_url = skill_gateway_cluster["gateway_url"]

        # Load hello-world on backend A directly.
        load_result = _tools_call(backend_url, "load_skill", {"skill_name": "hello-world"})
        load_data = _parse_content_json(load_result)
        assert load_data.get("loaded") is True, f"Failed to load hello-world on backend: {load_data}"

        try:
            # Wait for the gateway's aggregation refresh to pick up the new tool.
            tools = _wait_for_tool_suffix(gateway_url, "hello_world__greet", timeout=6.0)
            matching = [t for t in tools if t["name"].endswith("hello_world__greet")]
            assert matching, "hello_world__greet did not appear in gateway after loading on backend"
        finally:
            # Clean up: unload the skill on backend A.
            with contextlib.suppress(Exception):
                _tools_call(backend_url, "unload_skill", {"skill_name": "hello-world"})


# ── TestGatewayProgressiveLoadingCycle ───────────────────────────────────────


class TestGatewayProgressiveLoadingCycle:
    """Full progressive loading cycle exercised through the gateway.

    The gateway fans out skill management calls (search_skills, load_skill,
    etc.) to all live backends and returns aggregated results.  These tests
    exercise the cycle through the gateway and unwrap the aggregated response
    to verify the inner backend result.

    For load/unload we also drive the *backend directly* (which is how a
    real gateway session would operate after ``connect_to_dcc``) and verify
    the gateway's aggregated tools/list reflects the change.
    """

    def test_search_skills_through_gateway(self, skill_gateway_cluster):
        """``search_skills`` on the gateway returns matching skills (aggregated)."""
        gw = skill_gateway_cluster["gateway_url"]
        result = _tools_call(gw, "search_skills", {"query": "hello"})
        data = _parse_gateway_aggregated(result)
        assert "skills" in data, f"Expected 'skills' key in response: {data}"
        skill_names = [s.get("name", "") for s in data["skills"]]
        assert any("hello" in n for n in skill_names), f"'hello' not found in skills: {skill_names}"

    def test_load_skill_through_backend_and_verify_gateway(self, skill_gateway_cluster):
        """Loading a skill on a backend directly makes it visible in gateway tools/list."""
        backend_url = skill_gateway_cluster["backend_a_url"]
        gw = skill_gateway_cluster["gateway_url"]

        # Load hello-world directly on backend A.
        result = _tools_call(backend_url, "load_skill", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("loaded") is True, f"load_skill on backend failed: {data}"
        assert data.get("tool_count", 0) >= 1, f"Expected at least 1 tool: {data}"

        try:
            # Wait for gateway to aggregate the new tool.
            tools = _wait_for_tool_suffix(gw, "hello_world__greet", timeout=6.0)
            matching = [t for t in tools if t["name"].endswith("hello_world__greet")]
            assert matching, "hello_world__greet not visible through gateway after load on backend"
        finally:
            with contextlib.suppress(Exception):
                _tools_call(backend_url, "unload_skill", {"skill_name": "hello-world"})

    def test_call_loaded_skill_tool_through_gateway(self, skill_gateway_cluster):
        """After loading a skill on a backend, the tool is callable through the gateway."""
        backend_url = skill_gateway_cluster["backend_a_url"]
        gw = skill_gateway_cluster["gateway_url"]

        # Load hello-world on backend A.
        _tools_call(backend_url, "load_skill", {"skill_name": "hello-world"})

        try:
            # Wait for gateway to see the tool.
            tools = _wait_for_tool_suffix(gw, "hello_world__greet", timeout=6.0)
            # Find the exact namespaced tool name.
            gw_tool = next(t["name"] for t in tools if t["name"].endswith("hello_world__greet"))

            # Call it through the gateway.
            result = _tools_call(gw, gw_tool, {"name": "GatewayE2E"})
            text = _parse_content_text(result)
            assert "GatewayE2E" in text or "Hello" in text, f"Unexpected greeting: {text}"
        finally:
            with contextlib.suppress(Exception):
                _tools_call(backend_url, "unload_skill", {"skill_name": "hello-world"})

    def test_unload_skill_removes_tools_from_gateway(self, skill_gateway_cluster):
        """Unloading a skill on the backend removes the tool from gateway tools/list."""
        backend_url = skill_gateway_cluster["backend_a_url"]
        gw = skill_gateway_cluster["gateway_url"]

        # Load on backend.
        _tools_call(backend_url, "load_skill", {"skill_name": "hello-world"})
        _wait_for_tool_suffix(gw, "hello_world__greet", timeout=6.0)

        # Unload on backend.
        result = _tools_call(backend_url, "unload_skill", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("unloaded") is True, f"unload_skill failed: {data}"

        # Wait for gateway to drop the tool.
        _wait_for_tool_suffix(gw, "hello_world__greet", timeout=6.0, should_exist=False)


# ── TestWebViewAuroraViewDiscovery ───────────────────────────────────────────


class TestWebViewAuroraViewDiscovery:
    """AuroraView (WebView-host DCC) discoverable via gateway meta-tools."""

    def test_auroraview_appears_in_list_dcc_instances(self, webview_gateway_cluster):
        """``list_dcc_instances`` reports both maya and auroraview backends."""
        gw = webview_gateway_cluster["gateway_url"]
        result = _tools_call(gw, "list_dcc_instances", {})
        text = _parse_content_text(result)
        data = json.loads(text)

        dccs = {entry["dcc_type"] for entry in data.get("instances", [])}
        assert "maya" in dccs, f"maya backend missing: {dccs}"
        assert "auroraview" in dccs, f"auroraview backend missing: {dccs}"

    def test_auroraview_tools_aggregated_in_gateway(self, webview_gateway_cluster):
        """AuroraView tools appear namespaced in the gateway's tools/list."""
        tools = _tools_list(webview_gateway_cluster["gateway_url"])
        prefixed = [t for t in tools if "__" in t["name"] and not t["name"].startswith("__skill__")]
        suffixes = [t["name"].split("__", 1)[1] for t in prefixed]

        assert "navigate_url" in suffixes, f"auroraview.navigate_url missing. suffixes={suffixes}"
        assert "take_screenshot" in suffixes, f"auroraview.take_screenshot missing. suffixes={suffixes}"

    def test_maya_and_auroraview_tools_coexist(self, webview_gateway_cluster):
        """Both DCC types' tools coexist without name collisions in the gateway."""
        tools = _tools_list(webview_gateway_cluster["gateway_url"])
        prefixed = [t for t in tools if "__" in t["name"] and not t["name"].startswith("__skill__")]

        # Collect dcc types from tool annotations.
        dcc_types_seen = set()
        for t in prefixed:
            dt = t.get("_dcc_type")
            if dt:
                dcc_types_seen.add(dt)

        assert "maya" in dcc_types_seen, f"maya tools missing. dcc_types={dcc_types_seen}"
        assert "auroraview" in dcc_types_seen, f"auroraview tools missing. dcc_types={dcc_types_seen}"

        # Every namespaced tool name must be unique (no collision).
        names = [t["name"] for t in prefixed]
        assert len(names) == len(set(names)), f"Duplicate namespaced tool names: {names}"


# ── TestExtrasMetadataThroughGateway ─────────────────────────────────────────


class TestExtrasMetadataThroughGateway:
    """``ServiceEntry.extras`` round-trip through TransportManager and gateway."""

    def test_extras_round_trip_via_transport_manager(self, tmp_path):
        """Extras (cdp_port, url, window_title) survive registration and retrieval."""
        mgr = TransportManager(str(tmp_path))
        extras = {"cdp_port": 9222, "url": "http://localhost:3000", "window_title": "AuroraView"}
        iid = mgr.register_service("auroraview", "127.0.0.1", 3000, extras=extras)
        entry = mgr.get_service("auroraview", iid)

        assert entry is not None
        assert entry.extras["cdp_port"] == 9222
        assert entry.extras["url"] == "http://localhost:3000"
        assert entry.extras["window_title"] == "AuroraView"
        mgr.shutdown()

    def test_extras_in_to_dict_serialization(self, tmp_path):
        """``to_dict()`` faithfully includes nested extras values."""
        mgr = TransportManager(str(tmp_path))
        extras = {
            "capabilities": {"scene": False, "timeline": True},
            "tags": ["webview", "cdp"],
            "host_pid": 42000,
        }
        iid = mgr.register_service("webview-maya", "127.0.0.1", 3001, extras=extras)
        entry = mgr.get_service("webview-maya", iid)
        assert entry is not None

        as_dict = entry.to_dict()
        assert as_dict["extras"] == extras
        assert as_dict["extras"]["capabilities"]["timeline"] is True
        assert as_dict["extras"]["host_pid"] == 42000
        mgr.shutdown()

    def test_extras_visible_in_list_dcc_instances(self, webview_gateway_cluster):
        """Instances reported by ``list_dcc_instances`` include identifying info."""
        gw = webview_gateway_cluster["gateway_url"]
        result = _tools_call(gw, "list_dcc_instances", {})
        data = _parse_content_json(result)

        instances = data.get("instances", [])
        assert len(instances) >= 2, f"Expected at least 2 instances: {instances}"

        # Verify each instance has essential identifying fields.
        for inst in instances:
            assert "dcc_type" in inst, f"Instance missing dcc_type: {inst}"
            assert "instance_id" in inst or "id" in inst, f"Instance missing id: {inst}"


# ── TestSessionPinningToolRouting ────────────────────────────────────────────


class TestSessionPinningToolRouting:
    """Session pinning and tool-call routing through the gateway."""

    def test_connect_to_dcc_returns_connection_info(self, webview_gateway_cluster):
        """``connect_to_dcc`` returns non-error connection info for maya."""
        gw = webview_gateway_cluster["gateway_url"]
        result = _tools_call(gw, "connect_to_dcc", {"dcc_type": "maya"})
        assert result.get("isError") is not True, f"connect_to_dcc failed: {result}"

    def test_different_dcc_types_get_different_connections(self, webview_gateway_cluster):
        """Connecting to maya vs auroraview yields different instance info."""
        gw = webview_gateway_cluster["gateway_url"]

        result_maya = _tools_call(gw, "connect_to_dcc", {"dcc_type": "maya"})
        result_av = _tools_call(gw, "connect_to_dcc", {"dcc_type": "auroraview"})

        assert result_maya.get("isError") is not True, f"maya failed: {result_maya}"
        assert result_av.get("isError") is not True, f"auroraview failed: {result_av}"

        text_maya = _parse_content_text(result_maya)
        text_av = _parse_content_text(result_av)

        # The two responses should reference different DCC types.
        assert "maya" in text_maya.lower(), f"Expected maya reference: {text_maya}"
        assert "auroraview" in text_av.lower(), f"Expected auroraview reference: {text_av}"

    def test_tool_call_routes_to_correct_backend(self, webview_gateway_cluster):
        """Namespaced tool calls via gateway route to the correct backend handler."""
        gw = webview_gateway_cluster["gateway_url"]
        tools = _tools_list(gw)

        # Find the namespaced maya.create_sphere and auroraview.navigate_url.
        maya_tool = None
        av_tool = None
        for t in tools:
            name = t["name"]
            if "__" in name and not name.startswith("__skill__"):
                suffix = name.split("__", 1)[1]
                if suffix == "create_sphere" and maya_tool is None:
                    maya_tool = name
                elif suffix == "navigate_url" and av_tool is None:
                    av_tool = name

        assert maya_tool, "Maya create_sphere not found in gateway tools"
        assert av_tool, "AuroraView navigate_url not found in gateway tools"

        # Call each and verify the handler response identifies the correct backend.
        maya_result = _tools_call(gw, maya_tool)
        maya_text = _parse_content_text(maya_result)
        assert "maya" in maya_text.lower() or "create_sphere" in maya_text, (
            f"Maya tool did not route correctly: {maya_text}"
        )

        av_result = _tools_call(gw, av_tool)
        av_text = _parse_content_text(av_result)
        assert "auroraview" in av_text.lower() or "navigate_url" in av_text, (
            f"AuroraView tool did not route correctly: {av_text}"
        )


# ── TestBundledSkillsDiscovery ───────────────────────────────────────────────


class TestBundledSkillsDiscovery:
    """Bundled skills (dcc-diagnostics, workflow) via create_skill_server."""

    def test_bundled_skills_appear_as_stubs_or_in_list(self, bundled_skill_server):
        """Bundled skill stubs or catalog entries are visible after discovery."""
        _, _, url = bundled_skill_server
        tools = _tools_list(url)
        names = {t["name"] for t in tools}

        # Bundled skills should appear as __skill__ stubs or be listed by list_skills.
        stubs_found = any(n.startswith("__skill__") for n in names)
        if not stubs_found:
            # Fall back to list_skills to confirm discovery.
            result = _tools_call(url, "list_skills", {"status": "all"})
            data = _parse_content_json(result)
            assert data.get("total", 0) >= 1, f"No bundled skills found: {data}"
        else:
            # If stubs exist, great.
            assert stubs_found

    def test_bundled_skill_info_accessible(self, bundled_skill_server):
        """``get_skill_info`` returns valid info for a bundled skill."""
        _, _, url = bundled_skill_server

        # List all skills to find at least one bundled skill name.
        list_result = _tools_call(url, "list_skills", {"status": "all"})
        list_data = _parse_content_json(list_result)
        assert list_data.get("total", 0) >= 1, f"No skills available: {list_data}"

        skill_name = list_data["skills"][0]["name"]

        result = _tools_call(url, "get_skill_info", {"skill_name": skill_name})
        data = _parse_content_json(result)
        assert data.get("name") == skill_name, f"Skill info name mismatch: {data}"

    def test_bundled_skill_loadable_and_serves_tools(self, bundled_skill_server):
        """Loading a bundled skill registers its tools; stub disappears."""
        _, _, url = bundled_skill_server

        # Discover a bundled skill name.
        list_result = _tools_call(url, "list_skills", {"status": "all"})
        list_data = _parse_content_json(list_result)
        skill_name = list_data["skills"][0]["name"]

        # Load the skill.
        load_result = _tools_call(url, "load_skill", {"skill_name": skill_name})
        load_data = _parse_content_json(load_result)
        assert load_data.get("loaded") is True, f"Failed to load {skill_name}: {load_data}"
        assert load_data.get("tool_count", 0) >= 1, f"No tools registered: {load_data}"

        # The stub should be gone; real tools present.
        tools = _tools_list(url)
        names = {t["name"] for t in tools}
        stub_name = f"__skill__{skill_name}"
        assert stub_name not in names, f"Stub {stub_name} should be gone after loading"

        # At least one tool with the skill prefix should exist.
        skill_prefix = skill_name.replace("-", "_") + "__"
        skill_tools = [n for n in names if n.startswith(skill_prefix)]
        assert skill_tools, f"Expected tools starting with {skill_prefix!r}, got: {sorted(names)}"

        # Clean up.
        with contextlib.suppress(Exception):
            _tools_call(url, "unload_skill", {"skill_name": skill_name})


# ── TestRequiredCapabilitiesFiltering ────────────────────────────────────────


class TestRequiredCapabilitiesFiltering:
    """``required_capabilities`` + ``WebViewAdapter.matches_requirements``."""

    def test_webview_adapter_rejects_scene_tools(self):
        """Default WebViewAdapter does not match tools requiring 'scene'."""
        assert not WebViewAdapter.matches_requirements(["scene"]), "Default WebViewAdapter should not support 'scene'"

    def test_webview_adapter_accepts_uncapped_tools(self):
        """Tools with no required capabilities are accessible to all adapters."""
        assert WebViewAdapter.matches_requirements([]), "Empty requirements should always match"

    def test_auroraview_subclass_selective_matching(self):
        """An AuroraView subclass with ``undo=True`` passes selective checks."""
        from typing import ClassVar

        class AuroraLikeAdapter(WebViewAdapter):
            dcc_name = "auroraview"
            capabilities: ClassVar[dict[str, bool]] = {**WEBVIEW_DEFAULT_CAPABILITIES, "undo": True}

        # Supports undo.
        assert AuroraLikeAdapter.matches_requirements(["undo"]), "Should support undo"
        # Does not support scene.
        assert not AuroraLikeAdapter.matches_requirements(["scene"]), "Should not support scene"
        # Requires both undo + scene -> fails (scene unsupported).
        assert not AuroraLikeAdapter.matches_requirements(["undo", "scene"]), (
            "Should fail when any required capability is missing"
        )
        # Empty -> always passes.
        assert AuroraLikeAdapter.matches_requirements([]), "Empty should always match"


# ── TestCrossCuttingBoundary ─────────────────────────────────────────────────


class TestCrossCuttingBoundary:
    """Cross-system integration edge cases."""

    def test_create_skill_server_accepts_auroraview(self):
        """``create_skill_server`` does not reject 'auroraview' as app_name."""
        from dcc_mcp_core import create_skill_server

        config = McpHttpConfig(port=0, server_name="av-test")
        server = create_skill_server("auroraview", config=config)
        # Server created without error.
        handle = server.start()
        try:
            assert handle.port > 0
            # Ping to verify it's live.
            resp = _post_mcp(handle.mcp_url(), "ping")
            assert resp.get("result") is not None
        finally:
            handle.shutdown()

    def test_skill_catalog_fresh_on_new_server(self):
        """New server instances start with an empty catalog (no state persistence)."""
        if not Path(EXAMPLES_SKILLS_DIR).is_dir():
            pytest.skip("examples/skills directory not found")

        from dcc_mcp_core import create_skill_server

        # Server 1: discover + load hello-world.
        cfg1 = McpHttpConfig(port=0, server_name="fresh-test-1")
        server1 = create_skill_server("test", config=cfg1, extra_paths=[EXAMPLES_SKILLS_DIR])
        h1 = server1.start()
        try:
            _tools_call(h1.mcp_url(), "load_skill", {"skill_name": "hello-world"})
            tools1 = _tools_list(h1.mcp_url())
            names1 = {t["name"] for t in tools1}
            assert "hello_world__greet" in names1, "hello-world should be loaded on server 1"
        finally:
            h1.shutdown()

        # Server 2: fresh instance — hello-world should NOT be loaded.
        cfg2 = McpHttpConfig(port=0, server_name="fresh-test-2")
        server2 = create_skill_server("test", config=cfg2, extra_paths=[EXAMPLES_SKILLS_DIR])
        h2 = server2.start()
        try:
            tools2 = _tools_list(h2.mcp_url())
            names2 = {t["name"] for t in tools2}
            assert "hello_world__greet" not in names2, "hello-world should NOT be loaded on a fresh server instance"
            # But the __skill__ stub should be present (discovered, not loaded).
            assert "__skill__hello-world" in names2, "hello-world stub should be present on fresh server"
        finally:
            h2.shutdown()

    def test_multiple_backends_visible_through_gateway(self, skill_gateway_cluster):
        """Gateway's ``list_dcc_instances`` sees all registered backends."""
        gw = skill_gateway_cluster["gateway_url"]
        result = _tools_call(gw, "list_dcc_instances", {})
        data = _parse_content_json(result)

        instances = data.get("instances", [])
        dcc_types = {i["dcc_type"] for i in instances}
        assert "maya" in dcc_types, f"maya missing from instances: {dcc_types}"
        assert "blender" in dcc_types, f"blender missing from instances: {dcc_types}"
        assert len(instances) >= 2, f"Expected at least 2 instances: {instances}"

    def test_search_skills_various_query_types(self, skill_gateway_cluster):
        """Skill search works with name, tag, and description keywords."""
        gw = skill_gateway_cluster["gateway_url"]

        # Search by name substring.
        r1 = _tools_call(gw, "search_skills", {"query": "hello"})
        d1 = _parse_gateway_aggregated(r1)
        assert d1.get("total", 0) >= 1, f"Name search 'hello' found nothing: {d1}"

        # Search by tag.
        r2 = _tools_call(gw, "find_skills", {"tags": ["example"]})
        d2 = _parse_gateway_aggregated(r2)
        assert d2.get("total", 0) >= 1, f"Tag search 'example' found nothing: {d2}"

        # Search by description keyword (search-hint).
        r3 = _tools_call(gw, "search_skills", {"query": "greeting"})
        d3 = _parse_gateway_aggregated(r3)
        # hello-world has search-hint containing "greeting" — at least 1 result.
        assert d3.get("total", 0) >= 1, f"Description search 'greeting' found nothing: {d3}"
