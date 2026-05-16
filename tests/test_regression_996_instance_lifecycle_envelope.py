"""Regression tests for issue #996 — surface DCC instance lifecycle cause.

The contract under test:

  When an agent gets an ``instance-offline`` error, the envelope MUST
  carry enough provenance for the agent to know **why** the instance
  is no longer routable, so it does not invent a "DCC crashed" narrative
  for what was actually a graceful operator restart or a heartbeat
  timeout. Concretely:

    - Error envelope carries ``error.previous_status`` ∈ {"deregistered",
      "heartbeat-timeout", "never-registered"}.
    - Optionally carries ``error.previous_instance_id`` so agents can
      reconcile against earlier responses.

This is documentation + small protocol surface, not a behavioural bug.
The test simply locks the field shape so any future implementation
choice (whether short labels, longer enums, or a structured object)
includes the *cause* axis at all.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
import time

# Import third-party modules
import pytest

# Import local modules
from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


@pytest.fixture(scope="module")
def empty_server():
    """Create an MCP server with no DCC backends registered.

    Any describe / call against a fabricated DCC instance slug should
    return a structured envelope. We don't care here whether the kind
    is ``unknown-slug`` or ``instance-offline`` — we only assert the
    *previous_status* axis exists once a real implementation lands.
    """
    reg = ToolRegistry()
    cfg = McpHttpConfig(port=0, server_name="ci-regression-996")
    server = McpHttpServer(reg, cfg)
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


def _call_describe(url: str, slug: str) -> dict:
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "describe_tool", "arguments": {"tool_slug": slug}},
    }
    resp = McpClient(url).post(body)[1]
    return resp


# ── Contract: instance-offline envelope must carry cause ──


class TestRegression996InstanceLifecycleEnvelope:
    """Document-and-pin: ``instance-offline`` MUST carry provenance fields."""

    @pytest.mark.parametrize(
        "fabricated_slug",
        [
            "maya.deadbeef.maya_scene__list_objects",
            "blender.cafefade.bpy_scene__list_objects",
        ],
    )
    def test_describe_offline_envelope_carries_previous_status(self, empty_server, fabricated_slug):
        """Calling describe for a non-existent instance MUST return an
        error envelope that names the cause. Until #996 is implemented
        the gateway only returns ``unknown-slug`` with no provenance —
        that is the regression footprint.
        """
        url = empty_server.mcp_url()
        resp = _call_describe(url, fabricated_slug)

        # The envelope shape varies between MCP error JSON-RPC and the
        # in-content error tool envelope. Walk both and locate the
        # error block.
        error_block = None
        if "error" in resp:
            error_block = resp["error"]
        elif "result" in resp:
            content = (resp["result"].get("content") or [{}])[0]
            text = content.get("text", "")
            if text:
                try:
                    inner = json.loads(text)
                    error_block = inner.get("error") or inner
                except json.JSONDecodeError:
                    error_block = {"raw": text}
        assert error_block, f"describe_tool against a fabricated slug returned no error block: {resp!r}"

        # The acceptance criterion: a previous_status field exists
        # somewhere in the envelope. We accept both top-level and
        # nested-under-error placements to be lenient about final
        # protocol shape.
        flat = json.dumps(error_block)
        assert "previous_status" in flat, (
            "instance-offline / unknown-slug envelope is missing `previous_status` — "
            f"regression / not-yet-implemented for #996. Envelope: {error_block!r}"
        )
