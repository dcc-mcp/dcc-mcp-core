"""CI regression tests: on-demand skill loading contract.

These tests enforce the fundamental invariant of the progressive-loading design:

  BEFORE any explicit load_skill call, the tools/list response MUST NOT
  contain any skill-specific tool with a full input_schema. Every discovered
  but unloaded skill must appear ONLY as a lightweight ``__skill__<name>``
  stub.

This file is intentionally narrow — it is the authoritative CI regression
test that catches regressions where skills are accidentally pre-loaded on
server startup.
"""

from __future__ import annotations

import json
from pathlib import Path
import time
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS = REPO_ROOT / "examples" / "skills"

# Core meta-tools that are ALWAYS present regardless of skill load state.
CORE_TOOLS = frozenset(
    {
        "find_skills",
        "list_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
        "search_skills",
    }
)


# ── helpers ───────────────────────────────────────────────────────────────────


def _post(url: str, body: dict, headers: dict | None = None) -> dict:
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
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _tools_list(url: str) -> list[dict]:
    body = _post(url, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    return body["result"]["tools"]


def _initialize(url: str) -> str:
    data = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "ci-test", "version": "0.1"},
            },
        }
    ).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        header_sid = resp.headers.get("Mcp-Session-Id", "")
        if header_sid:
            return header_sid
        body = json.loads(resp.read())
        return body.get("result", {}).get("__session_id", "")


@pytest.fixture(scope="module")
def catalog_server():
    """Start McpHttpServer with discovered example skills (nothing pre-loaded)."""
    if not EXAMPLES_SKILLS.is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ActionRegistry()
    config = McpHttpConfig(port=0, server_name="ci-on-demand")
    server = McpHttpServer(reg, config)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


# ── Core contract tests ───────────────────────────────────────────────────────


class TestOnDemandLoadingContract:
    """The on-demand loading contract: no skill tool leaks into tools/list before load_skill."""

    def test_initial_tools_list_contains_only_core_and_stubs(self, catalog_server):
        """REGRESSION TEST: tools/list on a freshly started server with discovered
        but unloaded skills MUST contain only:
          1. Core meta-tools (find_skills, load_skill, etc.)
          2. __skill__<name> stub entries

        Any other tool name is a regression — it means a skill was pre-loaded
        (either accidentally on startup or by a background scan).
        """
        url = catalog_server.mcp_url()
        tools = _tools_list(url)

        violations = []
        for tool in tools:
            name = tool["name"]
            if name in CORE_TOOLS:
                continue  # expected
            if name.startswith("__skill__"):
                continue  # expected stub
            # Anything else is a regression
            violations.append(name)

        assert not violations, (
            f"REGRESSION: tools/list contains fully-registered skill tools "
            f"before any load_skill call: {violations}\n"
            f"All tool names: {[t['name'] for t in tools]}\n"
            f"These tools should only appear AFTER load_skill is called."
        )

    def test_stubs_have_no_full_input_schema(self, catalog_server):
        """Stub tools (__skill__*) MUST NOT carry per-parameter inputSchema
        definitions. A stub's inputSchema should be minimal — just enough
        for the agent to call it (or an empty passthrough).
        """
        url = catalog_server.mcp_url()
        tools = _tools_list(url)

        for tool in tools:
            name = tool["name"]
            if not name.startswith("__skill__"):
                continue

            schema = tool.get("inputSchema", {})
            properties = schema.get("properties", {})
            # Stubs may have a 'skill_name' passthrough property,
            # but must NOT have the skill's own tool parameters.
            tool_specific_props = {k: v for k, v in properties.items() if k not in ("skill_name", "arguments")}
            assert not tool_specific_props, (
                f"Stub '{name}' has unexpected parameter definitions in inputSchema: "
                f"{tool_specific_props}\n"
                f"Full schema: {schema}\n"
                f"Stubs must not expose the skill's actual parameter schema before loading."
            )

    def test_discovered_skill_count_matches_stub_count(self, catalog_server):
        """Every discovered skill must appear as exactly one stub.
        stub_count == discovered_skill_count guarantees 1:1 mapping.
        """
        url = catalog_server.mcp_url()
        tools = _tools_list(url)

        stubs = [t["name"] for t in tools if t["name"].startswith("__skill__")]
        skill_names_from_stubs = {s[len("__skill__") :] for s in stubs}

        # list_skills should return the same set of skills as the stubs
        body = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": "list_skills", "arguments": {"status": "all"}},
            },
        )
        list_data = json.loads(body["result"]["content"][0]["text"])
        all_skills = {s["name"] for s in list_data.get("skills", [])}

        assert skill_names_from_stubs == all_skills, (
            f"Stub names {skill_names_from_stubs} don't match list_skills names {all_skills}.\n"
            f"Every discovered skill must have exactly one stub in tools/list."
        )

    def test_no_skill_tool_leaks_after_server_start(self, catalog_server):
        """Call tools/list multiple times in quick succession.
        The result must be stable — no tool should 'appear' between calls
        (which would indicate a background auto-load race condition).
        """
        url = catalog_server.mcp_url()

        snapshots = []
        for _ in range(3):
            tools = _tools_list(url)
            names = frozenset(t["name"] for t in tools)
            snapshots.append(names)
            time.sleep(0.05)

        # All snapshots must be identical
        first = snapshots[0]
        for i, snap in enumerate(snapshots[1:], 1):
            assert snap == first, (
                f"tools/list changed between call 0 and call {i} without any load_skill:\n"
                f"  Added:   {snap - first}\n"
                f"  Removed: {first - snap}\n"
                f"This indicates a background auto-load race condition."
            )

    def test_core_tools_always_present(self, catalog_server):
        """All 6 core meta-tools must be present in every tools/list response."""
        url = catalog_server.mcp_url()
        tools = _tools_list(url)
        names = {t["name"] for t in tools}

        missing = CORE_TOOLS - names
        assert not missing, f"Core meta-tools missing from tools/list: {missing}\nPresent: {names}"

    def test_load_skill_moves_tools_from_stub_to_registered(self, catalog_server):
        """Explicit load_skill call must:
        1. Remove the __skill__<name> stub from tools/list
        2. Add the skill's real tools WITH full inputSchema
        3. Leave other stubs untouched
        """
        url = catalog_server.mcp_url()

        # Snapshot before
        before = {t["name"]: t for t in _tools_list(url)}
        assert "__skill__hello-world" in before, "Expected __skill__hello-world stub before loading"
        assert "hello_world__greet" not in before, "hello_world__greet must not be present before load_skill"

        # Load hello-world
        load_resp = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        load_data = json.loads(load_resp["result"]["content"][0]["text"])
        assert load_data.get("loaded") is True, f"load_skill must return loaded=true, got: {load_data}"

        # Snapshot after
        after = {t["name"]: t for t in _tools_list(url)}

        # Stub gone
        assert "__skill__hello-world" not in after, "The __skill__hello-world stub must be removed after loading"

        # Real tool present
        assert "hello_world__greet" in after, (
            f"hello_world__greet must appear after load_skill. Got: {list(after.keys())}"
        )

        # Other stubs unaffected (count of remaining stubs = before - 1)
        stubs_before = {n for n in before if n.startswith("__skill__")}
        stubs_after = {n for n in after if n.startswith("__skill__")}
        assert len(stubs_after) == len(stubs_before) - 1, (
            f"Expected {len(stubs_before) - 1} stubs after loading hello-world, got {len(stubs_after)}: {stubs_after}"
        )

    def test_unload_skill_restores_stub_removes_tools(self, catalog_server):
        """After unload_skill:
        1. The real tools must disappear
        2. The __skill__<name> stub must reappear
        """
        url = catalog_server.mcp_url()

        # Ensure hello-world is loaded (may already be from previous test)
        _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )

        # Unload
        ul_resp = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {"name": "unload_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        ul_data = json.loads(ul_resp["result"]["content"][0]["text"])
        assert ul_data.get("unloaded") is True, f"unload_skill must return unloaded=true, got: {ul_data}"

        # Snapshot after unload
        after_unload = {t["name"] for t in _tools_list(url)}

        assert "hello_world__greet" not in after_unload, "hello_world__greet must be removed after unload_skill"
        assert "__skill__hello-world" in after_unload, "__skill__hello-world stub must reappear after unload_skill"

    def test_tools_list_count_invariant(self, catalog_server):
        """Invariant: count = CORE(6) + loaded_tools + unloaded_stubs
        Loading a skill with N tools adds N tools and removes 1 stub → net +(N-1).
        Unloading reverses: removes N tools, adds 1 stub → net -(N-1).
        """
        url = catalog_server.mcp_url()

        # Unload hello-world first (in case previous test left it loaded)
        _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {"name": "unload_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )

        count_base = len(_tools_list(url))

        # Load hello-world (1 tool: hello_world__greet)
        _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        count_loaded = len(_tools_list(url))

        # Net change = +1 tool - 1 stub = 0 net for a 1-tool skill
        assert count_loaded == count_base, (
            f"Loading a 1-tool skill: expected net 0 change "
            f"(base={count_base}, after_load={count_loaded}). "
            f"+1 real tool -1 stub = 0"
        )

        # Unload
        _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {"name": "unload_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        count_unloaded = len(_tools_list(url))

        assert count_unloaded == count_base, (
            f"After unload count must return to base. base={count_base}, after_unload={count_unloaded}"
        )
