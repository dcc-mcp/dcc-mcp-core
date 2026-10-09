"""Exact opt-in B3 capability and public action-scope negotiation."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_contracts import typed_equal as _typed_equal
from dcc_mcp_core.host.cua_mcp_game_input import GAME_ACTIONS

GAME_CAPABILITY_KEY = "dcc-cua.game-input"
GAME_CONTRACT = "dcc-cua.game-navigation.b3.v1"
GAME_DESCRIPTOR = {
    "contract": GAME_CONTRACT,
    "profile": "game_b3.v1",
    "platform": "windows",
    "observation_mode": "pixels_only",
    "intent": "game_navigation",
    "input_kind": "raw_input",
    "delivery_mode": "foreground",
    "actions": ["game_navigation", "relative_mouse"],
    "limits": {
        "min_keys": 1,
        "max_keys": 4,
        "min_duration_ms": 0,
        "max_duration_ms": 500,
        "omitted_duration_ms": 0,
        "max_abs_relative_delta": 256,
    },
    "shape": {
        "canonical_keys_only": True,
        "null_duration_allowed": False,
        "game_optional_paired_delta": True,
        "relative_pair_required": True,
        "zero_delta_allowed": False,
        "relative_buttonless": True,
        "standalone_relative_packets": 1,
    },
    "combined_requires": ["game_navigation", "relative_mouse"],
}


def require_game_capability(capabilities: dict[str, Any], grants: tuple[str, ...]) -> None:
    """Require the exact typed descriptor only when the owner selected new scopes."""
    if not GAME_ACTIONS.intersection(grants):
        return
    experimental = capabilities.get("experimental")
    if not isinstance(experimental, dict) or not _typed_equal(experimental.get(GAME_CAPABILITY_KEY), GAME_DESCRIPTOR):
        raise CuaCliError("unsupported", "The selected runtime does not advertise the exact B3 game input contract.")


def require_game_scope_schema(tools: list[Any], grants: tuple[str, ...]) -> None:
    """Check the pixels-only action branch, not an unrelated semantic enum."""
    requested = GAME_ACTIONS.intersection(grants)
    if not requested:
        return
    try:
        supported = _supported_game_scopes(tools)
    except (AttributeError, TypeError):
        supported = set()
    if not requested.issubset(supported):
        raise CuaCliError("unsupported", "The selected runtime does not advertise the requested pixels game scopes.")


def _supported_game_scopes(tools: list[Any]) -> set[str]:
    schemas = [t.get("inputSchema") for t in tools if isinstance(t, dict) and t.get("name") == "start_task"]
    supported: set[str] = set()
    if len(schemas) == 1 and isinstance(schemas[0], dict):
        for conditional in schemas[0].get("allOf", []):
            if not isinstance(conditional, dict) or conditional.get("if") != {
                "properties": {"observation_mode": {"const": "pixels_only"}},
                "required": ["observation_mode"],
            }:
                continue
            properties = conditional.get("then", {}).get("properties", {})
            branches = properties.get("allowed_actions", {}).get("items", {}).get("oneOf", [])
            for branch in branches:
                if not isinstance(branch, dict):
                    continue
                fields = branch.get("properties", {})
                required = branch.get("required", [])
                if (
                    branch.get("type") != "object"
                    or branch.get("additionalProperties") is not False
                    or not isinstance(required, list)
                    or not {"action", "input_kind", "secret_input", "authorization_category"}.issubset(required)
                    or fields.get("input_kind") != {"const": "raw_input"}
                    or fields.get("authorization_category") != {"const": "raw_input"}
                    or not _typed_equal(fields.get("secret_input"), {"const": False})
                ):
                    continue
                action = fields.get("action", {})
                values = action.get("enum", [action.get("const")]) if isinstance(action, dict) else []
                if isinstance(values, list):
                    supported.update(value for value in values if isinstance(value, str))
    return supported
