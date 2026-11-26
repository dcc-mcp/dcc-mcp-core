"""Protocol smoke tests for the Roblox Studio stdio MCP example."""

from __future__ import annotations

import asyncio
import importlib.util
import json
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / "examples" / "roblox-studio" / "server.py"
SPEC = importlib.util.spec_from_file_location("roblox_studio_example_server", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
SERVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SERVER)


class _FakeBridge:
    def list_sessions(self):
        return [{"instance_id": "studio-1", "ready": True}]

    def call(self, method, *, target_instance_id=None, **params):
        return {
            "method": method,
            "target_instance_id": target_instance_id,
            "params": params,
        }


def _captured_response(capsys):
    return json.loads(capsys.readouterr().out)


def test_initialize_and_tools_list(capsys) -> None:
    bridge = _FakeBridge()

    asyncio.run(SERVER.handle_mcp_message(bridge, {"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
    initialized = _captured_response(capsys)
    assert initialized["result"]["serverInfo"]["name"] == "dcc-mcp-roblox-studio"

    asyncio.run(SERVER.handle_mcp_message(bridge, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
    listed = _captured_response(capsys)
    tool_names = {tool["name"] for tool in listed["result"]["tools"]}
    assert {"roblox_list_studios", "roblox_multi_edit"}.issubset(tool_names)


def test_tools_call_maps_studio_id_to_core_instance_id(capsys) -> None:
    bridge = _FakeBridge()

    asyncio.run(
        SERVER.handle_mcp_message(
            bridge,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "roblox_get_studio_state",
                    "arguments": {"studio_id": "studio-1"},
                },
            },
        )
    )
    response = _captured_response(capsys)
    payload = json.loads(response["result"]["content"][0]["text"])
    assert payload == {
        "method": "roblox.get_studio_state",
        "target_instance_id": "studio-1",
        "params": {},
    }


def test_list_studios_exposes_core_instance_as_studio_id(capsys) -> None:
    asyncio.run(
        SERVER.handle_mcp_message(
            _FakeBridge(),
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "roblox_list_studios"}},
        )
    )
    response = _captured_response(capsys)
    payload = json.loads(response["result"]["content"][0]["text"])
    assert payload["studios"][0]["studio_id"] == "studio-1"
