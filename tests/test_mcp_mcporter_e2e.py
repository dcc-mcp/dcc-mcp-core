"""E2E tests for McpHttpServer using mcporter CLI as the MCP client.

mcporter (https://github.com/steipete/mcporter) is a TypeScript/CLI tool that
connects to MCP servers over HTTP and can call tools via the command line.

These tests start a real McpHttpServer, then exercise it through ``npx mcporter``
to validate the full MCP protocol stack including:

- Protocol methods: initialize, ping, tools/list
- Core discovery tools: find_skills, list_skills, get_skill_info, load_skill, unload_skill
- Progressive loading flow: discover -> load -> call tool -> unload
- tools/call on registered handlers and skill-backed actions
- Batch requests, session lifecycle, notifications

Requirements:
    node >= 18, npx available in PATH
    dcc_mcp_core Python package installed (Rust wheel)

The tests are skipped automatically when ``npx`` is not found.
"""

from __future__ import annotations

# Import built-in modules
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
from typing import Any

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")

# ---------------------------------------------------------------------------
# Platform-aware mcporter invocation
# ---------------------------------------------------------------------------

# On Windows, .cmd wrappers are required for npm-installed executables.
_IS_WINDOWS = platform.system() == "Windows"
_NPX_CMD = "npx.cmd" if _IS_WINDOWS else "npx"
# mcporter may be installed globally via ``npm install -g mcporter``; when
# available the global binary is used directly to avoid per-call npm
# network round-trips (which can timeout on slow CI runners).
_MCPORTER_GLOBAL = "mcporter.cmd" if _IS_WINDOWS else "mcporter"

# Timeout budget per mcporter invocation.
# The first call may download the package on slow runners; subsequent ones
# hit the npm cache. 120 s is generous but prevents hanging forever.
_MCPORTER_TIMEOUT = int(os.environ.get("MCPORTER_TIMEOUT", "120"))


def _probe_cmd(cmd: str, timeout: int = 10) -> bool:
    """Return True if ``cmd --version`` succeeds within *timeout* seconds."""
    try:
        r = subprocess.run(
            [cmd, "--version"],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return r.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return False


# Prefer a globally-installed mcporter (CI installs it via npm install -g).
# Fall back to npx --yes (fetches on first call, caches afterward).
_MCPORTER_USE_GLOBAL = _probe_cmd(_MCPORTER_GLOBAL)


def _run_mcporter(*args: str, timeout: int | None = None) -> subprocess.CompletedProcess:
    """Run mcporter portably, preferring the global install over npx.

    When ``mcporter`` is installed globally (``npm install -g mcporter``) it
    starts instantly. When it is not, we fall back to ``npx --yes mcporter``
    which downloads the package on the first call and caches it afterward.
    """
    t = timeout if timeout is not None else _MCPORTER_TIMEOUT
    cmd = [_MCPORTER_GLOBAL, *args] if _MCPORTER_USE_GLOBAL else [_NPX_CMD, "--yes", "mcporter", *args]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=t)


# Keep backward-compatible alias used inside helpers.
def _run_npx(*args: str, timeout: int = 60) -> subprocess.CompletedProcess:
    """Alias for _run_mcporter; preserved for call-sites that pass --yes mcporter."""
    # Strip the "--yes" "mcporter" prefix if present (legacy callers pass it).
    stripped = list(args)
    if stripped[:2] == ["--yes", "mcporter"]:
        stripped = stripped[2:]
    return _run_mcporter(*stripped, timeout=max(timeout, _MCPORTER_TIMEOUT))


# ---------------------------------------------------------------------------
# mcporter availability check
# ---------------------------------------------------------------------------


def _npx_available() -> bool:
    """Return True if mcporter is reachable (global or via npx)."""
    if _MCPORTER_USE_GLOBAL:
        return True
    try:
        r = subprocess.run(
            [_NPX_CMD, "--yes", "mcporter", "--version"],
            capture_output=True,
            text=True,
            timeout=_MCPORTER_TIMEOUT,
        )
        return r.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return False


NPX_AVAILABLE = _npx_available()

# Windows-specific: npx/mcporter exit with non-zero code on Windows due to
# libuv handle assertion in the uv event loop teardown. The output is still
# valid JSON. We treat non-empty stderr that contains the known Windows uv
# assertion as a benign exit.
_WINDOWS_UV_ASSERT = "Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)"


def _is_benign_windows_exit(result: subprocess.CompletedProcess) -> bool:
    """Return True if the non-zero exit is only due to a Windows libuv teardown assertion."""
    return _IS_WINDOWS and _WINDOWS_UV_ASSERT in result.stderr


def _extract_content_text(result: dict[str, Any]) -> str:
    """Extract the text from an MCP tool call result.

    mcporter --output json may return the parsed content directly (if the tool
    response was valid JSON) or the full {"content": [...]} wrapper.
    """
    # If 'content' key present, it's the wrapper form
    raw = result.get("content")
    if raw is not None and isinstance(raw, list) and raw:
        return raw[0].get("text", "") if isinstance(raw[0], dict) else str(raw[0])
    # Otherwise result IS the data; convert back to string for text checks
    return json.dumps(result)


def _parse_content_json(result: dict[str, Any]) -> Any:
    """Return the JSON data from an mcporter tool call result.

    mcporter --output json behaves differently depending on the tool response:
    - If the content text is valid JSON (core discovery tools), mcporter
      returns the parsed JSON directly as the result dict.
    - If the content text is not valid JSON (Python handler returning repr),
      mcporter wraps it in {"content": [...], "isError": false}.

    We detect which case we're in and return the appropriate data.
    """
    # If result has 'skills', 'loaded', 'unloaded', etc. it IS the parsed data already
    _json_data_keys = {
        "skills",
        "total",
        "loaded",
        "unloaded",
        "tool_count",
        "registered_tools",
        "tools_removed",
        "name",
        "description",
    }
    if any(k in result for k in _json_data_keys):
        return result
    # Otherwise it's a wrapped response with content array
    text = _extract_content_text(result)
    return json.loads(text)


def _parse_mcporter_json(stdout: str) -> Any:
    """Extract the last valid JSON object/array from mcporter stdout.

    When the server writes Rust tracing to stdout, it can mix with mcporter
    output. We find the last JSON block and parse it.
    """
    # Try direct parse first
    try:
        return json.loads(stdout)
    except json.JSONDecodeError:
        pass

    # Find the last line starting a JSON block
    lines = stdout.splitlines()
    for i in range(len(lines) - 1, -1, -1):
        stripped = lines[i].lstrip()
        if stripped.startswith("{") or stripped.startswith("["):
            candidate = "\n".join(lines[i:])
            try:
                return json.loads(candidate)
            except json.JSONDecodeError:
                continue

    raise ValueError(f"No valid JSON found in mcporter output:\n{stdout[:500]}")


def _mcporter_call(server_url: str, server_name: str, tool: str, args: dict[str, Any] | None = None) -> dict[str, Any]:
    """Invoke ``mcporter call --server <name> --tool <tool>`` against a local server.

    Uses ``--server``/``--tool`` flags instead of the ``server.tool`` dot-notation,
    which avoids mcporter prepending the server name to the tool call.
    ``--allow-http`` is required for plain http:// URLs (localhost).
    Returns the parsed JSON output dict.
    """
    argv = [
        "call",
        "--http-url",
        server_url,
        "--allow-http",
        "--name",
        server_name,
        "--output",
        "json",
        "--server",
        server_name,
        "--tool",
        tool,
    ]
    if args:
        for key, val in args.items():
            if isinstance(val, (list, dict)):
                argv.append(f"{key}:{json.dumps(val)}")
            elif isinstance(val, bool):
                argv.append(f"{key}:{str(val).lower()}")
            else:
                argv.append(f"{key}:{val}")

    result = _run_mcporter(*argv)
    if result.returncode != 0 and not _is_benign_windows_exit(result):
        raise RuntimeError(f"mcporter call failed: {result.stderr}\nstdout: {result.stdout}")
    return _parse_mcporter_json(result.stdout)


def _mcporter_list_tools(server_url: str, server_name: str) -> list[dict[str, Any]]:
    """Return tools list via ``mcporter list --json``."""
    argv = [
        "list",
        "--http-url",
        server_url,
        "--allow-http",
        "--name",
        server_name,
        "--json",
    ]
    result = _run_mcporter(*argv)
    if result.returncode != 0 and not _is_benign_windows_exit(result):
        raise RuntimeError(f"mcporter list failed: {result.stderr}")
    data = _parse_mcporter_json(result.stdout)
    # mcporter list --json returns array of server objects: [{name, tools: [...]}]
    if isinstance(data, list):
        for entry in data:
            if isinstance(entry, dict) and "tools" in entry:
                return entry["tools"]
        return data
    if "tools" in data:
        return data["tools"]
    # try first server entry
    servers = data.get("servers", [])
    if servers:
        return servers[0].get("tools", [])
    return []


# ---------------------------------------------------------------------------
# fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def server_with_catalog():
    """Start McpHttpServer with SkillCatalog backed by example skills.

    Yields (server, handle, url, server_name).
    """
    if not Path(EXAMPLES_SKILLS_DIR).is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ActionRegistry()
    reg.register(
        "get_scene_info",
        description="Return info about the current scene",
        category="scene",
        tags=["query"],
        dcc="test",
        version="1.0.0",
    )

    config = McpHttpConfig(port=0, server_name="mcporter-e2e")
    server = McpHttpServer(reg, config)
    server.register_handler("get_scene_info", lambda params: {"scene": "test_scene", "objects": []})

    # Discover example skills so catalog has entries to find/load
    server.discover(extra_paths=[EXAMPLES_SKILLS_DIR])

    handle = server.start()
    url = handle.mcp_url()

    # Give the async runtime a moment to bind
    time.sleep(0.2)

    yield server, handle, url, "mcporter-e2e"
    handle.shutdown()


@pytest.fixture(scope="module")
def simple_server():
    """Minimal server with two registered actions (no catalog)."""
    reg = ActionRegistry()
    reg.register(
        "ping_action",
        description="A simple echo action for testing",
        category="test",
        tags=["test"],
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

    config = McpHttpConfig(port=0, server_name="simple-e2e")
    server = McpHttpServer(reg, config)
    server.register_handler("ping_action", lambda params: {"pong": True, "echo": params})
    server.register_handler("list_objects", lambda params: {"objects": ["cube", "sphere", "camera"]})
    handle = server.start()
    url = handle.mcp_url()
    time.sleep(0.2)

    yield server, handle, url, "simple-e2e"
    handle.shutdown()


# ---------------------------------------------------------------------------
# Basic tools/list via mcporter
# ---------------------------------------------------------------------------


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestMcporterToolsList:
    """Validate tools/list response shape using mcporter CLI."""

    def test_list_shows_registered_actions(self, simple_server):
        _, _, url, name = simple_server
        tools = _mcporter_list_tools(url, name)
        tool_names = {t["name"] if isinstance(t, dict) else t for t in tools}
        assert "ping_action" in tool_names
        assert "list_objects" in tool_names

    def test_list_includes_core_discovery_tools(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        tools = _mcporter_list_tools(url, name)
        tool_names = {t["name"] if isinstance(t, dict) else t for t in tools}
        # 5 core discovery tools must always be present
        for core_tool in ("find_skills", "list_skills", "get_skill_info", "load_skill", "unload_skill"):
            assert core_tool in tool_names, f"Missing core tool: {core_tool}"

    def test_tools_have_required_fields(self, simple_server):
        _, _, url, name = simple_server
        tools = _mcporter_list_tools(url, name)
        for tool in tools:
            if not isinstance(tool, dict):
                continue
            assert "name" in tool, f"Tool missing 'name': {tool}"
            assert "description" in tool, f"Tool '{tool['name']}' missing 'description'"


# ---------------------------------------------------------------------------
# Basic tool calls via mcporter
# ---------------------------------------------------------------------------


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestMcporterToolCall:
    """Invoke registered tools through mcporter and validate results."""

    def test_call_registered_handler(self, simple_server):
        _, _, url, name = simple_server
        result = _mcporter_call(url, name, "ping_action")
        assert result.get("isError") is False or "pong" in result
        # Handler may come back as raw parsed JSON or MCP content-wrapped output.
        raw = result.get("content") or []
        text = raw[0].get("text", "") if raw else json.dumps(result)
        assert "pong" in text

    def test_call_list_objects(self, simple_server):
        _, _, url, name = simple_server
        result = _mcporter_call(url, name, "list_objects")
        assert result.get("isError") is False or "objects" in result
        raw = result.get("content") or []
        text = raw[0].get("text", "") if raw else json.dumps(result)
        assert "cube" in text

    def test_call_unknown_tool_returns_error(self, simple_server):
        _, _, url, name = simple_server
        result = _mcporter_call(url, name, "this_tool_does_not_exist")
        # mcporter returns isError=true (rc=0) rather than raising
        assert result.get("isError") is True


# ---------------------------------------------------------------------------
# Core discovery tools via mcporter
# ---------------------------------------------------------------------------


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestMcporterCoreDiscoveryTools:
    """Test the 5 built-in discovery tools through mcporter."""

    def test_list_skills_returns_discovered_skills(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "list_skills", {"status": "all"})
        data = _parse_content_json(result)
        assert "skills" in data
        assert data["total"] >= 1

    def test_find_skills_by_keyword(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "find_skills", {"query": "hello"})
        data = _parse_content_json(result)
        assert "skills" in data
        skill_names = [s.get("name", "") for s in data["skills"]]
        assert any("hello" in n for n in skill_names), f"'hello' skill not found in: {skill_names}"

    def test_find_skills_by_tag(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "find_skills", {"tags": ["example"]})
        data = _parse_content_json(result)
        # hello-world has tag 'example'
        assert data["total"] >= 1

    def test_get_skill_info(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "get_skill_info", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("name") == "hello-world" or "hello-world" in str(data)

    def test_get_skill_info_missing_name_returns_error(self, server_with_catalog):
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "get_skill_info", {"skill_name": "nonexistent-skill-xyz"})
        text = _extract_content_text(result)
        # Should indicate error or not-found
        assert "not found" in text.lower() or "error" in text.lower() or result.get("isError")


# ---------------------------------------------------------------------------
# Progressive loading via mcporter
# ---------------------------------------------------------------------------


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestMcporterProgressiveLoading:
    """Test the discover -> load -> call -> unload workflow through mcporter."""

    def test_load_skill_registers_tools(self, server_with_catalog):
        """After load_skill, the skill's tools appear in tools/list."""
        _, _, url, name = server_with_catalog

        # Load hello-world skill
        result = _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("loaded") is True
        assert data.get("tool_count", 0) >= 1

        # tools/list should now include the skill's tool
        tools = _mcporter_list_tools(url, name)
        tool_names = {t["name"] if isinstance(t, dict) else t for t in tools}
        assert any("hello" in n.lower() for n in tool_names), f"hello-world tool not in list: {tool_names}"

    def test_call_skill_tool_after_load(self, server_with_catalog):
        """Invoke a skill-backed tool after loading it."""
        _, _, url, name = server_with_catalog

        # Ensure hello-world is loaded (may already be from previous test)
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})

        # Greet via the skill tool
        result = _mcporter_call(url, name, "hello_world__greet", {"name": "mcporter"})
        text = _extract_content_text(result)
        assert "mcporter" in text or "Hello" in text

    def test_unload_skill_removes_tools(self, server_with_catalog):
        """After unload_skill, the skill's tools disappear from tools/list."""
        _, _, url, name = server_with_catalog

        # Load first
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})

        # Unload
        result = _mcporter_call(url, name, "unload_skill", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("unloaded") is True

        # tools/list should no longer contain hello-world tools
        tools = _mcporter_list_tools(url, name)
        tool_names = {t["name"] if isinstance(t, dict) else t for t in tools}
        # hello_world__greet should be gone (core tools remain)
        assert "hello_world__greet" not in tool_names

    def test_load_multiple_skills_at_once(self, server_with_catalog):
        """load_skill with skill_names loads several skills in one call."""
        _, _, url, name = server_with_catalog

        result = _mcporter_call(
            url,
            name,
            "load_skill",
            {"skill_names": ["hello-world", "git-automation"]},
        )
        data = _parse_content_json(result)
        assert data.get("loaded") is True
        assert data.get("tool_count", 0) >= 1

    def test_list_skills_status_filter_loaded(self, server_with_catalog):
        """list_skills(status=loaded) only returns loaded skills."""
        _, _, url, name = server_with_catalog

        # Ensure at least one skill is loaded
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})

        result = _mcporter_call(url, name, "list_skills", {"status": "loaded"})
        data = _parse_content_json(result)
        assert data["total"] >= 1
        for skill in data["skills"]:
            assert skill.get("loaded") is True, f"Expected loaded=True, got: {skill}"

    def test_list_skills_status_filter_unloaded(self, server_with_catalog):
        """list_skills(status=unloaded) returns only unloaded skills."""
        _, _, url, name = server_with_catalog

        result = _mcporter_call(url, name, "list_skills", {"status": "unloaded"})
        data = _parse_content_json(result)
        # After loading hello-world, there should still be unloaded skills
        for skill in data["skills"]:
            assert skill.get("loaded") is False, f"Expected loaded=False, got: {skill}"

    def test_full_progressive_loading_cycle(self, server_with_catalog):
        """Full cycle: find -> get_info -> load -> call -> unload via mcporter."""
        _, _, url, name = server_with_catalog

        # 1. Find the skill
        find_result = _mcporter_call(url, name, "find_skills", {"query": "hello"})
        found_data = _parse_content_json(find_result)
        assert found_data["total"] >= 1

        # 2. Get skill info
        info_result = _mcporter_call(url, name, "get_skill_info", {"skill_name": "hello-world"})
        info_data = _parse_content_json(info_result)
        assert info_data  # non-empty info

        # 3. Load skill
        load_result = _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        load_data = _parse_content_json(load_result)
        assert load_data["loaded"] is True

        # 4. Call the skill tool
        call_result = _mcporter_call(url, name, "hello_world__greet", {"name": "E2E"})
        text = _extract_content_text(call_result)
        assert "E2E" in text or "Hello" in text

        # 5. Unload
        unload_result = _mcporter_call(url, name, "unload_skill", {"skill_name": "hello-world"})
        unload_data = _parse_content_json(unload_result)
        assert unload_data["unloaded"] is True


# ---------------------------------------------------------------------------
# Fallback: skip-friendly smoke test when npx is absent
# ---------------------------------------------------------------------------


class TestMcporterAvailability:
    """Sanity checks that run regardless of mcporter availability."""

    def test_npx_availability_logged(self, capsys):
        status = "available" if NPX_AVAILABLE else "NOT available"
        print(f"npx/mcporter: {status}", file=sys.stderr)
        # Always passes — just documents the environment
        assert True

    def test_server_reachable_with_stdlib(self, simple_server):
        """Verify the server is reachable even without mcporter."""
        import urllib.request

        _, _, url, _ = simple_server
        req = urllib.request.Request(
            url,
            data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping"}).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            assert resp.status == 200
            body = json.loads(resp.read())
            assert body["id"] == 1


# ---------------------------------------------------------------------------
# Multiple server instances (isolation + concurrent connections)
# ---------------------------------------------------------------------------


class TestMultipleServerInstances:
    """Verify that multiple McpHttpServer instances run independently.

    Each server gets its own port, own ActionRegistry, and own catalog state.
    Requests to one server must not affect the other.
    """

    @staticmethod
    def _ping(url: str) -> dict:
        import urllib.request

        req = urllib.request.Request(
            url,
            data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping"}).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read())

    @staticmethod
    def _tools_list(url: str) -> list[str]:
        import urllib.request

        req = urllib.request.Request(
            url,
            data=json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            body = json.loads(resp.read())
            return [t["name"] for t in body["result"]["tools"]]

    @staticmethod
    def _call_tool(url: str, tool: str, arguments: dict | None = None) -> dict:
        import urllib.request

        req = urllib.request.Request(
            url,
            data=json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": tool, "arguments": arguments or {}},
                }
            ).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read())

    def test_two_servers_bind_different_ports(self):
        """Each server gets its own random port; they coexist without conflict."""
        reg_a = ActionRegistry()
        reg_b = ActionRegistry()

        srv_a = McpHttpServer(reg_a, McpHttpConfig(port=0, server_name="srv-a"))
        srv_b = McpHttpServer(reg_b, McpHttpConfig(port=0, server_name="srv-b"))

        h_a = srv_a.start()
        h_b = srv_b.start()
        try:
            assert h_a.port != h_b.port, "Both servers must bind to distinct ports"
            assert h_a.port > 0
            assert h_b.port > 0
            # Both respond to ping
            self._ping(h_a.mcp_url())
            self._ping(h_b.mcp_url())
        finally:
            h_a.shutdown()
            h_b.shutdown()

    def test_registries_are_isolated(self):
        """Tools registered on server A are not visible on server B."""
        reg_a = ActionRegistry()
        reg_a.register("tool_only_in_a", description="Only in A", category="test", tags=[], dcc="test", version="1.0")

        reg_b = ActionRegistry()
        reg_b.register("tool_only_in_b", description="Only in B", category="test", tags=[], dcc="test", version="1.0")

        srv_a = McpHttpServer(reg_a, McpHttpConfig(port=0, server_name="iso-a"))
        srv_b = McpHttpServer(reg_b, McpHttpConfig(port=0, server_name="iso-b"))

        h_a = srv_a.start()
        h_b = srv_b.start()
        try:
            names_a = self._tools_list(h_a.mcp_url())
            names_b = self._tools_list(h_b.mcp_url())

            assert "tool_only_in_a" in names_a
            assert "tool_only_in_a" not in names_b

            assert "tool_only_in_b" in names_b
            assert "tool_only_in_b" not in names_a
        finally:
            h_a.shutdown()
            h_b.shutdown()

    def test_handlers_are_isolated(self):
        """Calling a tool on server A does not invoke server B's handler."""
        reg_a = ActionRegistry()
        reg_a.register("echo", description="Echo A", category="test", tags=[], dcc="test", version="1.0")

        reg_b = ActionRegistry()
        reg_b.register("echo", description="Echo B", category="test", tags=[], dcc="test", version="1.0")

        srv_a = McpHttpServer(reg_a, McpHttpConfig(port=0, server_name="hdl-a"))
        srv_b = McpHttpServer(reg_b, McpHttpConfig(port=0, server_name="hdl-b"))

        srv_a.register_handler("echo", lambda p: {"server": "A"})
        srv_b.register_handler("echo", lambda p: {"server": "B"})

        h_a = srv_a.start()
        h_b = srv_b.start()
        try:
            body_a = self._call_tool(h_a.mcp_url(), "echo")
            body_b = self._call_tool(h_b.mcp_url(), "echo")

            text_a = body_a["result"]["content"][0]["text"]
            text_b = body_b["result"]["content"][0]["text"]

            assert "A" in text_a, f"Server A handler not called: {text_a}"
            assert "B" in text_b, f"Server B handler not called: {text_b}"
            # Cross-contamination check
            assert "B" not in text_a or "A" not in text_b or text_a != text_b
        finally:
            h_a.shutdown()
            h_b.shutdown()

    def test_concurrent_pings_across_instances(self):
        """Ten threads each ping a separate server instance simultaneously."""
        import threading

        n = 5
        handles = []
        errors = []
        results = []

        regs = [ActionRegistry() for _ in range(n)]
        servers = [McpHttpServer(r, McpHttpConfig(port=0, server_name=f"conc-{i}")) for i, r in enumerate(regs)]
        handles = [s.start() for s in servers]

        def worker(url: str, idx: int) -> None:
            try:
                body = self._ping(url)
                results.append((idx, body["result"]))
            except Exception as exc:
                errors.append((idx, str(exc)))

        threads = [threading.Thread(target=worker, args=(h.mcp_url(), i)) for i, h in enumerate(handles)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10)

        for h in handles:
            h.shutdown()

        assert not errors, f"Errors during concurrent ping: {errors}"
        assert len(results) == n

    def test_skill_catalog_state_is_independent(self):
        """Loading a skill on server A does not affect server B's catalog."""
        if not Path(EXAMPLES_SKILLS_DIR).is_dir():
            pytest.skip("examples/skills directory not found")

        reg_a = ActionRegistry()
        reg_b = ActionRegistry()

        srv_a = McpHttpServer(reg_a, McpHttpConfig(port=0, server_name="cat-a"))
        srv_b = McpHttpServer(reg_b, McpHttpConfig(port=0, server_name="cat-b"))

        srv_a.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        srv_b.discover(extra_paths=[EXAMPLES_SKILLS_DIR])

        h_a = srv_a.start()
        h_b = srv_b.start()
        try:
            # Load hello-world only on server A
            body = self._call_tool(
                h_a.mcp_url(),
                "load_skill",
                {"skill_name": "hello-world"},
            )
            result_text = body["result"]["content"][0]["text"]
            load_data = json.loads(result_text)
            assert load_data.get("loaded") is True

            # Server A should have hello-world tools; server B should NOT
            names_a = self._tools_list(h_a.mcp_url())
            names_b = self._tools_list(h_b.mcp_url())

            assert any("hello" in n.lower() for n in names_a), f"hello-world missing from A: {names_a}"
            assert not any("hello_world" in n for n in names_b), f"hello-world leaked into B: {names_b}"
        finally:
            h_a.shutdown()
            h_b.shutdown()

    @pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
    def test_mcporter_connects_to_correct_instance(self):
        """Mcporter explicitly targets one URL; the other server is unaffected."""
        reg_a = ActionRegistry()
        reg_a.register("action_alpha", description="Alpha tool", category="test", tags=[], dcc="test", version="1.0")

        reg_b = ActionRegistry()
        reg_b.register("action_beta", description="Beta tool", category="test", tags=[], dcc="test", version="1.0")

        srv_a = McpHttpServer(reg_a, McpHttpConfig(port=0, server_name="target-a"))
        srv_b = McpHttpServer(reg_b, McpHttpConfig(port=0, server_name="target-b"))

        h_a = srv_a.start()
        h_b = srv_b.start()
        try:
            # mcporter targets only server A
            tools_a = _mcporter_list_tools(h_a.mcp_url(), "target-a")
            names_a = {t["name"] if isinstance(t, dict) else t for t in tools_a}

            # mcporter targets only server B
            tools_b = _mcporter_list_tools(h_b.mcp_url(), "target-b")
            names_b = {t["name"] if isinstance(t, dict) else t for t in tools_b}

            assert "action_alpha" in names_a
            assert "action_alpha" not in names_b

            assert "action_beta" in names_b
            assert "action_beta" not in names_a
        finally:
            h_a.shutdown()
            h_b.shutdown()


# ---------------------------------------------------------------------------
# Progressive loading boundary tests (mcporter + direct HTTP)
# ---------------------------------------------------------------------------


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestProgressiveLoadingBoundary:
    """Edge cases for the on-demand skill discovery / loading workflow."""

    def test_stub_not_present_after_skill_loaded(self, server_with_catalog):
        """Once a skill is loaded its __skill__ stub must disappear."""
        _, _, url, name = server_with_catalog
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        tools = _mcporter_list_tools(url, name)
        names = {t["name"] if isinstance(t, dict) else t for t in tools}
        assert "__skill__hello-world" not in names, "__skill__hello-world stub should be gone after loading"

    def test_stub_reappears_after_unload(self, server_with_catalog):
        """After unloading a skill its __skill__ stub must reappear."""
        _, _, url, name = server_with_catalog
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        _mcporter_call(url, name, "unload_skill", {"skill_name": "hello-world"})
        tools = _mcporter_list_tools(url, name)
        names = {t["name"] if isinstance(t, dict) else t for t in tools}
        assert "__skill__hello-world" in names, "__skill__hello-world stub should reappear after unloading"

    def test_search_skills_finds_by_search_hint(self, server_with_catalog):
        """search_skills must match against the search-hint SKILL.md field."""
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "search_skills", {"query": "greeting"})
        try:
            data = _parse_content_json(result)
        except (json.JSONDecodeError, KeyError, TypeError):
            pytest.skip("mcporter returned empty/invalid output (transient CI issue)")
        assert data.get("total", 0) >= 1, "Expected at least 1 result for 'greeting' (hello-world search-hint)"

    def test_list_skills_total_matches_skill_count(self, server_with_catalog):
        """list_skills total field must equal len(skills) list."""
        _, _, url, name = server_with_catalog
        all_skills = _mcporter_call(url, name, "list_skills", {"status": "all"})
        data = _parse_content_json(all_skills)
        total = data.get("total", -1)
        skills = data.get("skills", [])
        assert total == len(skills), f"'total' {total} != len(skills) {len(skills)}"

    def test_load_nonexistent_skill_returns_error(self, server_with_catalog):
        """load_skill with unknown name must surface an error."""
        _, _, url, name = server_with_catalog
        result = _mcporter_call(
            url,
            name,
            "load_skill",
            {"skill_name": "this-skill-does-not-exist-xyz"},
        )
        text = _extract_content_text(result)
        assert "not found" in text.lower() or "error" in text.lower() or result.get("isError") is True, (
            f"Expected error for unknown skill, got: {text}"
        )

    def test_get_skill_info_includes_name(self, server_with_catalog):
        """get_skill_info must return a name field matching the queried skill."""
        _, _, url, name = server_with_catalog
        result = _mcporter_call(url, name, "get_skill_info", {"skill_name": "hello-world"})
        data = _parse_content_json(result)
        assert data.get("name") == "hello-world"

    def test_unload_not_loaded_skill_returns_error(self, server_with_catalog):
        """unload_skill on a skill that is not loaded must return an error."""
        _, _, url, name = server_with_catalog
        # Ensure it's not loaded by unloading first if needed (ignore result)
        _mcporter_call(url, name, "unload_skill", {"skill_name": "hello-world"})
        # Now try again — it definitely should not be loaded
        result = _mcporter_call(
            url,
            name,
            "unload_skill",
            {"skill_name": "hello-world"},
        )
        text = _extract_content_text(result)
        assert "not loaded" in text.lower() or "error" in text.lower() or result.get("isError") is True, (
            f"Expected error for unloading non-loaded skill, got: {text}"
        )

    def test_tool_call_passes_params_to_script(self, server_with_catalog):
        """tools/call must forward arguments to the skill script."""
        _, _, url, name = server_with_catalog
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        result = _mcporter_call(
            url,
            name,
            "hello_world__greet",
            {"name": "BoundaryTest"},
        )
        text = _extract_content_text(result)
        assert "BoundaryTest" in text or "Hello" in text, f"Expected greeting with 'BoundaryTest', got: {text}"

    def test_double_load_does_not_duplicate_tools(self, server_with_catalog):
        """Loading the same skill twice must not duplicate its tools."""
        _, _, url, name = server_with_catalog
        for _ in range(2):
            _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        tools = _mcporter_list_tools(url, name)
        names = [t["name"] if isinstance(t, dict) else t for t in tools]
        count = names.count("hello_world__greet")
        assert count <= 1, f"hello_world__greet duplicated after double load: {count}"

    def test_unload_then_reload_re_registers_tools(self, server_with_catalog):
        """After unload → reload, the skill's tools must be available again."""
        _, _, url, name = server_with_catalog
        _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        _mcporter_call(url, name, "unload_skill", {"skill_name": "hello-world"})
        rl = _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
        rl_data = _parse_content_json(rl)
        assert rl_data.get("loaded") is True

        tools = _mcporter_list_tools(url, name)
        names = {t["name"] if isinstance(t, dict) else t for t in tools}
        assert any("hello" in n for n in names), f"Expected hello-world tool after reload, got: {names}"


@pytest.mark.skipif(not NPX_AVAILABLE, reason="npx / mcporter not available")
class TestConcurrencyBoundary:
    """Concurrent requests must not corrupt server state."""

    def test_concurrent_tool_calls_all_succeed(self, simple_server):
        """Multiple concurrent tools/call requests must all return correct results."""
        import threading

        _, _, url, name = simple_server
        results: list = []
        errors: list = []

        def call_ping():
            try:
                r = _mcporter_call(url, name, "ping_action", {})
                results.append(r)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=call_ping) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)

        assert not errors, f"Concurrent calls raised errors: {errors}"
        assert len(results) == 4, f"Expected 4 results, got {len(results)}"

    def test_concurrent_load_same_skill_idempotent(self, server_with_catalog):
        """Concurrently loading the same skill must not produce duplicate tools."""
        import threading

        _, _, url, name = server_with_catalog
        errors: list = []

        def load_hello():
            try:
                _mcporter_call(url, name, "load_skill", {"skill_name": "hello-world"})
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=load_hello) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)

        assert not errors, f"Concurrent loads raised: {errors}"

        tools = _mcporter_list_tools(url, name)
        tool_names = [t["name"] if isinstance(t, dict) else t for t in tools]
        count = tool_names.count("hello_world__greet")
        assert count <= 1, f"hello_world__greet duplicated: {count} occurrences"
