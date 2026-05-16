"""Regression tests for issue #993 — search_tools MUST include unloaded skill actions.

#993 is a re-occurrence of #858. The contract under test:

  In default minimal mode (typical agent boot state) most domain skills
  appear only as ``__skill__*`` stubs in ``tools/list``. The
  ``search_tools`` (and gateway ``POST /v1/search``) surface MUST still
  return per-action hits for those *unloaded* skills, with
  ``loaded: false`` and a load-hint, so agents can complete the canonical
  discover → ``load_skill`` → ``call_tool`` flow without first scraping
  ``list_skills``.

These tests boot an :class:`McpHttpServer` against the bundled
``examples/skills`` directory in **minimal mode** (no domain skill
loaded) and assert ``search_tools`` returns hits for actions that
demonstrably belong to an unloaded skill. The same contract is exercised
through HTTP by ``tests/vrs/traces/core-993-search-tools-includes-unloaded.jsonl``
when a real backend is available.

Will fail RED while #993 is open; turns GREEN once the search index
unions stub-mode skill actions with loaded ones.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path
import time

# Import third-party modules
import pytest

# Import local modules
from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS = REPO_ROOT / "examples" / "skills"

# Skill that ships actions but is NOT auto-loaded in minimal mode. Two
# action names from its tools.yaml that the agent should be able to find
# without first calling load_skill.
UNLOADED_SKILL = "multi-script"
UNLOADED_ACTIONS = ("multi_script__action_python",)


@pytest.fixture(scope="module")
def minimal_server():
    """Start a server with skills discovered but none loaded (minimal mode)."""
    if not EXAMPLES_SKILLS.is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ToolRegistry()
    cfg = McpHttpConfig(port=0, server_name="ci-regression-993")
    server = McpHttpServer(reg, cfg)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


def _call_search_tools(url: str, query: str, limit: int = 25) -> list[dict]:
    """Invoke ``search_tools`` MCP meta-tool and return the parsed hits list."""
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "search_tools",
            "arguments": {"query": query, "limit": limit},
        },
    }
    resp = McpClient(url).post(body)[1]
    text = resp["result"]["content"][0]["text"]
    return json.loads(text).get("hits", [])


# ── Contract: search_tools surfaces unloaded skill actions ──


class TestRegression993SearchToolsIncludesUnloaded:
    """Pin the unloaded-skill discovery contract from #858 so it cannot regress again."""

    def test_search_tools_returns_hit_for_unloaded_action(self, minimal_server):
        """A query naming an action from an *unloaded* skill MUST return
        at least one hit. Without this, the discover→load→call flow
        breaks and agents fall back to ``list_skills`` (which causes the
        bloat tracked under #995).
        """
        url = minimal_server.mcp_url()
        target = UNLOADED_ACTIONS[0]
        hits = _call_search_tools(url, query=target.split("__", 1)[-1], limit=25)
        action_names = [h.get("backend_tool") for h in hits]
        assert target in action_names, (
            f"search_tools missed unloaded action {target!r} — regression of #858 / #993. "
            f"Got {len(hits)} hits: {action_names}"
        )

    def test_unloaded_hit_marks_loaded_false_and_carries_load_hint(self, minimal_server):
        """The unloaded hit must explicitly carry ``loaded: false`` so
        agents know to call ``load_skill`` before ``call_tool``. A
        ``requires_load_skill`` flag or ``load_hint`` mapping is the
        documented contract on the gateway capability index — surfacing
        the same shape locally keeps clients consistent across local
        and gateway-mediated transports.
        """
        url = minimal_server.mcp_url()
        target = UNLOADED_ACTIONS[0]
        hits = _call_search_tools(url, query=target.split("__", 1)[-1], limit=25)
        match = next((h for h in hits if h.get("backend_tool") == target), None)
        if match is None:
            pytest.fail(f"Prerequisite missing: {target!r} not in search hits — see sibling test")

        assert match.get("loaded") is False, (
            "Unloaded action surfaced with loaded != false; agents will skip the "
            f"necessary load_skill step. Hit: {match!r}"
        )
