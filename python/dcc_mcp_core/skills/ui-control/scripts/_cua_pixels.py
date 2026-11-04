"""Pixels snapshot requests and projection without an invented semantic tree."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_popup import requested_popup_handles
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions


def check_capture_popup_owner(params: dict[str, Any]) -> None:
    """Refuse secondary observation without its operator ceiling before opening a child."""
    options = params.get("trusted_ui_control_runtime")
    requested_popup_handles(
        params.get("native_capture_popup_window_handles"),
        allowed=isinstance(options, UiControlRuntimeOptions) and options.allow_native_capture_popup_keyboard,
        root_handle=params.get("window_handle") or 0,
    )


def request_snapshot(client: Any, max_depth: int, max_nodes: int, popup_handles: Any) -> dict[str, Any]:
    """Forward secondary selection only to the explicit owned pixels consumer."""
    if popup_handles is not None and popup_handles != []:
        if not isinstance(client, PixelsMcpHostClient):
            raise CuaCliError("unsupported_action", "Native capture popup observation requires owned pixels MCP.")
        return client.snapshot(
            max_depth=max_depth, max_nodes=max_nodes, native_capture_popup_window_handles=popup_handles
        )
    return client.snapshot(max_depth=max_depth, max_nodes=max_nodes)


def check_recording_owner(params: dict[str, Any]) -> None:
    """Refuse a disabled recording ceiling before opening any owned child."""
    options = params.get("trusted_ui_control_runtime")
    if isinstance(options, UiControlRuntimeOptions) and options.recording is None:
        raise CuaCliError("unsupported_action", "The owner did not grant pixels recording.")


def capture_snapshot(raw: dict[str, Any], client: Any, entry: dict[str, Any], session_id: str) -> dict[str, Any]:
    """Retain native identity and provenance in the bundled tool's snapshot."""
    snapshot_id = raw["observation_id"]
    snapshot = {
        "session_id": session_id,
        "root": None,
        "node_count": 0,
        "metadata": {
            "snapshot_id": snapshot_id,
            "observation_mode": "pixels_only",
            "accessibility_state_id": None,
            "ui_control": {
                "backend": "dcc-cua",
                "scope": entry["scope"],
                "target": client.target,
                "accessibility_available": False,
            },
            "computer_use": raw["observation"],
        },
    }
    entry["snapshot_id"] = snapshot_id
    entry["snapshot"] = snapshot
    return {
        "success": True,
        "snapshot_id": snapshot_id,
        "snapshot": snapshot,
        "image": raw["image_bytes"],
        "mime_type": "image/png",
        "observation": raw["observation"],
        "target": client.target,
        "task_context": raw["task_context"],
        "observation_mode": "pixels_only",
        "accessibility_state_id": None,
        "accessibility_available": False,
        **(
            {
                "native_capture_popup_observation": raw["native_capture_popup_observation"],
                "native_capture_popup_overlays": raw["native_capture_popup_overlays"],
            }
            if "native_capture_popup_overlays" in raw
            else {}
        ),
    }


def error_context(exc: Exception, *, fresh_observation: bool) -> dict[str, Any]:
    """Retain safe native evidence independently of recovery guidance."""
    result = dict(exc.native_evidence) if isinstance(exc, OwnedCuaMcpError) else {}
    if fresh_observation or getattr(exc, "fresh_observation_required", False):
        result.update(fresh_observation_required=True, blind_retry=False)
    return result
