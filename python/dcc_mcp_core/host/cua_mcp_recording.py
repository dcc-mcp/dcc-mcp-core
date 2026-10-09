"""Video lifecycle on an existing runtime-owned pixels task, without new authority."""

from __future__ import annotations

from contextlib import suppress
from copy import deepcopy
import json
from pathlib import Path
import re
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_errors import safe_failure_evidence
from dcc_mcp_core.host.cua_mcp_recording_verification import RecordingProgressVerification
from dcc_mcp_core.host.ui_control_options import _ordinary_absolute_path

RECORDING_METHODS = ("recording_start", "recording_state", "recording_stop")


class PixelsMcpRecording:
    """Validate one native video lifecycle without owning general task cleanup."""

    def __init__(self, client: Any) -> None:
        self.client = client
        self.output_dir: str | None = None
        self.progress: RecordingProgressVerification | None = None

    def bind(self, opened: dict[str, Any]) -> None:
        recording = self.client.options.recording
        if recording is None:
            return
        expected = Path(recording.output_root) / self.client.task_id
        value = opened.get("recording_output_dir")
        try:
            path = _ordinary_absolute_path(value)
        except ValueError:
            raise CuaCliError("protocol_mismatch", "The runtime returned an invalid recording destination.") from None
        if path != expected or not expected.is_absolute() or expected.parent != Path(recording.output_root):
            raise CuaCliError("protocol_mismatch", "The runtime changed the operator-owned recording destination.")
        self.output_dir = value
        self.progress = RecordingProgressVerification(self.client, opened) if recording.require_progress else None

    def reset_progress(self) -> None:
        if self.progress is not None:
            self.progress.reset()

    def check_host_context(self, context: dict[str, Any]) -> None:
        if self.progress is not None:
            self.progress.check_context(context)

    def call(self, method: str, *, output_dir: str | None = None, record_video: bool = True) -> dict[str, Any]:
        if self.client.options.recording is None or self.output_dir is None:
            raise CuaCliError("unsupported_action", "The owner did not grant pixels recording.")
        params: dict[str, Any] = {}
        if method == "recording_start":
            if record_video is not True:
                raise CuaCliError("invalid_request", "pixels_only recording requires record_video=true.")
            if output_dir is not None and output_dir != self.output_dir:
                raise CuaCliError("invalid_request", "output_dir must equal the immutable runtime task directory.")
            # The runtime supplies its immutable path and video-only request.
            self.reset_progress()
        self.client._observation_id = None
        expected = {"recording_start": "recording_started", "recording_stop": "recording_stopped"}.get(method, method)
        try:
            raw = self.client._call(method, params, expected)
        except Exception:
            if method != "recording_state":
                self.reset_progress()
            raise
        progress = None
        try:
            if raw.get("session_id") != "mcp-" + self.client.task_id:
                raise CuaCliError("protocol_mismatch", "The recording response changed the Host session.")
            state = raw.get("result")
            if (
                not isinstance(state, dict)
                or state.get("backend") != "native_pixels_video"
                or state.get("status") not in {"active", "paused", "degraded", "failed", "stopped"}
                or type(state.get("active")) is not bool
                or type(state.get("healthy")) is not bool
                or state.get("expected_components") != ["video"]
                or not isinstance(state.get("issues"), list)
                or len(state["issues"]) > 32
                or any(
                    not isinstance(issue, str) or not re.fullmatch(r"[A-Za-z0-9_.:+-]{1,128}", issue)
                    for issue in state["issues"]
                )
                or state["healthy"] is not (not state["issues"])
                or (state["status"] == "stopped" and state["active"])
                or (state["status"] in {"active", "paused", "degraded"} and not state["active"])
                or state.get("trajectory_available") is not False
                or state.get("trajectory") is not None
                or "video" not in state
                or "source" not in state
                or (state["video"] is not None and not isinstance(state["video"], dict))
                or (state["source"] is not None and not isinstance(state["source"], dict))
                or ("cleanup_pending" in state and type(state["cleanup_pending"]) is not bool)
                or ("cleanup_issues" in state and not isinstance(state["cleanup_issues"], list))
            ):
                raise CuaCliError("protocol_mismatch", "The runtime omitted truthful native video state.")
            _bounded_state(state, Path(self.output_dir))
            if self.progress is not None:
                progress = self.progress.observe(state, raw["task_context"])
        except (CuaCliError, ValueError, TypeError):
            with suppress(Exception):
                self.client.stop()
            raise
        result = deepcopy(state)
        result.update(output_dir=self.output_dir, host_session_id=raw["session_id"], task_context=raw["task_context"])
        if progress is not None:
            result["recording_progress"] = progress
        if method == "recording_stop":
            self.reset_progress()
        if method != "recording_state" and state["status"] == "failed":
            exc = OwnedCuaMcpError(
                "recording_failed", "The native video lifecycle failed; inspect recording_state.", {}
            )
            exc.native_evidence = {"recording": _recording_failure_evidence(result)}
            exc.fresh_observation_required = True
            raise exc
        if progress is not None and (
            (method == "recording_state" and not progress["satisfied"])
            or (method == "recording_start" and (not progress["healthy"] or progress["current"] is None))
        ):
            exc = OwnedCuaMcpError(
                "recording_progress_unavailable", "Recording has not proved healthy sample advancement.", {}
            )
            exc.native_evidence = {"recording": _recording_failure_evidence(result)}
            exc.fresh_observation_required = True
            raise exc
        return result


def _bounded_state(state: dict[str, Any], root: Path) -> None:
    """Bound native JSON and reject artifact destinations outside the task child."""
    if len(json.dumps(state)) > 1024 * 1024:
        raise CuaCliError("protocol_mismatch", "Native recording state exceeds its bounded envelope.")
    budget = [4096]

    def artifact(value: Any) -> None:
        try:
            path = _ordinary_absolute_path(value)
        except ValueError:
            raise CuaCliError("protocol_mismatch", "Native recording returned an invalid artifact path.") from None
        if path != root and root not in path.parents:
            raise CuaCliError("protocol_mismatch", "Native recording escaped its immutable task directory.")

    def check(value: Any, depth: int = 0) -> None:
        budget[0] -= 1
        if budget[0] < 0 or depth > 16:
            raise CuaCliError("protocol_mismatch", "Native recording state exceeds its structural bounds.")
        if isinstance(value, dict):
            for key, item in value.items():
                if key in {"path", "manifest_path", "output_dir", "current_partial"} and item is not None:
                    artifact(item)
                if key == "segment_paths":
                    if not isinstance(item, list):
                        raise CuaCliError("protocol_mismatch", "Native recording returned invalid segment paths.")
                    for segment in item:
                        artifact(segment)
                check(item, depth + 1)
        elif isinstance(value, list):
            for item in value:
                check(item, depth + 1)

    check(state)


def _recording_failure_evidence(state: dict[str, Any]) -> dict[str, Any]:
    result = {
        key: state[key]
        for key in ("backend", "status", "active", "healthy", "issues", "output_dir", "host_session_id", "task_context")
    }
    result["task_context"] = safe_failure_evidence({"task_context": state["task_context"]}).get("task_context", {})
    if "recording_progress" in state:
        result["recording_progress"] = deepcopy(state["recording_progress"])
    video = state.get("video")
    if isinstance(video, dict):
        result["video"] = {
            key: video[key] for key in ("path", "manifest_path", "active", "finalized", "paused") if key in video
        }
    return result
