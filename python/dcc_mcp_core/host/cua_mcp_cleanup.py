"""Truthful terminal cleanup for every owned public-MCP pixels task."""

from __future__ import annotations

from copy import deepcopy
import json
from pathlib import Path
import re
import time
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_errors import safe_failure_evidence
from dcc_mcp_core.host.ui_control_options import _ordinary_absolute_path


class OwnedPixelsTaskCleanup:
    """Revoke one task, validate its actual ACK, and retain one conclusion."""

    def __init__(self, client: Any) -> None:
        self.client = client
        self.output_dir: str | None = None
        self._stopped: dict[str, Any] | None = None
        self._stop_error: CuaCliError | None = None

    def stop(self) -> dict[str, Any]:
        """Stop once, validate the actual ACK, and never turn uncertainty into success."""
        if self._stop_error is not None:
            raise self._stop_error
        if self._stopped is not None:
            return deepcopy(self._stopped)
        self.client._closed = True
        self.client._observation_id = None
        evidence: dict[str, Any] = {}
        failure: CuaCliError | None = None
        stopped: dict[str, Any] | None = None
        recording = self.client.options.recording
        deadline = time.monotonic() + (recording.cleanup_timeout_seconds if recording is not None else 5)
        try:
            if not self.client.task_id:
                raise CuaCliError("cleanup_unknown", "No owned task acknowledgement is available.")
            wire = self.client._transport.tool("stop_task", {"task_id": self.client.task_id}, cleanup=True)
            raw = wire["structuredContent"]
            self.client._check_context(raw)
            if recording is not None and raw.get("recording_output_dir") != self.output_dir:
                raise CuaCliError("cleanup_unknown", "Cleanup changed the immutable recording directory.")
            ack = raw.get("cleanup")
            evidence = cleanup_evidence(raw, self.output_dir)
            valid_ack = (
                isinstance(ack, dict)
                and ack.get("type") == "session_stopped"
                and ack.get("session_id") == "mcp-" + self.client.task_id
                and type(ack.get("success")) is bool
                and ack.get("active") is False
                and ack.get("cleanup_pending") is False
                and evidence.get("component_evidence_valid", True)
            )
            status = raw.get("status")
            if not valid_ack or status not in {"stopped", "cleanup_failed", "cleanup_unknown"}:
                raise CuaCliError("cleanup_unknown", "The owned task did not acknowledge exact inactive cleanup.")
            if status != "stopped" or ack["success"] is not True or raw.get("ok") is not True or wire.get("isError"):
                code = "cleanup_failed" if status == "cleanup_failed" else "cleanup_unknown"
                raise CuaCliError(code, "The native task retained a failed or unknown cleanup outcome.")
            stopped = {
                **evidence,
                "type": "session_stopped",
                "session_id": self.client.session_id,
                "success": True,
                "active": False,
                "cleanup_pending": False,
            }
        except Exception as exc:
            code = getattr(exc, "code", "cleanup_unknown")
            evidence["cleanup_status"] = "cleanup_failed" if code == "cleanup_failed" else "cleanup_unknown"
            message = str(exc) if isinstance(exc, CuaCliError) else "Owned native cleanup failed."
            failure = CuaCliError("cleanup_failed" if code == "cleanup_failed" else "cleanup_unknown", message)
        finally:
            try:
                self.client._transport.close(grace_seconds=max(0, deadline - time.monotonic()))
            except Exception as exc:
                evidence["process_cleanup_failed"] = True
                if failure is None:
                    failure = CuaCliError(
                        "cleanup_failed",
                        str(exc) if isinstance(exc, CuaCliError) else "The owned process did not finish cleanly.",
                    )
                    evidence["cleanup_status"] = "cleanup_failed"
        if failure is not None:
            self._stopped = None
            self._stop_error = OwnedCuaMcpError(failure.code, str(failure), {})
            self._stop_error.native_evidence = {**evidence, "cleanup_pending": failure.code != "cleanup_failed"}
            raise self._stop_error
        self._stopped = stopped
        return deepcopy(self._stopped)


def cleanup_evidence(raw: dict[str, Any], output_dir: str | None, *, nested: bool = False) -> dict[str, Any]:
    """Project bounded ACK diagnostics without arbitrary text or reusable authority."""
    ack = raw.get("cleanup")
    result = safe_failure_evidence(
        {"task_context": {key: raw.get(key) for key in ("provider", "runtime_version", "task_id", "target")}}
    )
    result["cleanup_status"] = (
        raw.get("status")
        if raw.get("status") in {"stopped", "cleanup_failed", "cleanup_unknown"}
        else "cleanup_unknown"
    )
    result["reported_cleanup_status"] = result["cleanup_status"]
    if not isinstance(ack, dict):
        return result
    projected = {key: ack[key] for key in ("success", "active", "cleanup_pending") if type(ack.get(key)) is bool}
    projected["type"] = "session_stopped" if ack.get("type") == "session_stopped" else "unknown"
    if isinstance(ack.get("session_id"), str) and len(ack["session_id"]) <= 128:
        projected["session_id"] = (
            ack["session_id"] if ack["session_id"] == "mcp-" + str(raw.get("task_id")) else "mismatched"
        )
    projected["cleanup_issues"] = (
        [
            {
                key: item[key]
                for key in ("phase", "code")
                if isinstance(item.get(key), str) and re.fullmatch(r"[A-Za-z0-9_.:+-]{1,128}", item[key])
            }
            for item in ack.get("cleanup_issues", [])[:32]
            if isinstance(item, dict)
        ]
        if isinstance(ack.get("cleanup_issues", []), list)
        else []
    )
    result["cleanup"] = projected
    try:
        projected.update(_component_evidence(ack, output_dir))
        video = projected.get("recording_video")
        if (
            ack.get("success") is True
            and video is not None
            and (
                not video["finalized"]
                or "error_code" in video
                or "current_partial" in video
                or ("capture_sidecar" in video and not video["capture_sidecar"]["finalized"])
            )
        ):
            result["component_evidence_valid"] = False
        response = ack.get("host_response")
        if response is not None:
            if (
                nested
                or not isinstance(response, dict)
                or response.get("session_id") != "mcp-" + str(raw.get("task_id"))
            ):
                raise ValueError("unexpected cleanup Host response identity")
            retained = cleanup_evidence({**raw, "cleanup": response}, output_dir, nested=True)
            projected["host_response"] = retained.get("cleanup", {})
            if retained.get("component_evidence_valid") is False:
                raise ValueError("invalid retained component evidence")
    except (ValueError, TypeError):
        result["component_evidence_valid"] = False
    if output_dir is not None:
        result["output_dir"] = output_dir
    return result


def _component_evidence(ack: dict[str, Any], output_dir: str | None) -> dict[str, Any]:
    """Keep only the formal typed component summaries, including partial paths."""
    result = {}

    def booleans(value: dict[str, Any], names: tuple[str, ...]) -> dict[str, Any]:
        if not isinstance(value, dict) or any(type(value.get(key)) is not bool for key in names):
            raise ValueError("malformed component state")
        return {key: value[key] for key in names}

    def artifact(value: Any) -> str:
        if not isinstance(value, str) or len(value) > 4096 or output_dir is None:
            raise ValueError("artifact lacks an owner recording directory")
        path, root = _ordinary_absolute_path(value), Path(output_dir)
        if root not in path.parents:
            raise ValueError("artifact escaped its task directory")
        return value

    video = ack.get("recording_video")
    if "recording_video" in ack:
        projected = booleans(video, ("active", "finalized"))
        if video["active"]:
            raise ValueError("recording remains active")
        for key in ("path", "manifest_path", "current_partial"):
            if key in video:
                projected[key] = artifact(video[key])
        paths = video.get("segment_paths")
        if not isinstance(paths, list) or len(paths) > 4096:
            raise ValueError("malformed segment paths")
        projected["segment_paths"] = [artifact(path) for path in paths]
        if "error_code" in video:
            if not isinstance(video["error_code"], str) or not re.fullmatch(
                r"[A-Za-z0-9_.:+-]{1,128}", video["error_code"]
            ):
                raise ValueError("malformed recording error code")
            projected["error_code"] = video["error_code"]
        if "capture_sidecar" in video:
            sidecar = video["capture_sidecar"]
            projected["capture_sidecar"] = booleans(sidecar, ("finalized",))
            if (
                not isinstance(sidecar.get("sha256"), str)
                or not re.fullmatch(r"[a-fA-F0-9]{64}", sidecar["sha256"])
                or any(
                    type(sidecar.get(key)) is not int or not 0 <= sidecar[key] < 2**64 for key in ("records", "frames")
                )
            ):
                raise ValueError("malformed capture sidecar receipt")
            projected["capture_sidecar"].update(
                path=artifact(sidecar.get("path")),
                sha256=sidecar["sha256"],
                records=sidecar["records"],
                frames=sidecar["frames"],
            )
        result["recording_video"] = projected
    if "live_observation" in ack:
        source = ack["live_observation"]
        projected = booleans(source, ("active", "cleanup_complete", "cleanup_pending"))
        if source["active"] or (
            ack.get("success") is True and (not source["cleanup_complete"] or source["cleanup_pending"])
        ):
            raise ValueError("native source did not acknowledge cleanup")
        if "stream_id" in source:
            if type(source["stream_id"]) is not int or not 0 <= source["stream_id"] < 2**64:
                raise ValueError("malformed native stream identity")
            projected["stream_id"] = source["stream_id"]
        result["live_observation"] = projected
    if len(json.dumps(result)) > 1024 * 1024:
        raise ValueError("component summary exceeds its bounded envelope")
    return result
