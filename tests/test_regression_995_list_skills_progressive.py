"""Regression tests for issue #995 — list_skills MUST be progressive.

#995 is a re-occurrence of #582. The contract under test:

  ``list_skills`` is the discovery-side dual of progressive loading. It
  MUST honour ``limit``, ``offset``, and a ``fields`` selector so agents
  can pull a small, on-budget slice instead of a 25 KB blob. Default
  ``fields`` MUST drop the heavy fields (full ``description``,
  ``search_hint``, every ``tool_name``) so even a no-args call stays
  under a small payload budget.

These tests boot an :class:`McpHttpServer` against
``examples/skills`` in minimal mode and lock the contract end-to-end.
The same scenarios are exercised over HTTP through
``tests/vrs/traces/core-995-list-skills-respects-limit.jsonl``.

Will fail RED while #995 is open. Once the response shape adds
``limit`` / ``offset`` and ``fields`` defaults to a compact projection,
all four tests turn GREEN automatically.
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

# Compact-mode default budget. Tuned to be ~2.5x the size of a small
# skill summary (name + tool_count + stage + first-sentence summary).
# A 25-skill backend should fit in this budget when fields are minimal.
DEFAULT_PAYLOAD_BUDGET_BYTES = 8 * 1024


@pytest.fixture(scope="module")
def discovery_server():
    if not EXAMPLES_SKILLS.is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ToolRegistry()
    cfg = McpHttpConfig(port=0, server_name="ci-regression-995")
    server = McpHttpServer(reg, cfg)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


def _call_list_skills(url: str, arguments: dict) -> dict:
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "list_skills", "arguments": arguments},
    }
    resp = McpClient(url).post(body)[1]
    text = resp["result"]["content"][0]["text"]
    return json.loads(text), text


# ── Contract: list_skills is progressive ──


class TestRegression995ListSkillsProgressive:
    """Pin the progressive-discovery contract so the bloat from #582 cannot re-land."""

    def test_default_response_stays_under_payload_budget(self, discovery_server):
        """A no-arguments ``list_skills`` call MUST stay under the
        compact-mode budget. If your fix changes the shape of
        ``SkillSummary`` to a fields-projected default, this test will
        pass; if you keep returning the full multi-line description for
        every skill, it fails (the #995 / #582 symptom).
        """
        url = discovery_server.mcp_url()
        _, raw = _call_list_skills(url, arguments={})
        assert len(raw.encode("utf-8")) < DEFAULT_PAYLOAD_BUDGET_BYTES, (
            "list_skills default response is {} bytes (budget {}). Regression of #582 / #995. "
            "Reduce default per-skill projection (drop multi-line description, search_hint, full tool_names) "
            "or default `fields` to a compact projection.".format(
                len(raw.encode("utf-8")), DEFAULT_PAYLOAD_BUDGET_BYTES
            )
        )

    def test_limit_argument_caps_returned_skill_count(self, discovery_server):
        """``limit`` MUST be honoured. Agents in CI today cannot opt
        into a smaller slice because the schema does not declare it; the
        fix lands when this passes.
        """
        url = discovery_server.mcp_url()
        payload, _ = _call_list_skills(url, arguments={"limit": 2})
        skills = payload.get("skills") or []
        assert len(skills) <= 2, (
            f"list_skills returned {len(skills)} skills despite limit=2 — regression of #995. "
            "Server must honour the limit argument."
        )

    def test_offset_skips_initial_skills(self, discovery_server):
        """``offset`` MUST work for cursor-style pagination."""
        url = discovery_server.mcp_url()
        page_a, _ = _call_list_skills(url, arguments={"limit": 2, "offset": 0})
        page_b, _ = _call_list_skills(url, arguments={"limit": 2, "offset": 2})
        names_a = [s.get("name") for s in (page_a.get("skills") or [])]
        names_b = [s.get("name") for s in (page_b.get("skills") or [])]
        assert names_a and names_b, "Both pages must be non-empty for the offset assertion to be meaningful"
        assert set(names_a).isdisjoint(set(names_b)), (
            "list_skills(limit=2, offset=2) returned overlapping skill names with offset=0 — "
            "offset is not honoured. Regression of #995."
        )

    def test_fields_selector_drops_heavy_fields(self, discovery_server):
        """``fields=["name"]`` MUST return ONLY the requested fields per
        skill. If the server still ships ``description`` / ``search_hint``
        / ``tool_names`` regardless of ``fields``, the projection
        contract is not implemented.
        """
        url = discovery_server.mcp_url()
        payload, _ = _call_list_skills(url, arguments={"limit": 1, "fields": ["name"]})
        skills = payload.get("skills") or []
        if not skills:
            pytest.fail("list_skills returned zero skills — discovery prerequisite failed")

        leaked = set()
        for entry in skills:
            for k in ("description", "search_hint", "tool_names", "tags"):
                if k in entry:
                    leaked.add(k)
        assert not leaked, (
            f"list_skills(fields=['name']) leaked unrequested heavy fields {leaked} — "
            "regression of #995. The fields selector must be a strict allow-list."
        )
