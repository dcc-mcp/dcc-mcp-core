"""Closed B3 input DTOs; physical delivery and release remain Native-owned."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError

GAME_ACTIONS = frozenset({"game_navigation", "relative_mouse"})
GAME_KEYS = ("W", "A", "S", "D", "SPACE", "LSHIFT", "LCTRL", "Z", "E", "F", "M")
_ALIASES = {
    "SHIFT": "LSHIFT",
    "LEFTSHIFT": "LSHIFT",
    "CTRL": "LCTRL",
    "CONTROL": "LCTRL",
    "LEFTCTRL": "LCTRL",
    "LEFTCONTROL": "LCTRL",
}
_CONTEXT = {"input_kind": "raw_input", "intent": "game_navigation", "delivery_mode": "foreground"}
_COMMON = frozenset({"action", *_CONTEXT})
_ENVELOPE = frozenset(
    {
        "session_id",
        "snapshot_id",
        "policy",
        "trusted_adapter_scope",
        "trusted_ui_control_runtime",
        "window_title",
        "process_id",
        "process_name",
        "window_handle",
    }
)


def _invalid(reason: str) -> CuaCliError:
    return CuaCliError("invalid_action", "Invalid game_b3.v1 input: " + reason + ".")


def normalize_game_keys(keys: Any) -> list[str]:
    """Translate only documented frontend aliases, preserving order and duplicates."""
    if not isinstance(keys, list) or not 1 <= len(keys) <= 4:
        raise _invalid("invalid_key_count")
    result = []
    for key in keys:
        if not isinstance(key, str) or not key.isascii() or len(key) > 128:
            raise _invalid("unsupported_key")
        for part in key.split("+"):
            token = part.strip().upper()
            result.append(_ALIASES.get(token, token))
    _validate_keys(result)
    return result


def _validate_keys(keys: Any) -> None:
    if not isinstance(keys, list) or not 1 <= len(keys) <= 4:
        raise _invalid("invalid_key_count")
    if any(not isinstance(key, str) or key not in GAME_KEYS for key in keys):
        raise _invalid("unsupported_key")
    if len(set(keys)) != len(keys):
        raise _invalid("duplicate_key")


def game_input_payload(action: dict[str, Any], grants: tuple[str, ...]) -> dict[str, Any]:
    """Validate canonical wire data and immutable grants without coercion or field loss."""
    name = action.get("action")
    if (
        not isinstance(name, str)
        or name not in GAME_ACTIONS
        or any(action.get(key) != value for key, value in _CONTEXT.items())
    ):
        raise _invalid("invalid_context")
    required = _COMMON | ({"keys"} if name == "game_navigation" else {"dx", "dy"})
    allowed = required | ({"duration_ms", "dx", "dy"} if name == "game_navigation" else set())
    if not required.issubset(action) or set(action) - allowed:
        raise _invalid("unknown_or_missing_field")
    if name == "game_navigation":
        _validate_keys(action["keys"])
        duration = action.get("duration_ms", 0)
        if type(duration) is not int or not 0 <= duration <= 500:
            raise _invalid("invalid_duration")
    has_delta = "dx" in action or "dy" in action
    if has_delta and (
        any(type(action.get(key)) is not int or not -256 <= action[key] <= 256 for key in ("dx", "dy"))
        or (action["dx"] == 0 and action["dy"] == 0)
    ):
        raise _invalid("invalid_delta")
    if name not in grants or (has_delta and "relative_mouse" not in grants):
        raise CuaCliError("unsupported_action", "Immutable game input grants do not authorize this action.")
    result = dict(action)
    if name == "game_navigation":
        result["keys"] = list(action["keys"])
        result["duration_ms"] = action.get("duration_ms", 0)
    return result


def game_input_from_params(params: dict[str, Any], grants: tuple[str, ...]) -> dict[str, Any]:
    """Strip only the routing envelope and derive fixed raw foreground delivery."""
    action = {key: value for key, value in params.items() if key not in _ENVELOPE}
    for key in ("input_kind", "delivery_mode"):
        if key in action and action[key] != _CONTEXT[key]:
            raise _invalid("invalid_context")
        action[key] = _CONTEXT[key]
    if action.get("action") == "game_navigation":
        action["keys"] = normalize_game_keys(action.get("keys"))
    return game_input_payload(action, grants)
