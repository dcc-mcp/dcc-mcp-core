"""Core-facing adapter for the standalone ``dcc-cua`` Host."""

from __future__ import annotations

from typing import Any
from typing import Dict
from typing import Optional

from dcc_mcp_core.cua_cli import CuaCliBridge
from dcc_mcp_core.cua_cli import CuaCliError

UiControlHostError = CuaCliError


class UiControlHostClient:
    """One Core session over a persistent standalone CUA JSONL bridge."""

    def __init__(
        self,
        *,
        session_id: str,
        task_grant_id: str,
        dcc_type: str,
        process_id: Optional[int],
        window_handle: Optional[int],
        allow_raw_input: bool,
        bridge: Optional[CuaCliBridge] = None,
    ) -> None:
        self.session_id = session_id
        self.task_grant_id = task_grant_id
        self._bridge = bridge or CuaCliBridge()
        self._window_capability: Optional[str] = None
        self._target: Dict[str, Any] = {}
        self._latest_observation_id: Optional[str] = None
        self._latest_accessibility_state_id: Optional[str] = None
        try:
            opened = self._call(
                "open_session",
                {
                    "session_id": session_id,
                    "grant": {
                        "task_grant_id": task_grant_id,
                        "application_label": dcc_type,
                        "process_id": process_id,
                        "window_handle": window_handle,
                        "allow_raw_input": allow_raw_input,
                        "allow_recording": True,
                    },
                },
                "session_opened",
            )
        except Exception:
            self._bridge.close()
            raise
        self._window_capability = str(opened["window_capability"])
        self._target = dict(opened.get("target") or {})

    @property
    def target(self) -> Dict[str, Any]:
        """Return the Host-validated exact target."""
        return dict(self._target)

    def snapshot(self, *, max_depth: int, max_nodes: int) -> Dict[str, Any]:
        """Capture one Host-owned shared-memory PNG and accessibility state."""
        response = self._snapshot_call("snapshot", max_depth=max_depth, max_nodes=max_nodes)
        image = response.get("image")
        if not isinstance(image, dict):
            self._invalidate_observation()
            raise UiControlHostError("capture_failed", "dcc-cua returned no screenshot.")
        try:
            pixels = self._bridge.read_image(response)
        except Exception as exc:
            self._invalidate_observation()
            if isinstance(exc, UiControlHostError):
                raise
            raise UiControlHostError("capture_failed", f"Cannot read the CUA screenshot: {exc}") from exc
        response["image_bytes"] = pixels
        return response

    def accessibility_snapshot(self, *, max_depth: int, max_nodes: int) -> Dict[str, Any]:
        """Refresh accessibility state without capturing pixels."""
        return self._snapshot_call("accessibility_snapshot", max_depth=max_depth, max_nodes=max_nodes)

    def window_state(self) -> Dict[str, Any]:
        """Read exact target state."""
        return self._call("get_window_state", self._authority(), "window_state")

    def change_window_state(self, operation: str) -> Dict[str, Any]:
        """Activate the exact target; CUA intentionally exposes no restore/show aliases."""
        if operation != "activate":
            raise UiControlHostError("unsupported", "dcc-cua only supports exact-window activation.")
        try:
            return self._call(
                "change_window_state",
                {**self._authority(), "operation": operation},
                "window_state_changed",
            )
        finally:
            self._invalidate_observation()

    def recording_start(self, *, output_dir: str, record_video: bool) -> Dict[str, Any]:
        """Start CUA trajectory recording for this exact session."""
        response = self._call(
            "recording_start",
            {
                **self._authority(),
                "request": {"output_dir": output_dir, "record_video": record_video},
            },
            "recording_started",
        )
        return self._tool_result(response, "recording_start")

    def recording_stop(self) -> Dict[str, Any]:
        """Finalize the active CUA trajectory recording."""
        response = self._call("recording_stop", self._authority(), "recording_stopped")
        return self._tool_result(response, "recording_stop")

    def recording_state(self) -> Dict[str, Any]:
        """Read the active CUA trajectory recording state."""
        response = self._call("recording_state", self._authority(), "recording_state")
        return self._tool_result(response, "recording_state")

    def execute(self, action: Dict[str, Any]) -> Dict[str, Any]:
        """Execute one action against the latest Host observation fences."""
        if not self._latest_observation_id or not self._latest_accessibility_state_id:
            raise UiControlHostError("stale_observation", "Take a fresh ui_control snapshot before acting.")
        try:
            response = self._call(
                "execute_action",
                {
                    **self._authority(),
                    "observation_id": self._latest_observation_id,
                    "accessibility_state_id": self._latest_accessibility_state_id,
                    "action": action,
                },
                "action_completed",
            )
            if bool(response.get("target_closed")):
                self._window_capability = None
                self._bridge.close()
            return response
        finally:
            self._invalidate_observation()

    def resume(self) -> None:
        """Clear the shared Escape stop latch for this authorized session."""
        self._call("resume_session", self._authority(), "session_resumed")
        self._invalidate_observation()

    def stop(self) -> Dict[str, Any]:
        """Stop this session and release its bridge connection."""
        if self._window_capability is None:
            return {"type": "session_stopped", "session_id": self.session_id, "cleanup_pending": False}
        try:
            response = self._call(
                "stop_session",
                {"session_id": self.session_id},
                "session_stopped",
            )
        finally:
            self._window_capability = None
            self._invalidate_observation()
            self._bridge.close()
        return response

    def _snapshot_call(self, method: str, *, max_depth: int, max_nodes: int) -> Dict[str, Any]:
        response = self._call(
            method,
            {**self._authority(), "max_depth": max_depth, "max_nodes": max_nodes},
            method,
        )
        self._latest_observation_id = str(response["observation_id"])
        self._latest_accessibility_state_id = str(response["accessibility_state_id"])
        self._target = dict(response.get("target") or self._target)
        root, focus_runtime_id = _legacy_accessibility_tree(response.get("root"))
        response["root"] = root
        response["focus_runtime_id"] = focus_runtime_id
        return response

    def _authority(self) -> Dict[str, Any]:
        if self._window_capability is None:
            raise UiControlHostError("backend_unavailable", "The dcc-cua session is closed.")
        return {
            "session_id": self.session_id,
            "task_grant_id": self.task_grant_id,
            "window_capability": self._window_capability,
        }

    def _invalidate_observation(self) -> None:
        self._latest_observation_id = None
        self._latest_accessibility_state_id = None

    def _call(self, method: str, params: Dict[str, Any], expected_type: str) -> Dict[str, Any]:
        response = self._bridge.call(method, params)
        if response.get("type") != expected_type:
            raise UiControlHostError(
                "protocol_mismatch",
                f"dcc-cua returned {response.get('type')!r} for {method!r}.",
            )
        return response

    @staticmethod
    def _tool_result(response: Dict[str, Any], method: str) -> Dict[str, Any]:
        result = response.get("result")
        if not isinstance(result, dict):
            raise UiControlHostError("protocol_mismatch", f"dcc-cua returned no result for {method!r}.")
        return result


def _legacy_accessibility_tree(raw: Any) -> tuple[Dict[str, Any], str]:
    """Convert CUA's compact depth-first element list for Core's existing finder."""
    if not isinstance(raw, dict) or not isinstance(raw.get("elements"), list):
        raise UiControlHostError("protocol_mismatch", "dcc-cua returned invalid accessibility data.")
    roots = []
    stack = []
    nodes_by_index = {}
    focus_runtime_id = ""
    uses_parent_indices = any(isinstance(element, dict) and "parent_index" in element for element in raw["elements"])
    for fallback_index, element in enumerate(raw["elements"]):
        if not isinstance(element, dict):
            raise UiControlHostError("protocol_mismatch", "dcc-cua returned an invalid accessibility element.")
        try:
            depth = int(element.get("depth") or 0)
        except (TypeError, ValueError):
            raise UiControlHostError("protocol_mismatch", "dcc-cua returned an invalid element depth.") from None
        if depth < 0 or (not uses_parent_indices and depth > len(stack)):
            raise UiControlHostError("protocol_mismatch", "dcc-cua returned a malformed accessibility tree.")
        token = str(element.get("element_token") or "")
        runtime_id = token or f"cua:{element.get('element_index', fallback_index)}"
        bounds = element.get("bounds") or element.get("frame")
        if isinstance(bounds, dict) and ("w" in bounds or "h" in bounds):
            bounds = {
                "x": bounds.get("x"),
                "y": bounds.get("y"),
                "width": bounds.get("w"),
                "height": bounds.get("h"),
            }
        node = {
            "runtime_id": runtime_id,
            "name": str(element.get("name") or element.get("label") or ""),
            "automation_id": str(element.get("automation_id") or ""),
            "class_name": str(element.get("class_name") or ""),
            "control_type": str(element.get("role") or "control"),
            "enabled": bool(element.get("enabled", True)),
            "offscreen": bool(element.get("offscreen", False)),
            "focused": bool(element.get("focused", False)),
            "bounds": bounds,
            "value": element.get("value"),
            "checked": element.get("checked"),
            "element_index": element.get("element_index"),
            "element_token": token or None,
            "children": [],
        }
        if uses_parent_indices:
            parent_index = element.get("parent_index")
            if parent_index is None:
                roots.append(node)
            elif isinstance(parent_index, int) and parent_index in nodes_by_index:
                nodes_by_index[parent_index]["children"].append(node)
            else:
                raise UiControlHostError("protocol_mismatch", "dcc-cua returned an invalid parent index.")
        else:
            del stack[depth:]
            if stack:
                stack[-1]["children"].append(node)
            else:
                roots.append(node)
            stack.append(node)
        element_index = element.get("element_index", fallback_index)
        if not isinstance(element_index, int) or element_index in nodes_by_index:
            raise UiControlHostError("protocol_mismatch", "dcc-cua returned an invalid element index.")
        nodes_by_index[element_index] = node
        if node["focused"]:
            focus_runtime_id = runtime_id
    if len(roots) == 1:
        return roots[0], focus_runtime_id
    return {
        "runtime_id": "cua:root",
        "name": "",
        "control_type": "window",
        "enabled": True,
        "offscreen": False,
        "children": roots,
    }, focus_runtime_id
