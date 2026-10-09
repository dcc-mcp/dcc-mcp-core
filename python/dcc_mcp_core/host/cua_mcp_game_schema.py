"""Pinned B3 execute_action schema branch consumed during MCP negotiation."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_contracts import typed_equal as _typed_equal
from dcc_mcp_core.host.cua_mcp_game_input import GAME_ACTIONS
from dcc_mcp_core.host.cua_mcp_game_input import GAME_KEYS


def game_call_condition() -> dict[str, Any]:
    """Return the closed action branch promised by the B3 protocol identity."""
    delta = {"type": "integer", "minimum": -256, "maximum": 256}
    context = {
        "input_kind": {"const": "raw_input"},
        "intent": {"const": "game_navigation"},
        "delivery_mode": {"const": "foreground"},
    }
    common = ["action", "input_kind", "intent", "delivery_mode"]
    zero = {"properties": {"dx": {"const": 0}, "dy": {"const": 0}}}
    actions = {
        "oneOf": [
            {
                "type": "object",
                "additionalProperties": False,
                "required": [*common, "keys"],
                "properties": {
                    "action": {"const": "game_navigation"},
                    **context,
                    "keys": {
                        "type": "array",
                        "items": {"enum": list(GAME_KEYS)},
                        "minItems": 1,
                        "maxItems": 4,
                        "uniqueItems": True,
                    },
                    "duration_ms": {"type": "integer", "minimum": 0, "maximum": 500, "default": 0},
                    "dx": dict(delta),
                    "dy": dict(delta),
                },
                "dependentRequired": {"dx": ["dy"], "dy": ["dx"]},
                "not": {"required": ["dx", "dy"], **zero},
            },
            {
                "type": "object",
                "additionalProperties": False,
                "required": [*common, "dx", "dy"],
                "properties": {"action": {"const": "relative_mouse"}, **context, "dx": dict(delta), "dy": dict(delta)},
                "not": zero,
            },
        ]
    }
    return {
        "if": {
            "required": ["method", "params"],
            "properties": {
                "method": {"const": "execute_action"},
                "params": {
                    "required": ["action"],
                    "properties": {
                        "action": {
                            "required": ["action"],
                            "properties": {
                                "action": {"enum": ["game_navigation", "relative_mouse"]},
                            },
                        },
                    },
                },
            },
        },
        "then": {
            "properties": {
                "params": {
                    "type": "object",
                    "additionalProperties": False,
                    "required": ["observation_id", "action"],
                    "properties": {
                        "observation_id": {"type": "string", "minLength": 1},
                        "action": actions,
                        "capture_after": {"type": "boolean"},
                    },
                }
            }
        },
    }


def require_game_action_schema(tools: list[Any], grants: tuple[str, ...]) -> None:
    """Reject missing, malformed or incompatible game action branches before start_task."""
    if not GAME_ACTIONS.intersection(grants):
        return
    schemas = [t.get("inputSchema") for t in tools if isinstance(t, dict) and t.get("name") == "dcc_cua_task_call"]
    if len(schemas) == 1 and isinstance(schemas[0], dict):
        conditions = schemas[0].get("allOf")
        if isinstance(conditions, list) and any(_typed_equal(item, game_call_condition()) for item in conditions):
            return
    raise CuaCliError("unsupported", "The selected runtime lacks the exact closed B3 execute_action schema.")
