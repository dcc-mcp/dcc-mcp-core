"""Select the opt-in B3 input contract without widening legacy canvas actions."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_game_input import GAME_ACTIONS
from dcc_mcp_core.host.cua_mcp_game_input import game_input_from_params
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions


def prepare(params: dict[str, Any]) -> dict[str, Any] | None:
    """Validate new requests before opening a task; return None for legacy actions."""
    action = params.get("action")
    if not isinstance(action, str):
        raise CuaCliError("invalid_action", "An action string is required.")
    runtime = params.get("trusted_ui_control_runtime")
    if action in GAME_ACTIONS and isinstance(runtime, UiControlRuntimeOptions):
        return game_input_from_params(params, runtime.allowed_actions)
    if (
        action == "relative_mouse"
        or params.get("intent") == "game_navigation"
        or (action == "game_navigation" and ("dx" in params or "dy" in params))
    ):
        raise CuaCliError("unsupported_action", "B3 game input requires the owner-selected pixels MCP transport.")
    return None
