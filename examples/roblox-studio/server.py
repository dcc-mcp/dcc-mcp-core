"""Small stdio MCP server for the Roblox Studio bridge example."""

from __future__ import annotations

import argparse
import asyncio
import functools
import json
import sys
from typing import Any

from dcc_mcp_core import DccBridge

SERVER_VERSION = "0.1.0"


TOOLS = [
    {
        "name": "roblox_list_studios",
        "description": "List connected Roblox Studio plugin sessions.",
        "inputSchema": {"type": "object", "properties": {}},
        "annotations": {"readOnlyHint": True, "idempotentHint": True},
        "method": None,
    },
    {
        "name": "roblox_get_studio_state",
        "description": "Read the connected Roblox Studio state.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "studio_id": {"type": "string"},
                "instance_id": {"type": "string"},
            },
        },
        "annotations": {"readOnlyHint": True, "idempotentHint": True},
        "method": "roblox.get_studio_state",
    },
    {
        "name": "roblox_search_game_tree",
        "description": "Search the connected Studio data model by path, class, or name.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "root": {"type": "string", "default": "game"},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 500},
                "studio_id": {"type": "string"},
                "instance_id": {"type": "string"},
            },
            "required": ["query"],
        },
        "annotations": {"readOnlyHint": True, "idempotentHint": True},
        "method": "roblox.search_game_tree",
    },
    {
        "name": "roblox_inspect_instance",
        "description": "Inspect one Roblox Studio instance and its immediate children.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "studio_id": {"type": "string"},
                "instance_id": {"type": "string"},
            },
            "required": ["path"],
        },
        "annotations": {"readOnlyHint": True, "idempotentHint": True},
        "method": "roblox.inspect_instance",
    },
    {
        "name": "roblox_script_read",
        "description": "Read the source of a Script, LocalScript, or ModuleScript.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "studio_id": {"type": "string"},
                "instance_id": {"type": "string"},
            },
            "required": ["path"],
        },
        "annotations": {"readOnlyHint": True, "idempotentHint": True},
        "method": "roblox.script_read",
    },
    {
        "name": "roblox_multi_edit",
        "description": "Apply bounded script edits as one ChangeHistoryService recording.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 32,
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "new_source": {"type": "string", "maxLength": 1000000},
                        },
                        "required": ["path", "new_source"],
                    },
                },
                "studio_id": {"type": "string"},
                "instance_id": {"type": "string"},
            },
            "required": ["edits"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "method": "roblox.multi_edit",
    },
]


def mcp_response(request_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def mcp_error(request_id: Any, code: int, message: str) -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": code, "message": message},
    }


def _take_studio_target(arguments: dict[str, Any]) -> str | None:
    """Resolve the official Roblox ``studio_id`` alias to core's instance ID."""
    instance_id = arguments.pop("instance_id", None)
    studio_id = arguments.pop("studio_id", None)
    if instance_id and studio_id and instance_id != studio_id:
        raise ValueError("instance_id and studio_id must identify the same Studio session")
    return studio_id or instance_id


def _studio_snapshots(bridge: DccBridge) -> list[dict[str, Any]]:
    """Expose core session metadata with Roblox's public studio_id spelling."""
    snapshots = []
    for session in bridge.list_sessions():
        snapshot = dict(session)
        snapshot["studio_id"] = snapshot.get("instance_id")
        snapshots.append(snapshot)
    return snapshots


async def _call_in_thread(function, *args, **kwargs):
    """Run bridge I/O on a worker on Python 3.7 and newer."""
    to_thread = getattr(asyncio, "to_thread", None)
    if to_thread is not None:
        return await to_thread(function, *args, **kwargs)
    loop = asyncio.get_event_loop()
    call = functools.partial(function, *args, **kwargs)
    return await loop.run_in_executor(None, call)


async def handle_mcp_message(bridge: DccBridge, message: dict[str, Any]) -> None:
    request_id = message.get("id")
    if request_id is None:
        return
    method = message.get("method")
    if method == "initialize":
        result = {
            "protocolVersion": "2025-03-26",
            "serverInfo": {"name": "dcc-mcp-roblox-studio", "version": SERVER_VERSION},
            "capabilities": {"tools": {}},
        }
        print(json.dumps(mcp_response(request_id, result)), flush=True)
        return
    if method == "notifications/initialized":
        return
    if method == "tools/list":
        visible_tools = [{key: value for key, value in tool.items() if key != "method"} for tool in TOOLS]
        print(json.dumps(mcp_response(request_id, {"tools": visible_tools})), flush=True)
        return
    if method != "tools/call":
        print(json.dumps(mcp_error(request_id, -32601, f"Unknown MCP method: {method}")), flush=True)
        return

    params = message.get("params", {})
    tool_name = params.get("name")
    tool = next((item for item in TOOLS if item["name"] == tool_name), None)
    if tool is None:
        print(json.dumps(mcp_error(request_id, -32601, f"Unknown tool: {tool_name}")), flush=True)
        return
    try:
        arguments = dict(params.get("arguments", {}))
        if tool_name == "roblox_list_studios":
            result = {"studios": _studio_snapshots(bridge)}
        else:
            target_instance_id = _take_studio_target(arguments)
            result = await _call_in_thread(
                bridge.call,
                tool["method"],
                target_instance_id=target_instance_id,
                **arguments,
            )
        content = [{"type": "text", "text": json.dumps(result, sort_keys=True)}]
        print(json.dumps(mcp_response(request_id, {"content": content, "isError": False})), flush=True)
    except Exception as exc:
        content = [{"type": "text", "text": str(exc)}]
        print(json.dumps(mcp_response(request_id, {"content": content, "isError": True})), flush=True)


async def run(args: argparse.Namespace) -> None:
    bridge = DccBridge(
        host=args.host,
        port=args.ws_port,
        server_name="dcc-mcp-roblox-studio",
        server_version=SERVER_VERSION,
    )
    bridge.connect()
    try:
        while True:
            line = await asyncio.to_thread(sys.stdin.readline)
            if not line:
                break
            try:
                message = json.loads(line)
                await handle_mcp_message(bridge, message)
            except json.JSONDecodeError as exc:
                print(json.dumps(mcp_error(None, -32700, str(exc))), flush=True)
    finally:
        bridge.disconnect()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--ws-port", type=int, default=9001)
    asyncio.run(run(parser.parse_args()))


if __name__ == "__main__":
    main()
