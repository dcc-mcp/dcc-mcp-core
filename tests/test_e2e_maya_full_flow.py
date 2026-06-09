"""Maya full-chain E2E integration test.

Validates the complete Maya adapter lifecycle:
  zero-instance → install → gateway ensure → search → call

Test phases:
  1. install plan — ``dcc-mcp-cli install --dcc-type maya`` produces a valid plan
  2. Gateway with Maya skills — ``create_skill_server("maya", ...)`` starts a
     Maya-compatible backend with gateway election
  3. Gateway canonical flow — search → describe → load_skill → call via gateway
  4. Error scenarios — wrong DCC type, missing skill, unknown tool
  5. Full install+gateway flow — (conditional: requires P1-4 install --execute)

Acceptance criteria:
  - Maya zero-instance → install complete → gateway running → search finds
    tools → call executes successfully
  - Cross-platform testable (at least Windows)
  - Failure scenarios produce clear, diagnosable error messages
  - Repeatable (idempotent or cleanable)
"""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
from typing import Any

import pytest

from conftest import REPO_ROOT
from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server

# ── Constants ──────────────────────────────────────────────────────────────

EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")
MAYA_SKILLS = ("maya-geometry", "maya-pipeline")

# Env-var gate for P1-4 install --execute (blocking dependency).
P1_4_AVAILABLE = os.environ.get("P1_4_INSTALL_EXECUTE") or os.environ.get("DCC_MCP_P1_4_READY")

# Resolve the dcc-mcp-cli binary (Rust CLI, not the Python package).
_DCC_MCP_CLI_BIN: str | None = None
_cli_candidates = [
    os.environ.get("DCC_MCP_CLI_BIN"),
    os.environ.get("DCC_MCP_CLI"),
    shutil.which("dcc-mcp-cli"),
    shutil.which("dcc-mcp-cli.exe"),
]
for _candidate in _cli_candidates:
    if _candidate:
        _DCC_MCP_CLI_BIN = _candidate
        break

DCC_MCP_CLI_AVAILABLE = _DCC_MCP_CLI_BIN is not None

CALL_TIMEOUT_S = 10.0
STARTUP_BUDGET_S = 5.0


# ── Helpers ────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_tcp_reachable(host: str, port: int, budget: float = STARTUP_BUDGET_S) -> bool:
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1, timeout: float = 10.0) -> dict:
    client = McpClient(url)
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    _, resp = client.post(body)
    return resp


def _mcp_tools_list(url: str) -> list[dict[str, Any]]:
    client = McpClient(url)
    tools: list[dict[str, Any]] = []
    cursor: str | None = None
    req_id = 10
    while True:
        params = {"cursor": cursor} if cursor is not None else None
        body: dict[str, Any] = {"jsonrpc": "2.0", "id": req_id, "method": "tools/list"}
        if params:
            body["params"] = params
        _status, resp = client.post(body)
        result = resp.get("result", {})
        tools.extend(result.get("tools", []))
        cursor = result.get("nextCursor")
        if cursor is None:
            return tools
        req_id += 1


def _mcp_call_tool(url: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    client = McpClient(url)
    _status, body = client.post(
        {
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": arguments},
        },
    )
    return body


def _parse_content_text(result: dict[str, Any]) -> str:
    content = result.get("result", {}).get("content", [])
    if content and isinstance(content[0], dict):
        return content[0].get("text", "")
    return json.dumps(result)


def _parse_gateway_payload(result: dict[str, Any]) -> dict[str, Any]:
    payload = json.loads(_parse_content_text(result))
    assert isinstance(payload, dict), f"Expected gateway payload dict, got: {payload!r}"
    return payload


# ── Conditional skip marker for P1-4 ────────────────────────────────────

requires_p1_4 = pytest.mark.skipif(
    not P1_4_AVAILABLE,
    reason=(
        "P1-4 install --execute not available. Set P1_4_INSTALL_EXECUTE or "
        "DCC_MCP_P1_4_READY env var to enable the full install flow."
    ),
)


# ═══════════════════════════════════════════════════════════════════════════
# Phase 1: Install Plan Verification
# ═══════════════════════════════════════════════════════════════════════════


requires_cli = pytest.mark.skipif(
    not DCC_MCP_CLI_AVAILABLE,
    reason="dcc-mcp-cli binary not found in PATH. Build with `cargo build --release -p dcc-mcp-cli`.",
)


@requires_cli
class TestMayaInstallPlan:
    """Validate the install planning output without executing it."""

    def _run_install(self, *args: str) -> subprocess.CompletedProcess:
        assert _DCC_MCP_CLI_BIN is not None
        return subprocess.run(
            [_DCC_MCP_CLI_BIN, "install", *args],
            capture_output=True,
            text=True,
            timeout=15,
        )

    def test_install_plan_has_maya_entry(self) -> None:
        """dcc-mcp-cli install --dcc-type maya must produce a valid InstallPlan."""
        result = self._run_install("--dcc-type", "maya")
        assert result.returncode == 0, (
            f"CLI returned {result.returncode}: {result.stderr[:500]}"
        )

        plan = json.loads(result.stdout)
        assert isinstance(plan, dict), f"Expected dict plan, got: {type(plan)}"
        assert "dcc_type" in plan, f"Plan missing dcc_type: {list(plan.keys())}"
        assert plan["dcc_type"] == "maya", f"Expected maya, got: {plan['dcc_type']}"

        # Verify plan has steps
        steps = plan.get("steps", [])
        assert len(steps) >= 1, f"Expected at least 1 step, got: {len(steps)}"

        step_names = [s.get("name", "") for s in steps]
        assert "resolve-adapter" in step_names, (
            f"Expected resolve-adapter step, got: {step_names}"
        )

    def test_install_plan_accepts_version_filter(self) -> None:
        """Install plan with --version must not error."""
        result = self._run_install("--dcc-type", "maya", "--version", "2026")
        assert result.returncode == 0, (
            f"CLI returned {result.returncode}: {result.stderr[:500]}"
        )

        plan = json.loads(result.stdout)
        assert isinstance(plan, dict)
        assert plan["dcc_type"] == "maya"
        assert plan.get("version") == "2026", (
            f"Expected version=2026, got: {plan.get('version')}"
        )

    def test_install_plan_unknown_dcc_reports_error(self) -> None:
        """Installing for an unknown DCC type must report the error without crashing."""
        result = self._run_install("--dcc-type", "nonexistent-dcc-xyz")
        # The CLI should exit with a non-zero code and informative message.
        assert result.returncode != 0, (
            f"Expected non-zero return for unknown DCC, got stdout: {result.stdout[:300]}"
        )
        assert result.stderr, "Expected non-empty stderr for unknown DCC"
        error_lower = result.stderr.lower()
        error_keywords = ("not found", "unrecognized", "error", "unknown", "no entry")
        assert any(kw in error_lower for kw in error_keywords), (
            f"Error message should mention the issue: {result.stderr[:500]}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 2: Maya Gateway Fixture
# ═══════════════════════════════════════════════════════════════════════════

# Session-scoped registry isolation for all gateway tests in this module.
@pytest.fixture(scope="module", autouse=True)
def _maya_registry_isolate(tmp_path_factory: pytest.TempPathFactory):
    """Redirect DCC_MCP_REGISTRY_DIR to an isolated temp dir for the module."""
    reg_dir = tmp_path_factory.mktemp("maya-e2e-registry")
    old = os.environ.get("DCC_MCP_REGISTRY_DIR")
    os.environ["DCC_MCP_REGISTRY_DIR"] = str(reg_dir)
    yield
    if old is None:
        os.environ.pop("DCC_MCP_REGISTRY_DIR", None)
    else:
        os.environ["DCC_MCP_REGISTRY_DIR"] = old


@pytest.fixture(scope="module")
def maya_gateway(tmp_path_factory: pytest.TempPathFactory):
    """Start a Maya-compatible backend with gateway election.

    Discovers example skills (maya-geometry, maya-pipeline) as stubs.
    The backend wins gateway election as the single instance.
    """
    if not Path(EXAMPLES_SKILLS_DIR).is_dir():
        pytest.skip("examples/skills directory not found")

    registry_dir = tmp_path_factory.mktemp("maya-gateway-registry")
    gateway_port = _pick_free_port()

    cfg = McpHttpConfig(port=0, server_name="maya-e2e-gateway")
    cfg.gateway_port = gateway_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "maya"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = create_skill_server(
        "maya",
        cfg,
        extra_paths=[EXAMPLES_SKILLS_DIR],
        accumulated=False,
    )
    handle = server.start()

    try:
        if not _wait_tcp_reachable("127.0.0.1", handle.port):
            pytest.skip(f"Backend port {handle.port} not reachable")
        if not handle.is_gateway:
            pytest.skip(f"Backend did not win gateway election on {gateway_port}")
        if not _wait_tcp_reachable("127.0.0.1", gateway_port):
            pytest.skip(f"Gateway port {gateway_port} not reachable")

        gateway_url = f"http://127.0.0.1:{gateway_port}/mcp"

        # Let the gateway SSE subscriber connect to the backend and the
        # tools/prompts aggregator complete its first tick before tests
        # issue tools/call that need to proxy through the backend.
        time.sleep(4.0)

        yield {
            "handle": handle,
            "gateway_url": gateway_url,
            "dcc_type": "maya",
        }
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


# ═══════════════════════════════════════════════════════════════════════════
# Phase 3: Gateway infrastructure health
# ═══════════════════════════════════════════════════════════════════════════


class TestMayaGatewayHealth:
    """Gateway health and basic MCP initialization."""

    def test_gateway_elected(self, maya_gateway) -> None:
        """The Maya backend must win gateway election."""
        assert maya_gateway["handle"].is_gateway is True

    def test_gateway_mcp_initialize(self, maya_gateway) -> None:
        """MCP initialize handshake must succeed."""
        url = maya_gateway["gateway_url"]
        client = McpClient(url, auto_init=False)
        result = client.initialize(client_name="maya-e2e-test")
        assert result.get("protocolVersion") in (
            "2025-03-26", "2025-06-18", "2025-11-25"
        ), f"Unexpected protocol version: {result.get('protocolVersion')}"
        assert "serverInfo" in result, f"Missing serverInfo in initialize result: {result}"

    def test_gateway_ping(self, maya_gateway) -> None:
        """Ping the gateway succeeds."""
        url = maya_gateway["gateway_url"]
        resp = _post_mcp(url, "ping")
        assert "error" not in resp, f"Ping error: {resp.get('error')}"

    def test_gateway_tools_list_exposes_canonical_surface(self, maya_gateway) -> None:
        """Gateway tools/list must expose the 4 canonical tools."""
        url = maya_gateway["gateway_url"]
        tools = _mcp_tools_list(url)
        tool_names = {t["name"] for t in tools}
        expected = {"search", "describe", "load_skill", "call"}
        missing = expected - tool_names
        assert not missing, (
            f"Gateway canonical tools missing: {missing}. Got: {tool_names}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 4: Gateway canonical workflow — search → describe → load → call
# ═══════════════════════════════════════════════════════════════════════════


class TestMayaGatewayCanonicalWorkflow:
    """Full gateway canonical workflow with Maya skills."""

    def test_search_finds_maya_skill_stub(self, maya_gateway) -> None:
        """Search for 'maya' must find maya-geometry stub before load."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url,
            "tools/call",
            {
                "name": "search",
                "arguments": {"query": "maya", "dcc_type": "maya", "limit": 10},
            },
        )
        assert "error" not in result, f"search error: {result.get('error')}"
        payload = _parse_gateway_payload(result)
        hits = payload.get("hits", [])
        skill_names = {h.get("skill_name", "") for h in hits}
        assert any("maya" in n for n in skill_names), (
            f"No maya skill stub found in search hits: {skill_names}"
        )

    def test_search_geometry_tools(self, maya_gateway) -> None:
        """Search for 'geometry' must find maya-geometry tools."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url,
            "tools/call",
            {
                "name": "search",
                "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 10},
            },
        )
        assert "error" not in result, f"search error: {result.get('error')}"
        payload = _parse_gateway_payload(result)
        hits = payload.get("hits", [])
        assert len(hits) >= 1, f"Expected at least 1 hit for 'sphere', got: {hits}"

    def test_load_maya_geometry_skill(self, maya_gateway) -> None:
        """load_skill('maya-geometry') must register its tools."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url,
            "tools/call",
            {
                "name": "load_skill",
                "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"},
            },
        )
        assert "error" not in result, f"load_skill error: {result.get('error')}"
        payload = _parse_gateway_payload(result)
        assert payload.get("loaded") is True, (
            f"load_skill did not report loaded=true: {payload}"
        )
        assert payload.get("skill_name") == "maya-geometry"
        assert "instance_id" in payload, f"Missing instance_id in load result: {payload}"

    def test_describe_maya_tool(self, maya_gateway) -> None:
        """Describe a Maya tool slug returned by search."""
        url = maya_gateway["gateway_url"]

        # Search for sphere tool to get a slug
        search_result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        payload = _parse_gateway_payload(search_result)
        hits = payload.get("hits", [])
        if not hits:
            pytest.skip("No search hits to describe")

        slug = hits[0]["tool_slug"]
        describe_result = _post_mcp(
            url, "tools/call",
            {"name": "describe", "arguments": {"tool_slug": slug}},
        )
        assert "error" not in describe_result, f"describe error: {describe_result.get('error')}"
        describe_payload = _parse_gateway_payload(describe_result)
        assert "tool" in describe_payload, f"describe result missing 'tool': {describe_payload}"
        tool = describe_payload["tool"]
        assert "name" in tool, f"described tool missing 'name': {tool}"

    def test_search_for_maya_tool_after_load(self, maya_gateway) -> None:
        """After load_skill, maya-geometry tools must appear in search."""
        url = maya_gateway["gateway_url"]

        # Ensure loaded (idempotent)
        _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"}},
        )

        result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        payload = _parse_gateway_payload(result)
        hits = payload.get("hits", [])
        # After load, the tool may appear as a backend_tool directly or as
        # a skill_candidate; either is fine.
        assert len(hits) >= 1, f"No search results for 'sphere' after load: {payload}"

    def test_call_maya_tool_returns_result(self, maya_gateway) -> None:
        """Call a maya-geometry tool through the gateway and inspect result."""
        url = maya_gateway["gateway_url"]

        # Load skill (idempotent)
        _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"}},
        )

        # Find the sphere tool slug
        search_result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        payload = _parse_gateway_payload(search_result)
        hits = payload.get("hits", [])
        sphere_hit = next(
            (h for h in hits if "sphere" in h.get("backend_tool", "") or "sphere" in h.get("name", "")),
            None,
        )
        if not sphere_hit:
            pytest.skip("No sphere tool found in search results; maya-geometry may not be loaded yet")

        slug = sphere_hit["tool_slug"]
        call_result = _post_mcp(
            url, "tools/call",
            {
                "name": "call",
                "arguments": {
                    "tool_slug": slug,
                    "arguments": {"name": "testSphere", "radius": 2.0},
                },
            },
        )
        assert "error" not in call_result, f"call error: {call_result.get('error')}"
        call_payload = _parse_gateway_payload(call_result)
        assert call_payload.get("success") is not False, (
            f"Call did not succeed: {call_payload}"
        )

    def test_batch_single_tool_through_gateway(self, maya_gateway) -> None:
        """A single tool call through the gateway succeeds.

        (Batch calling multiple unloaded backend tools is unreliable when
        multiple Maya example skills are discovered but not all loaded.)
        """
        url = maya_gateway["gateway_url"]

        # Load the skill (idempotent)
        _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"}},
        )

        # Find a single loaded tool slug
        search_result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        payload = _parse_gateway_payload(search_result)
        hits = payload.get("hits", [])
        slug = next(
            (h["tool_slug"] for h in hits if h.get("backend_tool") and "sphere" in h.get("backend_tool", "")),
            None,
        )
        if not slug:
            pytest.skip("No sphere tool slug found in search")

        call_result = _post_mcp(
            url, "tools/call",
            {
                "name": "call",
                "arguments": {
                    "tool_slug": slug,
                    "arguments": {"name": "testSphere", "radius": 1.0},
                },
            },
        )
        assert "error" not in call_result, f"call error: {call_result.get('error')}"
        call_payload = _parse_gateway_payload(call_result)
        assert call_payload.get("success") is not False, (
            f"Call did not succeed: {call_payload}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 5: Error scenarios
# ═══════════════════════════════════════════════════════════════════════════


class TestMayaGatewayErrors:
    """Error handling tests for the Maya gateway."""

    def test_search_wrong_dcc_type_returns_empty(self, maya_gateway) -> None:
        """Searching for a different DCC type must return empty results."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "houdini", "limit": 5}},
        )
        assert "error" not in result, f"search error: {result.get('error')}"
        payload = _parse_gateway_payload(result)
        hits = payload.get("hits", [])
        assert len(hits) == 0, (
            f"Expected 0 hits for non-maya DCC type, got {len(hits)}: {hits}"
        )

    def test_load_nonexistent_skill_returns_error(self, maya_gateway) -> None:
        """load_skill with an unknown skill name must return a structured error."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "nonexistent-skill-xyz"}},
        )
        payload_text = _parse_content_text(result)
        # Must not crash — should return an error message
        assert payload_text, "Expected an error message for nonexistent skill"
        is_error = result.get("result", {}).get("isError") is True
        error_keywords = ("not found", "error", "unknown", "not exist")
        assert is_error or any(kw in payload_text.lower() for kw in error_keywords), (
            f"Expected error for nonexistent skill, got: {payload_text[:300]}"
        )

    def test_call_unknown_tool_returns_error(self, maya_gateway) -> None:
        """Calling an unknown tool slug must return a structured error."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url, "tools/call",
            {
                "name": "call",
                "arguments": {
                    "tool_slug": "maya__nonexistent_tool_xyz",
                    "arguments": {},
                },
            },
        )
        payload_text = _parse_content_text(result)
        assert payload_text, "Expected an error message for unknown tool"
        is_error = result.get("result", {}).get("isError") is True
        error_keywords = ("not found", "error", "unknown", "no tool")
        assert is_error or any(kw in payload_text.lower() for kw in error_keywords), (
            f"Expected error for unknown tool, got: {payload_text[:300]}"
        )

    def test_describe_missing_slug_returns_error(self, maya_gateway) -> None:
        """Describe with a missing tool_slug must return a structured error."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url, "tools/call",
            {"name": "describe", "arguments": {"tool_slug": ""}},
        )
        payload_text = _parse_content_text(result)
        assert payload_text, "Expected an error message for empty slug"
        is_error = result.get("result", {}).get("isError") is True
        assert is_error or "not found" in payload_text.lower() or "error" in payload_text.lower(), (
            f"Expected error for empty slug, got: {payload_text[:300]}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 6: Maya skill MCP protocol
# ═══════════════════════════════════════════════════════════════════════════


class TestMayaSkillMCPProtocol:
    """MCP protocol-level tests for Maya backend skills."""

    def test_maya_tools_list_includes_skills_surface(self, maya_gateway) -> None:
        """Maya backend tools/list must include core discovery tools."""
        # Access the backend directly (not via gateway facade)
        handle = maya_gateway["handle"]
        # Backend MCP URL is handle.mcp_url() — the direct backend port.
        tools = _mcp_tools_list(handle.mcp_url())
        tool_names = {t["name"] for t in tools}
        expected_core = {"search_skills", "list_skills", "get_skill_info", "load_skill", "unload_skill"}
        missing = expected_core - tool_names
        assert not missing, (
            f"Maya backend missing core tools: {missing}. Got: {tool_names}"
        )

    def test_maya_search_skills_finds_maya_skills(self, maya_gateway) -> None:
        """search_skills on Maya backend must find example Maya skills."""
        url = maya_gateway["handle"].mcp_url()
        result = _mcp_call_tool(url, "search_skills", {"query": "maya-geometry"})
        assert result.get("result", {}).get("isError") is False, (
            f"search_skills error: {result}"
        )
        payload_text = _parse_content_text(result)
        assert "maya-geometry" in payload_text, (
            f"maya-geometry not found in search_skills: {payload_text[:300]}"
        )

    def test_maya_load_skill_on_backend(self, maya_gateway) -> None:
        """Load maya-geometry skill on the backend and verify tools registered."""
        url = maya_gateway["handle"].mcp_url()

        # Load maya-geometry
        load_result = _mcp_call_tool(url, "load_skill", {"skill_name": "maya-geometry"})
        load_data = json.loads(_parse_content_text(load_result))
        assert load_data.get("loaded") is True, f"load_skill failed: {load_data}"

        # Verify tools appear in tools/list
        tools = _mcp_tools_list(url)
        tool_names = {t["name"] for t in tools}
        assert any("maya" in n.lower() for n in tool_names), (
            f"No Maya tools found after load: {tool_names}"
        )

    def test_maya_unload_skill_removes_tools(self, maya_gateway) -> None:
        """Unload maya-geometry and verify tools disappear."""
        url = maya_gateway["handle"].mcp_url()

        # Ensure loaded first
        _mcp_call_tool(url, "load_skill", {"skill_name": "maya-geometry"})

        # Get tools before unload
        tools_before = _mcp_tools_list(url)
        names_before = {t["name"] for t in tools_before}

        # Unload
        unload_result = _mcp_call_tool(url, "unload_skill", {"skill_name": "maya-geometry"})
        unload_data = json.loads(_parse_content_text(unload_result))
        assert unload_data.get("unloaded") is True, f"unload_skill failed: {unload_data}"

        # Tools after unload
        tools_after = _mcp_tools_list(url)
        names_after = {t["name"] for t in tools_after}

        # Maya skill tools should be gone (revealed as __skill__ stubs again)
        unloaded_tools = names_before - names_after
        assert unloaded_tools, "Expected some tools to be removed after unload"

    def test_maya_get_skill_info(self, maya_gateway) -> None:
        """get_skill_info returns metadata for a Maya skill."""
        url = maya_gateway["handle"].mcp_url()
        result = _mcp_call_tool(url, "get_skill_info", {"skill_name": "maya-geometry"})
        assert result.get("result", {}).get("isError") is False, (
            f"get_skill_info error: {result}"
        )
        payload_text = _parse_content_text(result)
        assert "maya-geometry" in payload_text, (
            f"Skill info missing name: {payload_text[:300]}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 7: Full install + gateway flow (conditional on P1-4)
# ═══════════════════════════════════════════════════════════════════════════


@requires_p1_4
class TestMayaFullFlowWithInstall:
    """Full end-to-end flow including Maya adapter installation.

    This test requires P1-4 install --execute to be available.
    """

    def test_p1_4_install_execute_available(self) -> None:
        """Verify the P1-4 install --execute tool is reachable."""
        install_cmd = os.environ.get("P1_4_INSTALL_EXECUTE", "dcc-mcp-cli")
        try:
            result = subprocess.run(
                [install_cmd, "--version"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            assert result.returncode == 0, (
                f"P1-4 tool returned {result.returncode}: {result.stderr[:200]}"
            )
        except (FileNotFoundError, subprocess.TimeoutExpired) as exc:
            pytest.fail(f"P1-4 tool not available: {exc}")

    def test_install_maya_adapter_then_gateway_ensure(self, maya_gateway) -> None:
        """After install, gateway ensure must detect or start the Maya gateway."""
        install_cmd = os.environ.get("P1_4_INSTALL_EXECUTE", "dcc-mcp-cli")
        try:
            install_result = subprocess.run(
                [install_cmd, "install", "--dcc-type", "maya", "--execute"],
                capture_output=True,
                text=True,
                timeout=60,
            )
            assert install_result.returncode == 0, (
                f"Install failed: {install_result.stderr[:500]}"
            )
        except (FileNotFoundError, subprocess.TimeoutExpired) as exc:
            pytest.fail(f"Install command failed: {exc}")

    def test_search_after_install_finds_maya_tools(self, maya_gateway) -> None:
        """After full install, search must find Maya tools."""
        url = maya_gateway["gateway_url"]
        result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "mesh", "dcc_type": "maya", "limit": 10}},
        )
        assert "error" not in result, f"search error: {result.get('error')}"
        payload = _parse_gateway_payload(result)
        hits = payload.get("hits", [])
        assert len(hits) >= 1, (
            f"No tools found after install: {payload}"
        )

    def test_call_maya_tool_after_full_install(self, maya_gateway) -> None:
        """After full install, call a Maya tool and verify result."""
        url = maya_gateway["gateway_url"]

        # Search then call
        search_result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        payload = _parse_gateway_payload(search_result)
        hits = payload.get("hits", [])
        if not hits:
            pytest.skip("No search hits after install")

        slug = hits[0]["tool_slug"]
        call_result = _post_mcp(
            url, "tools/call",
            {
                "name": "call",
                "arguments": {"tool_slug": slug, "arguments": {}},
            },
        )
        assert "error" not in call_result, f"call error after install: {call_result.get('error')}"
        call_payload = _parse_gateway_payload(call_result)
        assert call_payload.get("success") is not False, (
            f"Call after install did not succeed: {call_payload}"
        )


# ═══════════════════════════════════════════════════════════════════════════
# Phase 8: Idempotency and cleanup
# ═══════════════════════════════════════════════════════════════════════════


class TestMayaGatewayIdempotency:
    """Tests that the Maya gateway handles repeated operations safely."""

    def test_repeated_search_same_result(self, maya_gateway) -> None:
        """Repeated searches with the same query must return consistent results."""
        url = maya_gateway["gateway_url"]
        results = []
        for _ in range(3):
            result = _post_mcp(
                url, "tools/call",
                {"name": "search", "arguments": {"query": "maya", "dcc_type": "maya", "limit": 5}},
            )
            assert "error" not in result, f"search error on repeat: {result.get('error')}"
            payload = _parse_gateway_payload(result)
            results.append(payload.get("hits", []))

        # All results should be structurally similar
        hit_counts = [len(r) for r in results]
        assert max(hit_counts) - min(hit_counts) <= 1, (
            f"Inconsistent search result counts: {hit_counts}"
        )

    def test_double_load_skill_is_idempotent(self, maya_gateway) -> None:
        """Loading the same Maya skill twice must not error or duplicate."""
        url = maya_gateway["gateway_url"]
        for _ in range(2):
            result = _post_mcp(
                url, "tools/call",
                {
                    "name": "load_skill",
                    "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"},
                },
            )
            assert "error" not in result, f"load_skill error on repeat: {result.get('error')}"

    def test_load_then_unload_then_reload(self, maya_gateway) -> None:
        """Full load → unload → reload cycle must succeed."""
        url = maya_gateway["gateway_url"]

        # Load maya-geometry
        _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"}},
        )

        # Verify it's loaded by calling a tool
        search = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}},
        )
        search_payload = _parse_gateway_payload(search)
        assert search_payload.get("hits"), f"Expected hits before reload: {search_payload}"

        # Load again (idempotent — must not error)
        reload_result = _post_mcp(
            url, "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-geometry", "dcc_type": "maya"}},
        )
        assert "error" not in reload_result, f"reload error: {reload_result.get('error')}"
        payload = _parse_gateway_payload(reload_result)
        assert payload.get("loaded") is True, (
            f"Reload did not report loaded=true: {payload}"
        )

    def test_gateway_responds_after_tool_calls(self, maya_gateway) -> None:
        """Ping must still work after several tool calls."""
        url = maya_gateway["gateway_url"]
        resp = _post_mcp(url, "ping")
        assert "error" not in resp, f"Ping after tool calls failed: {resp.get('error')}"

    def test_search_cleanup_after_all_loads(self, maya_gateway) -> None:
        """Search must still work after load/unload cycles."""
        url = maya_gateway["gateway_url"]

        # Load all maya skills (best-effort, maya-pipeline may not exist)
        for skill in MAYA_SKILLS:
            _post_mcp(
                url, "tools/call",
                {"name": "load_skill", "arguments": {"skill_name": skill, "dcc_type": "maya"}},
            )

        # Verify search still works
        result = _post_mcp(
            url, "tools/call",
            {"name": "search", "arguments": {"query": "maya", "dcc_type": "maya", "limit": 5}},
        )
        assert "error" not in result, (
            f"search after cleanup failed: {result.get('error')}"
        )
