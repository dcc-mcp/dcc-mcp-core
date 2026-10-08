"""Validate explicit physical geometry without conflating Win32 and DWM bounds."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError


def validate_geometry(observation: dict[str, Any], provenance: dict[str, Any]) -> None:
    """Retain optional geometry for older runtimes without synthesizing new proof."""
    rect = observation["source_rect"]
    size = [observation["width"], observation["height"]]
    if list(rect[2:]) != size:
        raise CuaCliError("protocol_mismatch", "The PNG and its physical source rectangle differ.")
    visible = provenance.get("native_visible_bounds")
    wgc = provenance.get("wgc_geometry")
    if "native_visible_bounds" not in provenance and "wgc_geometry" not in provenance:
        return
    if not _rectangle(visible) or not _rectangle(provenance["native_window_bounds"]):
        raise CuaCliError("protocol_mismatch", "The runtime omitted its actual native visible bounds.")
    if wgc is None:
        if list(rect) != visible:
            raise CuaCliError("protocol_mismatch", "Verified-visible pixels changed their physical origin.")
        return
    win32 = provenance["native_window_bounds"]
    candidates = [candidate for candidate in (win32, visible) if candidate[2:] == size]
    origin = (
        "identical_native_bounds"
        if win32 == visible
        else ("win32_window" if candidates == [win32] else "dwm_extended_frame")
    )
    frame = wgc.get("frame") if isinstance(wgc, dict) else None
    if (
        not candidates
        or any(candidate != rect for candidate in candidates)
        or not isinstance(wgc, dict)
        or wgc.get("source_rect") != rect
        or wgc.get("origin") != origin
        or not isinstance(frame, dict)
        or any(
            frame.get(key) != size or any(type(item) is not int for item in frame[key])
            for key in ("item_size_before", "item_size_after", "pool_size", "content_size", "texture_size")
        )
        or type(frame.get("row_pitch_bytes")) is not int
        or frame["row_pitch_bytes"] < size[0] * 4
        or type(wgc.get("bgra_byte_len")) is not int
        or wgc["bgra_byte_len"] != size[0] * size[1] * 4
        or size[0] * size[1] > 64 * 1024 * 1024
    ):
        raise CuaCliError("protocol_mismatch", "The WGC physical origin or actual frame shape is inconsistent.")


def _rectangle(value: Any) -> bool:
    return (
        isinstance(value, list)
        and len(value) == 4
        and all(type(item) is int and -(2**31) <= item < 2**31 for item in value)
        and value[2] > 0
        and value[3] > 0
        and all(-(2**31) <= value[index] + value[index + 2] < 2**31 for index in (0, 1))
    )
