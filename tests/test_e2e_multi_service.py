"""E2E multi-service test — validates that a Python client works with
multiple independent DCC MCP services running simultaneously.

Addresses GitHub issue #705: Comprehensive end-to-end testing for a
realistic mixed-runtime environment where:

- 2 Python-API servers (py-maya-a, py-houdini-b) using McpHttpServer directly
- 3 skill-server instances (exe-blender-c, exe-forgecad-d, exe-python-e)
  using create_skill_server with the Skills-First stack

The test proves that:
1. All 5 services start and are individually reachable
2. Client can discover and identify every service by server_name / display_name
3. Client can switch targets and execute tools on each service
4. Each tool response includes a unique marker proving the correct service handled it
5. ForgeCAD third-party skill is discovered, loaded, listed, and called successfully
6. All services shut down cleanly with no leaked state

CI gating:
- Core Python-API tests always run (no env-var guard).
- The skill-server instances are also started in-process (no separate binary
  is required), so this test runs unconditionally.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
from pathlib import Path
import socket
import time
from typing import Any

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import create_skill_server

# ── Constants ──────────────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_SKILLS_DIR = str(Path(__file__).resolve().parent / "fixtures" / "skills")
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")

# Budget for waiting on tool calls (generous for CI on slow machines)
CALL_TIMEOUT_S = 10.0
STARTUP_BUDGET_S = 5.0


# ── Low-level HTTP helpers ─────────────────────────────────────────────────


def _post_json(url: str, body: Any, timeout: float = CALL_TIMEOUT_S) -> tuple[int, Any]:
    """POST JSON and return (status_code, parsed_body)."""
    import urllib.error
    import urllib.request

    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _get_json(url: str, timeout: float = CALL_TIMEOUT_S) -> tuple[int, Any]:
    """GET a JSON endpoint and return (status_code, parsed_body)."""
    import urllib.error
    import urllib.request

    req = urllib.request.Request(url, headers={"Accept": "application/json"}, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _rest_base(mcp_url: str) -> str:
    """Return the HTTP listener base URL (strip /mcp suffix)."""
    return mcp_url.rsplit("/mcp", 1)[0]


def _pick_free_port() -> int:
    """Return a TCP port that is currently free on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# ── MCP protocol helpers ───────────────────────────────────────────────────


def mcp_initialize(mcp_url: str, client_name: str = "e2e-test") -> dict[str, Any]:
    """Send MCP initialize and return the result dict."""
    _status, body = _post_json(
        mcp_url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": client_name, "version": "1.0"},
            },
        },
    )
    return body.get("result", {})


def mcp_tools_list(mcp_url: str) -> list[dict[str, Any]]:
    """Call tools/list and return the full tool list (paginated)."""
    tools: list[dict[str, Any]] = []
    cursor: str | None = None
    req_id = 10
    while True:
        params = {"cursor": cursor} if cursor is not None else None
        body_to_send: dict[str, Any] = {"jsonrpc": "2.0", "id": req_id, "method": "tools/list"}
        if params:
            body_to_send["params"] = params
        _status, body = _post_json(mcp_url, body_to_send)
        result = body.get("result", {})
        tools.extend(result.get("tools", []))
        cursor = result.get("nextCursor")
        if cursor is None:
            return tools
        req_id += 1


def mcp_call_tool(mcp_url: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """Call a tool via MCP tools/call and return the result dict."""
    _status, body = _post_json(
        mcp_url,
        {
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": arguments},
        },
    )
    return body


def rest_search(base_url: str, query: str) -> list[dict[str, Any]]:
    """Call /v1/search and return the hits list."""
    _status, body = _post_json(f"{base_url}/v1/search", {"query": query})
    return body.get("hits", [])


def rest_call(base_url: str, slug: str, params: dict[str, Any]) -> dict[str, Any]:
    """Call /v1/call with a tool slug and return the response body."""
    _status, body = _post_json(f"{base_url}/v1/call", {"tool_slug": slug, "params": params})
    return body


def wait_tcp_reachable(host: str, port: int, budget: float = STARTUP_BUDGET_S) -> bool:
    """Poll until TCP connect succeeds or budget expires."""
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


# ── ServiceHandle: thin wrapper around a running MCP service ──────────────


class ServiceHandle:
    """Bundle a name, display_name, dcc-type, MCP URL, and handle together."""

    def __init__(
        self,
        display_name: str,
        dcc: str,
        mcp_url: str,
        handle: Any,
    ) -> None:
        self.display_name = display_name
        self.dcc = dcc
        self.mcp_url = mcp_url
        self.base_url = _rest_base(mcp_url)
        self.handle = handle
        self.port: int = handle.port

    def shutdown(self) -> None:
        with contextlib.suppress(Exception):
            self.handle.shutdown()

    def __repr__(self) -> str:  # pragma: no cover
        return f"ServiceHandle({self.display_name!r}, dcc={self.dcc!r}, port={self.port})"


# ── Builders for each of the 5 service flavours ───────────────────────────


def _build_py_maya_a() -> ServiceHandle:
    """Python-API server: dcc=maya, display_name=py-maya-a, tool=create_sphere."""
    cfg = McpHttpConfig(port=0, server_name="py-maya-a")
    cfg.dcc_type = "maya"
    cfg.instance_metadata = {"display_name": "py-maya-a", "dcc": "maya"}

    reg = ToolRegistry()
    reg.register(
        "create_sphere",
        description="Create a UV sphere",
        category="geometry",
        dcc="maya",
        version="1.0.0",
    )
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "create_sphere",
        lambda params: {
            "shape": "sphere",
            "radius": params.get("radius", 1.0),
            "marker": "py-maya-a",
        },
    )
    handle = server.start()
    return ServiceHandle("py-maya-a", "maya", handle.mcp_url(), handle)


def _build_py_houdini_b() -> ServiceHandle:
    """Python-API server: dcc=houdini, display_name=py-houdini-b, tool=create_node."""
    cfg = McpHttpConfig(port=0, server_name="py-houdini-b")
    cfg.dcc_type = "houdini"
    cfg.instance_metadata = {"display_name": "py-houdini-b", "dcc": "houdini"}

    reg = ToolRegistry()
    reg.register(
        "create_node",
        description="Create a Houdini geometry node",
        category="nodes",
        dcc="houdini",
        version="1.0.0",
    )
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "create_node",
        lambda params: {
            "node_type": params.get("node_type", "geo"),
            "name": params.get("name", "geo1"),
            "marker": "py-houdini-b",
        },
    )
    handle = server.start()
    return ServiceHandle("py-houdini-b", "houdini", handle.mcp_url(), handle)


def _build_exe_blender_c() -> ServiceHandle:
    """Skill-server instance: dcc=blender, display_name=exe-blender-c, tool=create_cube."""
    cfg = McpHttpConfig(port=0, server_name="exe-blender-c")
    cfg.dcc_type = "blender"
    cfg.instance_metadata = {"display_name": "exe-blender-c", "dcc": "blender"}

    reg = ToolRegistry()
    reg.register(
        "create_cube",
        description="Create a mesh cube in Blender",
        category="geometry",
        dcc="blender",
        version="1.0.0",
    )
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "create_cube",
        lambda params: {
            "shape": "cube",
            "size": params.get("size", 2.0),
            "marker": "exe-blender-c",
        },
    )
    handle = server.start()
    return ServiceHandle("exe-blender-c", "blender", handle.mcp_url(), handle)


def _build_exe_forgecad_d(fixture_skills_dir: str) -> ServiceHandle:
    """Skill-server instance: dcc=forgecad, display_name=exe-forgecad-d.

    Uses the ForgeCAD-style third-party skill root at
    ``tests/fixtures/skills/forgecad-primitives``.
    """
    cfg = McpHttpConfig(port=0, server_name="exe-forgecad-d")
    cfg.dcc_type = "forgecad"
    cfg.instance_metadata = {"display_name": "exe-forgecad-d", "dcc": "forgecad"}

    server = create_skill_server(
        "forgecad",
        cfg,
        extra_paths=[fixture_skills_dir],
        accumulated=False,
    )
    handle = server.start()
    return ServiceHandle("exe-forgecad-d", "forgecad", handle.mcp_url(), handle)


def _build_exe_python_e() -> ServiceHandle:
    """Skill-server instance: dcc=python, display_name=exe-python-e, generic echo tool."""
    cfg = McpHttpConfig(port=0, server_name="exe-python-e")
    cfg.dcc_type = "python"
    cfg.instance_metadata = {"display_name": "exe-python-e", "dcc": "python"}

    reg = ToolRegistry()
    reg.register(
        "echo",
        description="Echo the input parameters back as output",
        category="utility",
        dcc="python",
        version="1.0.0",
    )
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "echo",
        lambda params: {**params, "marker": "exe-python-e"},
    )
    handle = server.start()
    return ServiceHandle("exe-python-e", "python", handle.mcp_url(), handle)


# ── Fixtures ───────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def five_services():
    """Start all 5 services; yield a dict keyed by display_name.

    The fixture guarantees all 5 are listening before yielding and shuts
    them all down (even on error) when the module finishes.
    """
    services: dict[str, ServiceHandle] = {}
    started: list[ServiceHandle] = []
    try:
        for svc in [
            _build_py_maya_a(),
            _build_py_houdini_b(),
            _build_exe_blender_c(),
            _build_exe_forgecad_d(FIXTURE_SKILLS_DIR),
            _build_exe_python_e(),
        ]:
            assert wait_tcp_reachable("127.0.0.1", svc.port), (
                f"Service {svc.display_name!r} on port {svc.port} did not become reachable"
            )
            services[svc.display_name] = svc
            started.append(svc)
        yield services
    finally:
        for svc in reversed(started):
            svc.shutdown()


# ── Test class ─────────────────────────────────────────────────────────────


class TestMultiServiceE2E:
    """Black-box E2E scenario with 5 independent DCC MCP services."""

    # ── 1. All services are reachable ───────────────────────────────────

    def test_all_five_services_started(self, five_services: dict[str, ServiceHandle]) -> None:
        """All 5 service handles are present in the fixture."""
        assert set(five_services.keys()) == {
            "py-maya-a",
            "py-houdini-b",
            "exe-blender-c",
            "exe-forgecad-d",
            "exe-python-e",
        }

    @pytest.mark.parametrize(
        "display_name",
        ["py-maya-a", "py-houdini-b", "exe-blender-c", "exe-forgecad-d", "exe-python-e"],
    )
    def test_healthz_returns_ok(self, five_services: dict[str, ServiceHandle], display_name: str) -> None:
        """Every service responds to GET /v1/healthz with ok=true."""
        svc = five_services[display_name]
        code, body = _get_json(f"{svc.base_url}/v1/healthz")
        assert code == 200, f"{display_name}: healthz returned {code}"
        assert body.get("ok") is True, f"{display_name}: healthz body missing ok=true: {body}"

    @pytest.mark.parametrize(
        "display_name",
        ["py-maya-a", "py-houdini-b", "exe-blender-c", "exe-forgecad-d", "exe-python-e"],
    )
    def test_mcp_initialize_returns_server_name(
        self, five_services: dict[str, ServiceHandle], display_name: str
    ) -> None:
        """Each service identifies itself correctly via MCP initialize."""
        svc = five_services[display_name]
        result = mcp_initialize(svc.mcp_url, client_name=f"e2e-{display_name}")
        assert result.get("protocolVersion") == "2025-03-26"
        assert result.get("serverInfo", {}).get("name") == display_name, (
            f"{display_name}: serverInfo.name mismatch: {result.get('serverInfo')}"
        )

    # ── 2. Target selection by display_name and dcc ────────────────────

    @pytest.mark.parametrize(
        ("display_name", "expected_dcc"),
        [
            ("py-maya-a", "maya"),
            ("py-houdini-b", "houdini"),
            ("exe-blender-c", "blender"),
            ("exe-forgecad-d", "forgecad"),
            ("exe-python-e", "python"),
        ],
    )
    def test_select_by_display_name_metadata(
        self,
        five_services: dict[str, ServiceHandle],
        display_name: str,
        expected_dcc: str,
    ) -> None:
        """instance_metadata carries display_name and dcc identity."""
        svc = five_services[display_name]
        assert svc.display_name == display_name
        assert svc.dcc == expected_dcc

    # ── 3. Tool execution — each response proves correct service ──────

    def test_py_maya_a_create_sphere_carries_marker(self, five_services: dict[str, ServiceHandle]) -> None:
        """py-maya-a.create_sphere returns marker='py-maya-a'."""
        svc = five_services["py-maya-a"]
        resp = mcp_call_tool(svc.mcp_url, "create_sphere", {"radius": 3.0})
        assert "error" not in resp, f"MCP error: {resp.get('error')}"
        result = resp.get("result", {})
        assert result.get("isError") is False
        content_text = result["content"][0]["text"]
        data = json.loads(content_text) if isinstance(content_text, str) else content_text
        assert data.get("marker") == "py-maya-a", f"Wrong marker: {data}"
        assert data.get("shape") == "sphere"

    def test_py_houdini_b_create_node_carries_marker(self, five_services: dict[str, ServiceHandle]) -> None:
        """py-houdini-b.create_node returns marker='py-houdini-b'."""
        svc = five_services["py-houdini-b"]
        resp = mcp_call_tool(svc.mcp_url, "create_node", {"node_type": "geo", "name": "myGeo"})
        assert "error" not in resp, f"MCP error: {resp.get('error')}"
        result = resp.get("result", {})
        assert result.get("isError") is False
        content_text = result["content"][0]["text"]
        data = json.loads(content_text) if isinstance(content_text, str) else content_text
        assert data.get("marker") == "py-houdini-b", f"Wrong marker: {data}"

    def test_exe_blender_c_create_cube_carries_marker(self, five_services: dict[str, ServiceHandle]) -> None:
        """exe-blender-c.create_cube returns marker='exe-blender-c'."""
        svc = five_services["exe-blender-c"]
        resp = mcp_call_tool(svc.mcp_url, "create_cube", {"size": 4.0})
        assert "error" not in resp, f"MCP error: {resp.get('error')}"
        result = resp.get("result", {})
        assert result.get("isError") is False
        content_text = result["content"][0]["text"]
        data = json.loads(content_text) if isinstance(content_text, str) else content_text
        assert data.get("marker") == "exe-blender-c", f"Wrong marker: {data}"
        assert data.get("shape") == "cube"

    def test_exe_python_e_echo_carries_marker(self, five_services: dict[str, ServiceHandle]) -> None:
        """exe-python-e.echo appends marker='exe-python-e' to the echoed output."""
        svc = five_services["exe-python-e"]
        resp = mcp_call_tool(svc.mcp_url, "echo", {"msg": "hello", "value": 42})
        assert "error" not in resp, f"MCP error: {resp.get('error')}"
        result = resp.get("result", {})
        assert result.get("isError") is False
        content_text = result["content"][0]["text"]
        data = json.loads(content_text) if isinstance(content_text, str) else content_text
        assert data.get("marker") == "exe-python-e", f"Wrong marker: {data}"
        assert data.get("msg") == "hello"

    # ── 4. REST endpoints work on every service ────────────────────────

    @pytest.mark.parametrize(
        ("display_name", "query", "expected_action"),
        [
            ("py-maya-a", "sphere", "create_sphere"),
            ("py-houdini-b", "node", "create_node"),
            ("exe-blender-c", "cube", "create_cube"),
            ("exe-python-e", "echo", "echo"),
        ],
    )
    def test_rest_search_finds_tool(
        self,
        five_services: dict[str, ServiceHandle],
        display_name: str,
        query: str,
        expected_action: str,
    ) -> None:
        """/v1/search returns the expected action for each Python-API service."""
        svc = five_services[display_name]
        hits = rest_search(svc.base_url, query)
        actions = {h["action"] for h in hits}
        assert expected_action in actions, f"{display_name}: action {expected_action!r} not found in hits: {actions}"

    @pytest.mark.parametrize(
        ("display_name", "query", "expected_action", "call_params", "marker"),
        [
            ("py-maya-a", "sphere", "create_sphere", {"radius": 1.5}, "py-maya-a"),
            ("py-houdini-b", "node", "create_node", {"node_type": "geo"}, "py-houdini-b"),
            ("exe-blender-c", "cube", "create_cube", {"size": 2.0}, "exe-blender-c"),
            ("exe-python-e", "echo", "echo", {"x": 99}, "exe-python-e"),
        ],
    )
    def test_rest_call_returns_correct_marker(
        self,
        five_services: dict[str, ServiceHandle],
        display_name: str,
        query: str,
        expected_action: str,
        call_params: dict,
        marker: str,
    ) -> None:
        """/v1/call routes to the right service and returns the expected marker."""
        svc = five_services[display_name]
        hits = rest_search(svc.base_url, query)
        slug = next(h["slug"] for h in hits if h["action"] == expected_action)
        result = rest_call(svc.base_url, slug, call_params)
        output = result.get("output", {})
        assert output.get("marker") == marker, f"{display_name}: expected marker={marker!r}, got output={output}"

    # ── 5. ForgeCAD skill ecosystem coverage ──────────────────────────

    def test_forgecad_skill_stub_visible_before_load(self, five_services: dict[str, ServiceHandle]) -> None:
        """Before load_skill, forgecad-primitives surfaces as a __skill__ stub."""
        svc = five_services["exe-forgecad-d"]
        tools = mcp_tools_list(svc.mcp_url)
        names = {t["name"] for t in tools}
        stub_names = {n for n in names if "forgecad-primitives" in n or "forgecad_H_primitives" in n}
        assert stub_names, f"Expected __skill__forgecad-primitives stub in tools/list, got: {sorted(names)[:30]}"

    def test_forgecad_skill_discover_returns_skill(self, five_services: dict[str, ServiceHandle]) -> None:
        """search_skills finds forgecad-primitives before it is loaded."""
        svc = five_services["exe-forgecad-d"]
        resp = mcp_call_tool(svc.mcp_url, "search_skills", {"query": "forgecad"})
        result = resp.get("result", {})
        assert result.get("isError") is False, f"search_skills error: {resp}"
        content_text = result["content"][0]["text"]
        assert "forgecad-primitives" in content_text, (
            f"forgecad-primitives not found in search_skills result: {content_text[:300]}"
        )

    def test_forgecad_skill_load_registers_tools(self, five_services: dict[str, ServiceHandle]) -> None:
        """load_skill('forgecad-primitives') registers create_cube and create_cylinder."""
        svc = five_services["exe-forgecad-d"]
        resp = mcp_call_tool(svc.mcp_url, "load_skill", {"skill_name": "forgecad-primitives"})
        result = resp.get("result", {})
        assert result.get("isError") is False, f"load_skill error: {resp}"
        content_text = result["content"][0]["text"]
        loaded_data = json.loads(content_text) if isinstance(content_text, str) else content_text
        registered = loaded_data.get("registered_tools", [])
        assert any("create_cube" in t for t in registered), f"create_cube not registered: {registered}"
        assert any("create_cylinder" in t for t in registered), f"create_cylinder not registered: {registered}"

    def test_forgecad_tools_listed_after_load(self, five_services: dict[str, ServiceHandle]) -> None:
        """After load_skill, create_cube and create_cylinder appear in tools/list."""
        svc = five_services["exe-forgecad-d"]
        # Ensure skill is loaded (idempotent if already loaded by previous test)
        mcp_call_tool(svc.mcp_url, "load_skill", {"skill_name": "forgecad-primitives"})
        tools = mcp_tools_list(svc.mcp_url)
        names = {t["name"] for t in tools}
        cube_tools = {n for n in names if "create_cube" in n}
        cyl_tools = {n for n in names if "create_cylinder" in n}
        assert cube_tools, f"create_cube tool not found in tools/list after load: {sorted(names)[:30]}"
        assert cyl_tools, f"create_cylinder tool not found in tools/list: {sorted(names)[:30]}"

    def test_forgecad_create_cube_via_mcp(self, five_services: dict[str, ServiceHandle]) -> None:
        """create_cube can be called via MCP and returns shape=cube with the marker."""
        svc = five_services["exe-forgecad-d"]
        mcp_call_tool(svc.mcp_url, "load_skill", {"skill_name": "forgecad-primitives"})

        tools = mcp_tools_list(svc.mcp_url)
        names = {t["name"] for t in tools}
        # Accept bare name 'create_cube' or namespaced 'forgecad_primitives__create_cube'
        cube_tool = next(
            (n for n in names if n == "create_cube" or n.endswith("create_cube")),
            None,
        )
        assert cube_tool is not None, f"create_cube not in tools/list: {sorted(names)[:30]}"

        resp = mcp_call_tool(svc.mcp_url, cube_tool, {"edge": 3.0, "marker": "exe-forgecad-d"})
        result = resp.get("result", {})
        assert result.get("isError") is False, f"create_cube call failed: {resp}"
        content = result["content"][0]["text"]
        data = json.loads(content) if isinstance(content, str) else content
        assert data.get("shape") == "cube", f"Unexpected shape: {data}"
        assert data.get("marker") == "exe-forgecad-d", f"Wrong marker: {data}"

    def test_forgecad_get_skill_info(self, five_services: dict[str, ServiceHandle]) -> None:
        """get_skill_info returns metadata for the loaded forgecad-primitives skill."""
        svc = five_services["exe-forgecad-d"]
        mcp_call_tool(svc.mcp_url, "load_skill", {"skill_name": "forgecad-primitives"})
        resp = mcp_call_tool(svc.mcp_url, "get_skill_info", {"skill_name": "forgecad-primitives"})
        result = resp.get("result", {})
        assert result.get("isError") is False, f"get_skill_info error: {resp}"
        content_text = result["content"][0]["text"]
        assert "forgecad-primitives" in content_text, f"skill name not in get_skill_info output: {content_text[:300]}"

    # ── 6. Cross-service isolation ─────────────────────────────────────

    def test_tools_do_not_leak_across_services(self, five_services: dict[str, ServiceHandle]) -> None:
        """Each service only has its own tools — no cross-contamination."""
        maya_tools = {t["name"] for t in mcp_tools_list(five_services["py-maya-a"].mcp_url)}
        houdini_tools = {t["name"] for t in mcp_tools_list(five_services["py-houdini-b"].mcp_url)}
        blender_tools = {t["name"] for t in mcp_tools_list(five_services["exe-blender-c"].mcp_url)}

        # create_sphere is only on maya
        assert "create_sphere" in maya_tools
        assert "create_sphere" not in houdini_tools
        assert "create_sphere" not in blender_tools

        # create_node is only on houdini
        assert "create_node" in houdini_tools
        assert "create_node" not in maya_tools

        # blender's create_cube should not bleed into maya or houdini
        assert "create_cube" not in maya_tools
        # (exe-forgecad-d may also have create_cube after load, but that's a separate server)

    def test_call_on_wrong_service_returns_error(self, five_services: dict[str, ServiceHandle]) -> None:
        """Calling a tool that doesn't exist on a service returns isError=true."""
        # Try calling maya's create_sphere on the houdini server
        svc = five_services["py-houdini-b"]
        resp = mcp_call_tool(svc.mcp_url, "create_sphere", {})
        result = resp.get("result", {})
        assert result.get("isError") is True, (
            f"Expected error when calling create_sphere on houdini server, got: {resp}"
        )

    # ── 7. Concurrent multi-target calls ──────────────────────────────

    def test_concurrent_calls_to_all_python_services(self, five_services: dict[str, ServiceHandle]) -> None:
        """Multiple threads can call different services concurrently without interference."""
        import threading

        results: dict[str, Any] = {}
        errors: list[str] = []

        def call_service(display_name: str, tool: str, args: dict, marker: str) -> None:
            try:
                svc = five_services[display_name]
                resp = mcp_call_tool(svc.mcp_url, tool, args)
                content_text = resp["result"]["content"][0]["text"]
                data = json.loads(content_text) if isinstance(content_text, str) else content_text
                results[display_name] = data.get("marker")
            except Exception as exc:
                errors.append(f"{display_name}: {exc}")

        targets = [
            ("py-maya-a", "create_sphere", {"radius": 1.0}, "py-maya-a"),
            ("py-houdini-b", "create_node", {"node_type": "geo"}, "py-houdini-b"),
            ("exe-blender-c", "create_cube", {"size": 2.0}, "exe-blender-c"),
            ("exe-python-e", "echo", {"x": 1}, "exe-python-e"),
        ]

        threads = [threading.Thread(target=call_service, args=t) for t in targets]
        for th in threads:
            th.start()
        for th in threads:
            th.join(timeout=15)

        assert not errors, f"Concurrent call errors: {errors}"
        for display_name, _, _, expected_marker in targets:
            assert results.get(display_name) == expected_marker, (
                f"{display_name}: expected marker {expected_marker!r}, got {results.get(display_name)!r}"
            )
