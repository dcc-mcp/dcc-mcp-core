"""Bounded screenshot-coordinate pointer requests for the exact pixels route."""

from __future__ import annotations

import math
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError

MAX_POINTER_DURATION_MS = 1000
MAX_POINTER_POINTS = 256


def pointer_payload(action: dict[str, Any], size: tuple[int, int] | None) -> dict[str, Any]:
    """Validate a single move or press-move-release without translating pixels.

    Native owns screen translation, DPI/instance/lease revalidation and input
    release. Core never exposes a persistent held button across tool calls.
    """

    def invalid() -> CuaCliError:
        return CuaCliError("invalid_action", "Pointer input requires a bounded path inside the latest screenshot.")

    if size is None or any(action.get(key) for key in ("keys", "modifiers", "text", "scroll_x", "scroll_y")):
        raise invalid()
    duration = action.get("duration_ms", 500)
    if type(duration) is not int or not 1 <= duration <= MAX_POINTER_DURATION_MS:
        raise invalid()

    def point(value: Any) -> dict[str, int | float]:
        if not isinstance(value, dict) or set(value) != {"x", "y"}:
            raise invalid()
        for key, bound in zip(("x", "y"), size):
            coordinate = value[key]
            if type(coordinate) not in (int, float) or not 0 <= coordinate < bound or not math.isfinite(coordinate):
                raise invalid()
        return dict(value)

    result: dict[str, Any] = {"duration_ms": duration}
    if action["action"] == "move":
        if action.get("path") or action.get("button") is not None:
            raise invalid()
        result.update(point({"x": action.get("x"), "y": action.get("y")}))
    else:
        path = action.get("path")
        button = action.get("button", "left")
        if (
            not isinstance(path, list)
            or not 2 <= len(path) <= MAX_POINTER_POINTS
            or not isinstance(button, str)
            or button not in {"left", "middle", "right"}
            or action.get("x") is not None
            or action.get("y") is not None
        ):
            raise invalid()
        result.update(path=[point(value) for value in path], button=button)
    return result
