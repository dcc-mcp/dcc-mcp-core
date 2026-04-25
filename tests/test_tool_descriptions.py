"""Structural tests for built-in MCP tool descriptions (issue #341).

Every built-in tool registered on a fresh ``McpHttpServer`` must ship a
description that follows the 3-layer "what / when to use / how to use"
structure introduced in issue #341 and must fit inside the 500-char
soft cap documented in ``AGENTS.md``.  Per-parameter ``description``
strings inside each ``inputSchema`` must stay under 100 chars so the
full schema remains readable in MCP clients that display them inline.

These assertions are intentionally structural — they do NOT pin exact
description text, so future copy tweaks stay cheap while still
guaranteeing the contract for AI agents that rely on the 3-layer
choreography.
"""

from __future__ import annotations

import json
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# Built-in tools always emitted by the HTTP server regardless of whether
# any skill has been loaded.  Kept in sync with
# ``build_core_tools_inner()`` + ``build_lazy_action_tools()`` in
# ``crates/dcc-mcp-http/src/handler.rs``.
CORE_TOOLS = frozenset(
    {
        "list_roots",
        "search_skills",
        "list_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
        "activate_tool_group",
        "deactivate_tool_group",
        "search_tools",
    }
)

LAZY_ACTION_TOOLS = frozenset({"list_actions", "describe_action", "call_action"})

MAX_DESCRIPTION_CHARS = 500
MAX_PARAM_DESCRIPTION_CHARS = 100


# ── helpers ───────────────────────────────────────────────────────────────────


def _post(url: str, body: dict[str, Any]) -> dict[str, Any]:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _tools_list(url: str) -> list[dict[str, Any]]:
    body = _post(url, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    return body["result"]["tools"]


def _iter_param_descriptions(schema: dict[str, Any]) -> list[tuple[str, str]]:
    """Yield ``(param_name, description)`` for every property in ``schema``."""
    out: list[tuple[str, str]] = []
    for name, prop in (schema.get("properties") or {}).items():
        desc = prop.get("description")
        if isinstance(desc, str):
            out.append((name, desc))
    return out


# ── fixtures ──────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def core_server():
    """Server with no skills loaded — exposes only the built-in tools."""
    reg = ToolRegistry()
    config = McpHttpConfig(port=0, server_name="ci-tool-descriptions")
    server = McpHttpServer(reg, config)
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


@pytest.fixture(scope="module")
def lazy_action_server():
    """Server with ``lazy_actions=True`` — adds the 3 lazy-action meta-tools."""
    reg = ToolRegistry()
    config = McpHttpConfig(port=0, server_name="ci-lazy-tool-descriptions")
    config.lazy_actions = True
    server = McpHttpServer(reg, config)
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


# ── structural assertions ────────────────────────────────────────────────────


def _assert_description_structure(name: str, description: str) -> None:
    assert isinstance(description, str) and description, f"tool {name!r} has an empty description"
    assert len(description) <= MAX_DESCRIPTION_CHARS, (
        f"tool {name!r} description is {len(description)} chars, "
        f"exceeds soft cap of {MAX_DESCRIPTION_CHARS} — "
        "move long prose into docs/api/http.md"
    )
    assert "When to use:" in description, (
        f"tool {name!r} description is missing the 'When to use:' section "
        "required by the 3-layer structure (issue #341)"
    )
    assert "How to use:" in description, (
        f"tool {name!r} description is missing the 'How to use:' section required by the 3-layer structure (issue #341)"
    )


def _assert_param_descriptions(tool: dict[str, Any]) -> None:
    schema = tool.get("inputSchema") or {}
    for pname, pdesc in _iter_param_descriptions(schema):
        assert len(pdesc) <= MAX_PARAM_DESCRIPTION_CHARS, (
            f"tool {tool['name']!r}, param {pname!r}: description is "
            f"{len(pdesc)} chars, exceeds {MAX_PARAM_DESCRIPTION_CHARS}"
        )


# ── tests ─────────────────────────────────────────────────────────────────────


class TestCoreToolDescriptions:
    def test_all_core_tools_present(self, core_server):
        tools = _tools_list(core_server.mcp_url())
        names = {t["name"] for t in tools}
        missing = CORE_TOOLS - names
        assert not missing, f"missing core tools: {missing}"

    def test_core_tool_descriptions_follow_3_layer_structure(self, core_server):
        tools = {t["name"]: t for t in _tools_list(core_server.mcp_url())}
        for name in CORE_TOOLS:
            _assert_description_structure(name, tools[name].get("description", ""))

    def test_core_tool_param_descriptions_under_cap(self, core_server):
        tools = {t["name"]: t for t in _tools_list(core_server.mcp_url())}
        for name in CORE_TOOLS:
            _assert_param_descriptions(tools[name])


class TestLazyActionToolDescriptions:
    def test_all_lazy_action_tools_present(self, lazy_action_server):
        tools = _tools_list(lazy_action_server.mcp_url())
        names = {t["name"] for t in tools}
        missing = LAZY_ACTION_TOOLS - names
        assert not missing, f"missing lazy-action tools: {missing}"

    def test_lazy_action_descriptions_follow_3_layer_structure(self, lazy_action_server):
        tools = {t["name"]: t for t in _tools_list(lazy_action_server.mcp_url())}
        for name in LAZY_ACTION_TOOLS:
            _assert_description_structure(name, tools[name].get("description", ""))

    def test_lazy_action_param_descriptions_under_cap(self, lazy_action_server):
        tools = {t["name"]: t for t in _tools_list(lazy_action_server.mcp_url())}
        for name in LAZY_ACTION_TOOLS:
            _assert_param_descriptions(tools[name])
