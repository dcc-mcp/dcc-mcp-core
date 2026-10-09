"""Exact-target pixels consumer of the public DCC-CUA task MCP protocol."""

from __future__ import annotations

import base64
import binascii
from contextlib import suppress
from copy import deepcopy
import struct
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_cleanup import OwnedPixelsTaskCleanup
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_geometry import validate_geometry
from dcc_mcp_core.host.cua_mcp_pointer import pointer_payload
from dcc_mcp_core.host.cua_mcp_preparation import PixelsMcpPreparation
from dcc_mcp_core.host.cua_mcp_recording import RECORDING_METHODS
from dcc_mcp_core.host.cua_mcp_recording import PixelsMcpRecording
from dcc_mcp_core.host.cua_mcp_transport import OwnedCuaMcpTransport
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions


class PixelsMcpHostClient:
    """One logical UI session, one owned process, and one runtime-issued task.

    UI logical session ids, public task ids, and native observation session ids
    remain distinct. No Core-side lease is minted. Unsupported semantics fail
    explicitly; this client never falls back to shared JSONL or accessibility.
    """

    observation_mode = "pixels_only"

    def __init__(
        self,
        *,
        session_id: str,
        dcc_type: str,
        process_id: int | None,
        window_handle: int | None,
        allow_raw_input: bool,
        options: UiControlRuntimeOptions,
        window_title: str | None = None,
    ) -> None:
        if type(process_id) is not int or process_id <= 0 or type(window_handle) is not int or window_handle <= 0:
            raise CuaCliError("invalid_target", "Owned pixels MCP requires an exact positive PID and HWND.")
        self.session_id = session_id
        self.options = options
        self._target = {"process_id": process_id, "window_handle": window_handle}
        self._title_constraint = window_title or ""
        self.task_id: str | None = None
        self._observation_id: str | None = None
        self._observation_size: tuple[int, int] | None = None
        self._native_session_id: str | None = None
        self._native_instance: dict[str, Any] | None = None
        self._window_state_id: str | None = None
        self._last_window_state_id: str | None = None
        self._last_observation_id: str | None = None
        self._closed = False
        self._recording = PixelsMcpRecording(self)
        self._cleanup = OwnedPixelsTaskCleanup(self)
        self.preparation = PixelsMcpPreparation(self)
        self._actions = options.allowed_actions if allow_raw_input else ()
        methods = ["snapshot", "get_window_state"]
        scopes = [
            {"action": action, "input_kind": "raw_input", "secret_input": False, "authorization_category": "raw_input"}
            for action in self._actions
        ]
        if self._actions:
            methods.append("execute_action")
        self.preparation.register(methods, scopes)
        if options.recording is not None:
            methods.extend(RECORDING_METHODS)
        if any(operation in options.window_operations for operation in ("activate", "restore_activate")):
            methods.append("change_window_state")
        if "minimize" in options.window_operations:
            methods.append("minimize_window")
            scopes.append(
                {
                    "action": "minimize_window",
                    "input_kind": "window_state",
                    "secret_input": False,
                    "authorization_category": "window_state",
                }
            )
        if "set_frame" in options.window_operations:
            methods.append("set_window_frame")
            scopes.append(
                {
                    "action": "set_window_frame",
                    "input_kind": "window_state",
                    "secret_input": False,
                    "authorization_category": "window_state",
                }
            )
        self._transport = OwnedCuaMcpTransport(options)
        try:
            opened = self._transport.tool(
                "start_task",
                {
                    "application_label": dcc_type,
                    "target_process_id": process_id,
                    "target_window_handle": window_handle,
                    "surface": "window",
                    "observation_mode": "pixels_only",
                    "allowed_methods": methods,
                    "allowed_actions": scopes,
                    "ttl_minutes": options.ttl_minutes,
                    **({"allow_recording": True} if options.recording is not None else {}),
                    **({"allow_capture_preparation": True} if options.capture_preparation is not None else {}),
                },
            )["structuredContent"]
            # Retain the returned id before validation so failed startup can revoke it.
            task_id = opened.get("task_id")
            if isinstance(task_id, str) and task_id:
                self.task_id = task_id
            if (
                opened.get("ok") is not True
                or opened.get("status") != "started"
                or opened.get("provider") != "dcc-cua"
                or opened.get("runtime_version") != options.runtime_version
                or not self.task_id
            ):
                raise CuaCliError("protocol_mismatch", "The runtime did not start the expected bounded pixels task.")
            self._check_target(opened.get("target"), verify_title=True)
            self._target = deepcopy(opened["target"])
            self._recording.bind(opened)
            self._cleanup.output_dir = self._recording.output_dir
        except Exception:
            with suppress(Exception):
                self.stop()
            raise

    @property
    def target(self) -> dict[str, Any]:
        """Return the exact validated target, without substituting another window."""
        return deepcopy(self._target)

    def _check_target(self, value: Any, *, verify_title: bool = False) -> None:
        if not isinstance(value, dict) or any(
            type(value.get(key)) is not int or value[key] != self._target[key]
            for key in ("process_id", "window_handle")
        ):
            raise CuaCliError("invalid_target", "The owned MCP response changed the exact PID/HWND.")
        if verify_title and self._title_constraint:
            title = value.get("window_title")
            if not isinstance(title, str) or self._title_constraint.casefold() not in title.casefold():
                raise CuaCliError("invalid_target", "The exact target does not satisfy the narrowed title constraint.")

    def _call(self, method: str, params: dict[str, Any], expected_type: str) -> dict[str, Any]:
        if self._closed or not self.task_id:
            raise CuaCliError("backend_unavailable", "The owned pixels task is closed.")
        if method in {
            "execute_action",
            "change_window_state",
            "minimize_window",
            "set_window_frame",
            "recording_start",
            "recording_stop",
        }:
            self._window_state_id = None
        try:
            result = self._transport.tool(
                "dcc_cua_task_call", {"task_id": self.task_id, "method": method, "params": params}
            )
        except OwnedCuaMcpError as exc:
            context = exc.native_evidence.get("task_context")
            try:
                if context is not None:
                    self._check_context(context)
            except CuaCliError:
                with suppress(Exception):
                    self.stop()
                raise
            if method in {
                "execute_action",
                "change_window_state",
                "minimize_window",
                "set_window_frame",
                *RECORDING_METHODS,
            }:
                self._observation_id = None
                exc.fresh_observation_required = True
            raise
        raw = deepcopy(result["structuredContent"])
        try:
            context = raw.get("task_context")
            self._check_context(context)
            if raw.get("type") != expected_type:
                raise CuaCliError("protocol_mismatch", "The owned MCP task identity or response type changed.")
            if raw.get("target") is not None:
                self._check_target(raw["target"], verify_title=True)
        except Exception:
            with suppress(Exception):
                self.stop()
            raise
        if method in {"snapshot", "capture_preparation_snapshot"}:
            raw["_mcp_content"] = result.get("content")
        return raw

    def _check_context(self, context: Any) -> None:
        if (
            not isinstance(context, dict)
            or context.get("task_id") != self.task_id
            or context.get("provider") != "dcc-cua"
            or context.get("runtime_version") != self.options.runtime_version
        ):
            raise CuaCliError("protocol_mismatch", "The owned MCP task identity changed.")
        self._check_target(context.get("target"))

    def snapshot(self, *, max_depth: int, max_nodes: int) -> dict[str, Any]:
        """Capture real pixels and retain the native provenance with AX absent."""
        self._observation_id = None
        self._observation_size = None
        try:
            raw = self._call("snapshot", {}, "snapshot")
            observation = raw.get("observation")
            if (
                raw.get("observation_mode") != "pixels_only"
                or raw.get("accessibility_state_id") is not None
                or not isinstance(observation, dict)
            ):
                raise CuaCliError(
                    "protocol_mismatch", "Owned pixels MCP returned semantic or missing observation data."
                )
            observation_id = raw.get("observation_id")
            native_session = observation.get("session_id")
            provenance = observation.get("capture_provenance")
            if (
                not isinstance(observation_id, str)
                or not observation_id
                or observation_id == self._last_observation_id
                or observation.get("observation_id") != observation_id
                or not isinstance(native_session, str)
                or not native_session
                or (self._native_session_id is not None and self._native_session_id != native_session)
                or not isinstance(provenance, dict)
            ):
                raise CuaCliError(
                    "protocol_mismatch", "Owned pixels MCP returned inconsistent native observation identity."
                )
            self._check_target(observation, verify_title=True)
            self._check_target(provenance)
            instance = provenance.get("native_instance")
            if (
                provenance.get("observation_mode") != "pixels_only"
                or provenance.get("pixels_captured") is not True
                or provenance.get("whole_desktop_capture") is not False
                or provenance.get("accessibility_available") is not False
                or not isinstance(provenance.get("backend"), str)
                or not provenance["backend"]
                or not _positive_int(provenance.get("window_dpi"))
                or not _positive_int(provenance.get("capture_generation"))
                or not _rect_valid(provenance.get("native_window_bounds"))
                or not _native_instance_valid(instance)
                or (self._native_instance is not None and self._native_instance != instance)
                or not _positive_int(observation.get("width"))
                or not _positive_int(observation.get("height"))
                or observation.get("capture_backend") != provenance["backend"]
                or not _rect_valid(observation.get("source_rect"))
            ):
                raise CuaCliError("protocol_mismatch", "Owned pixels MCP omitted the exact native pixel fence.")
            validate_geometry(observation, provenance)
            images = [
                block
                for block in raw.pop("_mcp_content", [])
                if isinstance(block, dict) and block.get("type") == "image"
            ]
            if len(images) != 1 or images[0].get("mimeType") != "image/png":
                raise CuaCliError("capture_failed", "Owned pixels MCP must return exactly one native PNG.")
            encoded = images[0].get("data")
            if not isinstance(encoded, str) or len(encoded) > 90 * 1024 * 1024:
                raise CuaCliError("capture_failed", "Owned pixels MCP returned invalid image content.")
            pixels = base64.b64decode(encoded, validate=True)
            image = raw.get("image")
            if (
                len(pixels) < 24
                or not pixels.startswith(b"\x89PNG\r\n\x1a\n")
                or pixels[12:16] != b"IHDR"
                or struct.unpack(">II", pixels[16:24]) != (observation["width"], observation["height"])
                or not isinstance(image, dict)
                or image.get("length") != len(pixels)
            ):
                raise CuaCliError("capture_failed", "Owned pixels MCP image does not match its native descriptor.")
            raw["image_bytes"] = pixels
            raw["node_count"] = 0
            self._observation_id = observation_id
            self._observation_size = (observation["width"], observation["height"])
            self._last_observation_id = observation_id
            self._native_session_id = native_session
            self._native_instance = deepcopy(instance)
            return raw
        except (CuaCliError, ValueError, TypeError, binascii.Error) as exc:
            with suppress(Exception):
                self.stop()
            if isinstance(exc, CuaCliError):
                raise
            raise CuaCliError("protocol_mismatch", "Owned pixels MCP returned malformed pixel evidence.") from exc

    def execute(self, action: dict[str, Any]) -> dict[str, Any]:
        """Deliver one allowed physical action against the latest native pixels."""
        if not self._observation_id:
            raise CuaCliError("stale_observation", "Take a fresh pixels snapshot before acting.")
        name = action.get("action")
        if action.get("input_kind") != "raw_input" or name not in self._actions:
            raise CuaCliError("unsupported_action", "This pixels task does not authorize that physical action.")
        if any(key in action for key in ("element_token", "element_index", "accessibility_state_id", "secret_handle")):
            raise CuaCliError("unsupported_action", "Semantic and secret fields are unavailable in pixels_only mode.")
        allowed = {"action", "input_kind", "intent", "delivery_mode", "x", "y", "keys", "text", "button"}
        # Backend compatibility fields are omitted deliberately, not forwarded to
        # the deny-unknown-fields public protocol or mistaken for semantic tokens.
        payload = {key: value for key, value in action.items() if key in allowed}
        if name in {"move", "drag"}:
            payload = {
                **{key: value for key, value in payload.items() if key in {"action", "input_kind", "intent"}},
                **pointer_payload(action, self._observation_size),
            }
        payload["delivery_mode"] = "foreground"
        if name == "type":
            payload["type_chars_only"] = True
        observation_id = self._observation_id
        self._observation_id = None
        raw = self._call(
            "execute_action",
            {"observation_id": observation_id, "action": payload, "capture_after": False},
            "action_completed",
        )
        result = raw.get("result")
        try:
            delivery = result.get("delivery") if isinstance(result, dict) else None
            if (
                not isinstance(result, dict)
                or result.get("route") != "windows_exact_pixel_final_input"
                or result.get("native_instance") != self._native_instance
                or result.get("effect") != "unverifiable"
                or result.get("verification_required") is not True
                or result.get("fresh_observation_required") is not True
                or not isinstance(delivery, dict)
                or type(delivery.get("delivery_completed")) is not bool
                or type(delivery.get("post_dispatch_validated")) is not bool
                or type(raw.get("success")) is not bool
                or result.get("success") is not raw["success"]
                or raw["success"] is not (delivery["delivery_completed"] and delivery["post_dispatch_validated"])
            ):
                raise CuaCliError("protocol_mismatch", "The physical input result omitted its exact native fence.")
            self._check_target(result.get("target"))
        except CuaCliError:
            with suppress(Exception):
                self.stop()
            raise
        raw["effect"] = result["effect"]
        raw["verification_required"] = True
        raw["fresh_observation_required"] = True
        return raw

    def window_state(self) -> dict[str, Any]:
        """Read only this task's exact window state."""
        self._window_state_id = None
        raw = self._call("get_window_state", {}, "window_state")
        try:
            self._check_window_state(raw.get("state"))
            if "set_frame" in self.options.window_operations or self.options.capture_preparation is not None:
                state_id = raw["state"].get("window_state_id")
                if (
                    raw.get("session_id") != "mcp-" + self.task_id
                    or raw.get("observation_id") is not None
                    or raw.get("accessibility_state_id") is not None
                ):
                    raise CuaCliError(
                        "protocol_mismatch", "The exact metadata response changed its Host session or token kind."
                    )
                frame_ready = (
                    raw["state"]["visible"]
                    and not raw["state"]["minimized"]
                    and _physical_rect_valid(raw["state"].get("bounds"))
                    and _physical_rect_valid(raw["state"].get("visible_bounds"))
                )
                if frame_ready:
                    if (
                        not isinstance(state_id, str)
                        or not 1 <= len(state_id) <= 256
                        or state_id == self._last_window_state_id
                    ):
                        raise CuaCliError(
                            "protocol_mismatch", "The exact metadata response omitted its fresh window-state identity."
                        )
                    self._window_state_id = state_id
                    self._last_window_state_id = state_id
                elif state_id is not None:
                    raise CuaCliError("protocol_mismatch", "Unavailable frame geometry cannot mint a metadata token.")
            if raw["state"]["minimized"] or not raw["state"]["visible"]:
                self._observation_id = None
        except CuaCliError:
            with suppress(Exception):
                self.stop()
            raise
        return raw

    def _check_window_state(self, state: Any, *, require_frame_geometry: bool = False) -> None:
        if not isinstance(state, dict):
            raise CuaCliError("protocol_mismatch", "The exact window state is absent.")
        self._check_target(state)
        instance = state.get("native_instance")
        if (
            state.get("exists") is not True
            or state.get("backend") != "windows-exact-native-state"
            or not _positive_int(state.get("dpi"))
            or (
                not _rect_valid(state.get("bounds"))
                and not ("set_frame" in self.options.window_operations and state.get("bounds") is None)
            )
            or any(type(state.get(key)) is not bool for key in ("visible", "minimized", "foreground"))
            or not _native_instance_valid(instance)
            or (self._native_instance is not None and instance != self._native_instance)
        ):
            raise CuaCliError("protocol_mismatch", "The window state omitted its exact native instance or geometry.")
        if "set_frame" in self.options.window_operations and (
            (state.get("bounds") is not None and not _physical_rect_valid(state["bounds"]))
            or (state.get("visible_bounds") is not None and not _physical_rect_valid(state["visible_bounds"]))
            or state["dpi"] >= 2**32
        ):
            raise CuaCliError("protocol_mismatch", "Set frame requires valid native Win32 and DWM physical geometry.")
        if require_frame_geometry and (
            not _physical_rect_valid(state.get("bounds"))
            or not _physical_rect_valid(state.get("visible_bounds"))
            or not state["visible"]
            or state["minimized"]
        ):
            raise CuaCliError("protocol_mismatch", "Set frame requires visible native Win32 and DWM physical geometry.")
        self._native_instance = deepcopy(instance)

    def invalidate_action_evidence(self) -> None:
        """Discard local mutation tokens after a rejected or attempted frame call."""
        self._window_state_id = None
        self._observation_id = None

    def set_frame(self, frame: dict[str, int], *, window_state_id: str) -> dict[str, Any]:
        """Consume this task's fresh metadata token without capture or activation."""
        if "set_frame" not in self.options.window_operations:
            raise CuaCliError("unsupported_action", "The owner did not authorize setting the exact window frame.")
        current_id = self._window_state_id
        self.invalidate_action_evidence()
        requested = validate_window_frame(frame)
        if not isinstance(window_state_id, str) or not current_id or window_state_id != current_id:
            raise CuaCliError("stale_observation", "Read fresh get_window_state metadata before setting the frame.")
        raw = self._call(
            "set_window_frame",
            {"window_state_id": window_state_id, "frame": requested},
            "window_frame_set",
        )
        try:
            state = raw.get("state")
            self._check_window_state(state, require_frame_geometry=True)
            result = raw.get("result")
            applied = [requested[key] for key in ("x", "y", "width", "height")]
            try:
                returned_frame = validate_window_frame(
                    result.get("requested_frame") if isinstance(result, dict) else None
                )
            except CuaCliError:
                raise CuaCliError(
                    "protocol_mismatch", "The frame mutation omitted its exact requested frame."
                ) from None
            if (
                raw.get("session_id") != "mcp-" + self.task_id
                or not isinstance(result, dict)
                or result.get("success") is not True
                or result.get("effect") != "confirmed"
                or result.get("operation") != "set_window_frame"
                or returned_frame != requested
                or not _rect_valid(result.get("applied_frame"))
                or result["applied_frame"] != applied
                or state["bounds"] != applied
                or result.get("window_state_id") != window_state_id
                or result.get("native_instance") != self._native_instance
                or result.get("fresh_observation_required") is not True
                or result.get("automatic_input") is not False
                or result.get("process_terminated") is not False
                or result.get("cua") != {"path": "windows_exact_instance_set_window_pos"}
                or "window_state_id" in state
            ):
                raise CuaCliError("protocol_mismatch", "The frame mutation omitted its exact native completion fence.")
            self._check_target(result.get("target"))
            actual = result.get("state")
            self._check_window_state(actual, require_frame_geometry=True)
            if "window_state_id" in actual or any(
                actual.get(key) != state.get(key)
                for key in ("bounds", "visible_bounds", "dpi", "visible", "minimized", "foreground", "native_instance")
            ):
                raise CuaCliError("protocol_mismatch", "The frame mutation changed its native state readback.")
        except CuaCliError:
            with suppress(Exception):
                self.stop()
            raise
        return raw

    def change_window_state(self, operation: str) -> dict[str, Any]:
        """Perform only an owner-granted window operation, without taking pixels.

        Minimize consumes the latest native observation. Activation and restore
        require explicit calls and do not imply content interaction or capture.
        """
        operation = "restore_activate" if operation == "restore" else operation
        if (
            operation not in {"activate", "restore_activate", "minimize"}
            or operation not in self.options.window_operations
        ):
            raise CuaCliError("unsupported_action", "The owner did not authorize that exact window operation.")
        observation_id = self._observation_id
        if operation == "minimize" and not observation_id:
            raise CuaCliError("stale_observation", "Take a fresh pixels snapshot before minimizing.")
        self._observation_id = None
        method = "minimize_window" if operation == "minimize" else "change_window_state"
        params = {"observation_id": observation_id} if operation == "minimize" else {"operation": operation}
        raw = self._call(method, params, "window_state_changed")
        try:
            self._check_window_state(raw.get("state"))
            result = raw.get("result")
            if (
                raw.get("operation") != ("minimize" if operation == "minimize" else operation)
                or not isinstance(result, dict)
                or result.get("success") is not True
                or result.get("fresh_observation_required") is not True
                or result.get("automatic_input") is not False
            ):
                raise CuaCliError("protocol_mismatch", "The window mutation omitted its native completion fence.")
            if operation == "minimize":
                self._check_target(result.get("target"))
                minimized_state = result.get("state")
                if not isinstance(minimized_state, dict):
                    raise CuaCliError("protocol_mismatch", "Minimize omitted its native state.")
                self._check_target(minimized_state)
                if (
                    raw["state"]["minimized"] is not True
                    or result.get("effect") != "confirmed"
                    or result.get("operation") != "minimize"
                    or result.get("observation_id") != observation_id
                    or result.get("native_instance") != self._native_instance
                    or result.get("process_terminated") is not False
                    or minimized_state.get("minimized") is not True
                    or minimized_state.get("instance") != self._native_instance
                ):
                    raise CuaCliError("protocol_mismatch", "Minimize changed its observation or native instance.")
            else:
                target = result.get("target")
                # Activation uses the native Rust WindowTarget, not target_wire.
                if not isinstance(target, dict):
                    raise CuaCliError("invalid_target", "Activation omitted its exact native target.")
                self._check_target({"process_id": target.get("pid"), "window_handle": target.get("window_id")})
                if (
                    target.get("is_foreground") is not True
                    or raw["state"]["foreground"] is not True
                    or raw["state"]["minimized"] is not False
                    or raw["state"]["visible"] is not True
                ):
                    raise CuaCliError(
                        "protocol_mismatch", "The explicit activation did not retain the exact foreground."
                    )
        except CuaCliError:
            with suppress(Exception):
                self.stop()
            raise
        return raw

    def stop(self) -> dict[str, Any]:
        """Revoke only this task and close only this owned executable."""
        self._window_state_id = None
        return self._cleanup.stop()

    def _unsupported(self, *args: Any, **kwargs: Any) -> Any:
        raise CuaCliError(
            "unsupported_action",
            "The owned pixels MCP transport does not support semantic or resume operations.",
        )

    accessibility_snapshot = _unsupported
    invoke_menu = _unsupported
    resume = _unsupported

    def recording_start(self, *, output_dir: str | None = None, record_video: bool = True) -> dict[str, Any]:
        """Manually start only the operator-granted video-only lifecycle."""
        return self._recording.call("recording_start", output_dir=output_dir, record_video=record_video)

    def recording_state(self) -> dict[str, Any]:
        """Read native paused/degraded/failed state without claiming finalization."""
        return self._recording.call("recording_state")

    def recording_stop(self) -> dict[str, Any]:
        """Finalize the same task's recording, retaining native failures."""
        return self._recording.call("recording_stop")


def _positive_int(value: Any) -> bool:
    return type(value) is int and value > 0


def validate_window_frame(value: Any) -> dict[str, int]:
    """Require a complete physical frame without coercing booleans or numbers."""
    keys = {"x", "y", "width", "height"}
    if (
        not isinstance(value, dict)
        or set(value) != keys
        or any(type(value[key]) is not int for key in keys)
        or not all(-(2**31) <= value[key] < 2**31 for key in ("x", "y"))
        or not all(0 < value[key] < 2**31 for key in ("width", "height"))
        or value["x"] + value["width"] >= 2**31
        or value["y"] + value["height"] >= 2**31
    ):
        raise CuaCliError(
            "invalid_action", "frame requires exact x/y/width/height integers with positive signed-32-bit extents."
        )
    return {key: value[key] for key in ("x", "y", "width", "height")}


def _native_instance_valid(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and _positive_int(value.get("process_creation_time_100ns"))
        and _positive_int(value.get("window_thread_id"))
        and type(value.get("window_class_hash")) is int
        and 0 <= value["window_class_hash"] < 2**64
        and type(value.get("owner_window_handle")) is int
        and value["owner_window_handle"] >= 0
    )


def _rect_valid(value: Any) -> bool:
    return (
        isinstance(value, (list, tuple))
        and len(value) == 4
        and all(type(item) is int for item in value)
        and value[2] > 0
        and value[3] > 0
    )


def _physical_rect_valid(value: Any) -> bool:
    return (
        _rect_valid(value)
        and all(-(2**31) <= item < 2**31 for item in value)
        and value[0] + value[2] < 2**31
        and value[1] + value[3] < 2**31
    )
