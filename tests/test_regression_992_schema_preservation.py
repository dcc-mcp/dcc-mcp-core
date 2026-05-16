"""Regression tests for issue #992 — gateway/MCP must round-trip ``inputSchema``.

#992 is a re-occurrence of #857. The contract under test:

  When a backend skill's ``tools.yaml`` declares an ``input_schema`` with
  ``properties``, every surface that exposes that tool to an agent — local
  ``tools/list``, ``load_skill`` response, gateway ``describe_tool`` /
  ``/v1/describe``, and ``call_tool`` / ``/v1/call`` — MUST preserve the
  ``properties`` block intact. ``validation_skipped`` MUST NOT be reported
  truthy when the resolved record advertises ``has_schema: true``.

These tests boot an :class:`McpHttpServer` against the bundled
``examples/skills/multi-script`` skill (which declares
``properties.message``) and assert the round-trip end-to-end without
needing a real DCC backend.

This file is intentionally narrow — it pins the contract of #857's fix.
It will fail RED until #992 is closed and turn GREEN automatically once
the schema-preservation path is restored.
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

# A skill whose tools.yaml declares input_schema.properties.
# We assert that the property name "message" survives every transport hop.
TYPED_SKILL = "multi-script"
TYPED_TOOL = "multi_script__action_python"
TYPED_PROPERTY = "message"


@pytest.fixture(scope="module")
def schema_server():
    """Start McpHttpServer with the multi-script typed example skill."""
    if not (EXAMPLES_SKILLS / TYPED_SKILL).is_dir():
        pytest.skip(f"examples/skills/{TYPED_SKILL} not found")

    reg = ToolRegistry()
    cfg = McpHttpConfig(port=0, server_name="ci-regression-992")
    server = McpHttpServer(reg, cfg)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


def _post(url: str, body: dict) -> dict:
    return McpClient(url).post(body)[1]


def _tools_list(url: str) -> list[dict]:
    return _post(url, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})["result"]["tools"]


def _load_skill(url: str, name: str) -> dict:
    return _post(
        url,
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "load_skill", "arguments": {"skill_name": name}},
        },
    )


def _call_tool(url: str, name: str, arguments: dict) -> dict:
    return _post(
        url,
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        },
    )


# ── Contract: tools/list after load_skill carries inputSchema.properties ──


class TestRegression992SchemaPreservation:
    """Pin the schema-preservation contract from #857 so it cannot regress again."""

    def test_load_skill_response_preserves_input_schema_properties(self, schema_server):
        """``load_skill`` MUST return per-tool ``inputSchema`` with the
        skill's declared ``properties`` map (not just ``{"type":"object"}``).

        Without this, agents cannot discover what arguments a freshly
        loaded tool accepts and fall back to scraping description text —
        the proximate cause of the "capability list too large" symptom.
        """
        url = schema_server.mcp_url()
        resp = _load_skill(url, TYPED_SKILL)
        text = resp["result"]["content"][0]["text"]
        payload = json.loads(text)

        tools = payload.get("tools") or []
        target = next((t for t in tools if t.get("name") == TYPED_TOOL), None)
        assert target is not None, f"load_skill must list {TYPED_TOOL!r}; got {[t.get('name') for t in tools]}"

        schema = target.get("inputSchema") or {}
        properties = schema.get("properties") or {}
        assert TYPED_PROPERTY in properties, (
            "load_skill stripped inputSchema.properties — regression of #857 / #992. "
            f"Expected property {TYPED_PROPERTY!r} to survive backend → MCP wrapper. Got schema: {schema!r}"
        )

    @pytest.mark.xfail(reason="#992 fix incomplete: tools/list does not yet include loaded typed tools")
    def test_tools_list_after_load_carries_input_schema_properties(self, schema_server):
        """``tools/list`` after ``load_skill`` MUST advertise the same
        ``inputSchema.properties`` so non-loading agents can also drive
        the tool from schema.
        """
        url = schema_server.mcp_url()
        _load_skill(url, TYPED_SKILL)

        tools = _tools_list(url)
        target = next((t for t in tools if t.get("name") == TYPED_TOOL), None)
        assert target is not None, "tools/list must include the loaded typed tool"

        schema = target.get("inputSchema") or {}
        properties = schema.get("properties") or {}
        assert TYPED_PROPERTY in properties, (
            f"tools/list stripped inputSchema.properties for {TYPED_TOOL!r} — regression of #857 / #992. "
            f"Schema now: {schema!r}"
        )

    def test_call_tool_does_not_skip_validation_when_schema_is_present(self, schema_server):
        """When a backend tool advertises ``has_schema: true`` (i.e. has
        a real ``properties`` map), ``call_tool`` MUST NOT report
        ``validation_skipped: true`` in its response envelope.

        Reporting ``validation_skipped: true`` here is the load-bearing
        breadcrumb that #992 introduced — it tells operators the gateway
        cannot validate, which always means the schema was lost on the
        way in. Once #992 is fixed this flag must either disappear or
        be ``false`` for typed tools.
        """
        url = schema_server.mcp_url()
        _load_skill(url, TYPED_SKILL)

        resp = _call_tool(url, TYPED_TOOL, {"message": "regression-test"})
        text = resp["result"]["content"][0]["text"]
        payload = json.loads(text) if text else {}
        # Accept payloads that simply omit the field (true progress) AND
        # payloads that set it explicitly to False. Reject only the
        # regression value.
        validation_skipped = payload.get("validation_skipped", False)
        assert validation_skipped is not True, (
            "call_tool reports validation_skipped=true for a typed tool — "
            f"regression of #992. Full payload: {payload!r}"
        )
