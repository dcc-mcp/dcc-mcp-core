"""Exercise the real Core HTTP facade with a clearly labeled contract fixture.

This does not connect a vendor host or certify native design acceptance.
"""

from __future__ import annotations

import asyncio
import importlib.util
from pathlib import Path

import pytest

from dcc_mcp_core.design_bridge import DesignSessionBridge


class ContractSession:
    """Provide deterministic upstream data for an isolated transport test."""

    async def list_tools(self, cursor=None):
        return {
            "tools": [
                {
                    "name": "fixture_write",
                    "description": "Contract fixture only.",
                    "inputSchema": {"type": "object", "properties": {"label": {"type": "string"}}},
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                }
            ]
        }

    async def call_tool(self, name, arguments=None):
        return {
            "content": [{"type": "text", "text": arguments["label"]}],
            "structuredContent": {"fixture": True},
            "isError": False,
            "_meta": {"fixture": "host_free_contract"},
        }


def _facade_module():
    source = Path(__file__).resolve().parents[1] / "examples" / "design-provider-bridge" / "facade.py"
    spec = importlib.util.spec_from_file_location("design_provider_facade_test", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_real_core_http_keeps_upstream_result_and_rejects_stale_calls(tmp_path):
    """Prove actual Core dispatch preserves the nested result and generation."""
    pytest.importorskip("mcp", reason="The example's optional official SDK is not a Core dependency.")
    from mcp import ClientSession
    from mcp.client.streamable_http import streamable_http_client

    async def exercise():
        bridge = DesignSessionBridge("penpot")
        await bridge.connect(ContractSession())
        facade = _facade_module().create_facade(bridge, asyncio.get_running_loop(), tmp_path)
        handle = facade.start()
        try:
            async with streamable_http_client(handle.mcp_url()) as (read, write, _):
                async with ClientSession(read, write) as session:
                    await session.initialize()
                    discovery = await session.call_tool("design_session__capabilities", {})
                    catalogue = discovery.structuredContent["context"]
                    assert catalogue["tools"][0]["annotations"]["destructiveHint"] is True
                    generation = catalogue["generation"]
                    refreshed = await session.call_tool("design_session__refresh", {"generation": generation})
                    current_generation = refreshed.structuredContent["context"]["generation"]
                    obsolete = await session.call_tool(
                        "design_session__call",
                        {"name": "fixture_write", "generation": generation, "arguments": {}},
                    )
                    assert obsolete.isError is True
                    assert obsolete.structuredContent["context"]["dispatched"] is False
                    generation = current_generation
                    response = await session.call_tool(
                        "design_session__call",
                        {"name": "fixture_write", "generation": generation, "arguments": {"label": "Editable fixture"}},
                    )
                    context = response.structuredContent["context"]
                    assert context["upstream_result"]["content"][0]["text"] == "Editable fixture"
                    assert context["upstream_result"]["_meta"] == {"fixture": "host_free_contract"}
                    assert context["native_acceptance"] == "unverified"
                    bridge.disconnect()
                    stale = await session.call_tool(
                        "design_session__call", {"name": "fixture_write", "generation": generation, "arguments": {}}
                    )
                    assert stale.isError is True
                    assert stale.structuredContent["context"]["dispatched"] is False
        finally:
            handle.shutdown()

    asyncio.run(exercise())
