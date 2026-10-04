"""Expose one caller-owned upstream session through Core's existing server.

This is a small JSON facade, not an implementation of a vendor MCP server.
Core's outer result envelope contains the complete ``upstream_result``.
"""

from __future__ import annotations

import asyncio
import json
import signal
from tempfile import TemporaryDirectory

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.experimental.design_bridge import DesignBridgeError
from dcc_mcp_core.result_envelope import ToolResultEnvelope


def create_facade(bridge, loop, registry_dir, *, port=0):
    """Register before starting; keep the official session on its owning loop."""
    config = McpHttpConfig(port=port, server_name="official-design-session", enable_cors=False)
    config.host = "127.0.0.1"
    config.gateway_port = 0
    config.registry_dir = str(registry_dir)
    config.dcc_type = "design-" + bridge.profile.provider
    config.instance_metadata = {"dcc_mcp_instance_type": "standalone", "provider": bridge.profile.provider}
    server = create_skill_server(config.dcc_type, config, accumulated=False)

    async def invoke(operation, params):
        try:
            if isinstance(params, str):
                params = json.loads(params)
            if not isinstance(params, dict):
                return ToolResultEnvelope.invalid_input("Arguments must be an object.").to_dict()
            if operation == "status":
                return ToolResultEnvelope.ok("Observed bridge status.", **bridge.status()).to_dict()
            if operation == "capabilities":
                return ToolResultEnvelope.ok("Observed upstream descriptors.", **bridge.capabilities()).to_dict()
            if operation == "refresh":
                catalogue = await bridge.refresh_tools(expected_generation=params["generation"])
                return ToolResultEnvelope.ok("Refreshed upstream descriptors.", **catalogue).to_dict()
            if operation == "describe":
                description = bridge.describe_tool(params["name"], expected_generation=params["generation"])
                return ToolResultEnvelope.ok("Exact upstream descriptor.", tool=description).to_dict()
            result = await bridge.call_tool(
                params["name"],
                params.get("arguments", {}),
                expected_generation=params["generation"],
            )
            return result.to_dict()
        except DesignBridgeError as exc:
            return exc.to_dict()
        except (KeyError, TypeError, ValueError):
            return ToolResultEnvelope.invalid_input(
                "Use the current discovered name, generation and object arguments."
            ).to_dict()

    def handler(operation):
        def dispatch(params):
            # The SDK owns timeouts. Never cancel/replay a mutation because a
            # downstream client disconnected. Core may dispatch this as a job.
            return asyncio.run_coroutine_threadsafe(invoke(operation, params), loop).result()

        return dispatch

    empty_schema = {"type": "object", "properties": {}, "additionalProperties": False}
    target_schema = {
        "type": "object",
        "properties": {"name": {"type": "string"}, "generation": {"type": "integer", "minimum": 0}},
        "required": ["name", "generation"],
        "additionalProperties": False,
    }
    call_schema = {
        "type": "object",
        "properties": {**target_schema["properties"], "arguments": {"type": "object"}},
        "required": ["name", "generation"],
        "additionalProperties": False,
    }
    definitions = (
        (
            "status",
            "Observe connection/catalog status; authentication and native effects remain unverified.",
            empty_schema,
        ),
        (
            "capabilities",
            "Read exact live upstream schemas, annotations and metadata; inspect before invocation.",
            empty_schema,
        ),
        (
            "refresh",
            "Refresh the official catalog before edits; advances generation and invalidates earlier descriptors.",
            {
                "type": "object",
                "properties": {"generation": {"type": "integer", "minimum": 0}},
                "required": ["generation"],
                "additionalProperties": False,
            },
        ),
        ("describe", "Read one exact upstream descriptor from the specified discovery generation.", target_schema),
        (
            "call",
            "Invoke the named official operation. Treat as mutating; upstream annotations are hints, not authority.",
            call_schema,
        ),
    )
    specs = [
        ToolSpec(
            name="design_session__" + name,
            description=description,
            input_schema=schema,
            handler=handler(name),
            category="pipeline",
            tags=["design", "experimental", bridge.profile.provider],
        )
        for name, description, schema in definitions
    ]
    if register_tools(server, specs, dcc_name=config.dcc_type) != len(specs):
        raise RuntimeError("The complete facade tool surface could not be registered.")
    return server


async def serve_bridge(bridge, *, port=0, registry_dir=None):
    """Run a loopback trial without joining or restarting a production gateway."""
    loop = asyncio.get_running_loop()
    stop = asyncio.Event()
    previous_handlers = {}

    def request_stop(_signal_number, _frame):
        loop.call_soon_threadsafe(stop.set)

    with TemporaryDirectory(prefix="dcc-design-facade-") as trial_registry:
        server = create_facade(bridge, loop, registry_dir or trial_registry, port=port)
        handle = server.start()
        try:
            for number in (signal.SIGINT, signal.SIGTERM):
                previous_handlers[number] = signal.signal(number, request_stop)
            print(
                json.dumps(
                    {"mcp_url": handle.mcp_url(), "provider": bridge.profile.provider, "acceptance": "unverified"}
                )
            )
            await stop.wait()
        finally:
            handle.shutdown()
            for number, previous in previous_handlers.items():
                signal.signal(number, previous)
