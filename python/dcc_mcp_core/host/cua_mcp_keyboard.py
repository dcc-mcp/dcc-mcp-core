"""Action-specific keyboard payloads for the owned exact-window pixels route."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError

MAX_KEYPRESS_DURATION_MS = 10000
_MOVEMENT_KEYS = frozenset({"W", "A", "S", "D", "UP", "DOWN", "LEFT", "RIGHT"})
_FIELDS = frozenset({"action", "input_kind", "intent", "delivery_mode", "keys", "duration_ms"})
_EMPTY_COMPATIBILITY_FIELDS = {
    "x": None,
    "y": None,
    "button": None,
    "text": None,
    "scroll_x": None,
    "scroll_y": None,
    "checked": None,
    "path": [],
    "modifiers": [],
}


def keypress_payload(action: dict[str, Any]) -> dict[str, Any]:
    """Preserve an optional finite hold without widening Native key policy."""

    def invalid() -> CuaCliError:
        return CuaCliError(
            "invalid_action",
            "Pixels keypress requires keys only; a 1..10000 ms hold requires one or two unique WASD/arrow keys.",
        )

    for name, value in action.items():
        if name not in _FIELDS and (
            name not in _EMPTY_COMPATIBILITY_FIELDS or value != _EMPTY_COMPATIBILITY_FIELDS[name]
        ):
            raise invalid()
    keys = action.get("keys")
    if (
        not isinstance(keys, list)
        or not 1 <= len(keys) <= 16
        or any(not isinstance(key, str) or not key.strip() or len(key) > 32 for key in keys)
    ):
        raise invalid()
    result: dict[str, Any] = {"keys": list(keys)}
    if "duration_ms" in action:
        duration = action["duration_ms"]
        normalized = [key.strip().upper() for key in keys]
        if (
            type(duration) is not int
            or not 1 <= duration <= MAX_KEYPRESS_DURATION_MS
            or len(keys) > 2
            or len(set(normalized)) != len(normalized)
            or any(key not in _MOVEMENT_KEYS for key in normalized)
        ):
            raise invalid()
        result["duration_ms"] = duration
    return result
