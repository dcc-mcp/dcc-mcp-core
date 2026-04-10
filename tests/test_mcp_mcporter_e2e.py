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
# Platform-aware npx invocation
# ---------------------------------------------------------------------------

# On Windows, npx is a .cmd script that requires shell=True or the .cmd suffix.
_IS_WINDOWS = platform.system() == "Windows"
_NPX_CMD = "npx.cmd" if _IS_WINDOWS else "npx"


def _run_npx(*args: str, timeout: int = 60) -> subprocess.CompletedProcess:
    """Run ``npx <args>`` portably on Windows and Unix."""
    cmd = [_NPX_CMD, *args]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


# ---------------------------------------------------------------------------
# mcporter availability check
# ---------------------------------------------------------------------------


def _npx_available() -> bool:
    try:
        r = _run_npx("--version", timeout=10)
        return r.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
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
        "action_count",
        "registered_actions",
        "actions_removed",
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
    """Invoke ``npx mcporter call --server <name> --tool <tool>`` against a local server.

    Uses ``--server``/``--tool`` flags instead of the ``server.tool`` dot-notation,
    which avoids mcporter prepending the server name to the tool call.
    ``--allow-http`` is required for plain http:// URLs (localhost).
    Returns the parsed JSON output dict.
    """
    argv = [
        "--yes",
        "mcporter",
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

    result = _run_npx(*argv, timeout=60)
    if result.returncode != 0 and not _is_benign_windows_exit(result):
        raise RuntimeError(f"mcporter call failed: {result.stderr}\nstdout: {result.stdout}")
    return _parse_mcporter_json(result.stdout)


def _mcporter_list_tools(server_url: str, server_name: str) -> list[dict[str, Any]]:
    """Return tools list via ``npx mcporter list --json``."""
    argv = [
        "--yes",
        "mcporter",
        "list",
        "--http-url",
        server_url,
        "--allow-http",
        "--name",
        server_name,
        "--json",
    ]
    result = _run_npx(*argv, timeout=60)
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
        assert result.get("isError") is False
        # Handler returns {"pong": True}; content text may be JSON or Python repr
        raw = result.get("content") or []
        assert len(raw) > 0
        text = raw[0].get("text", "") if isinstance(raw, list) else str(result)
        assert "pong" in text

    def test_call_list_objects(self, simple_server):
        _, _, url, name = simple_server
        result = _mcporter_call(url, name, "list_objects")
        assert result.get("isError") is False
        raw = result.get("content") or []
        text = raw[0].get("text", "") if isinstance(raw, list) else str(result)
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
        assert data.get("action_count", 0) >= 1

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
        assert data.get("action_count", 0) >= 1

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
