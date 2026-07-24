"""Backend selector for the bundled ui_control skill entry points."""

from __future__ import annotations

import base64
from datetime import datetime
from datetime import timezone
import importlib
import json
import os
from pathlib import Path
import re
import sys
import threading
from typing import Any
from typing import Callable
from typing import Dict
from typing import Optional

from dcc_mcp_core import artefact_put_bytes
from dcc_mcp_core import artefact_put_file
from dcc_mcp_core.skill import skill_error

_AUDIT_LOCK = threading.Lock()
_CAPTURE_TTL_SECS = 24 * 60 * 60


def emit(result: Dict[str, Any]) -> None:
    """Emit a JSON tool result."""
    print(json.dumps(result, sort_keys=True))


def _read_subprocess_params() -> Dict[str, Any]:
    """Read the authoritative JSON payload written by the skill executor."""
    try:
        if sys.stdin.isatty():
            return {}
        raw = sys.stdin.read()
    except (OSError, ValueError):
        return {}
    if not raw.strip():
        return {}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def _import_sibling(name: str) -> Any:
    if __package__:
        return importlib.import_module(f".{name}", __package__)
    return importlib.import_module(name)


def _load_backend() -> Any:
    backend = os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock").strip().lower()
    if backend in {"", "mock"}:
        return _import_sibling("_backend")
    if backend in {"chrome", "chrome-cdp", "cdp"}:
        return _import_sibling("_chrome_backend")
    if backend in {"edge", "msedge", "microsoft-edge"}:
        os.environ.setdefault("DCC_MCP_UI_CONTROL_CDP_PRESET", "edge")
        return _import_sibling("_chrome_backend")
    if backend in {"agent-browser", "agent_browser", "agentbrowser"}:
        os.environ.setdefault("DCC_MCP_UI_CONTROL_CDP_PRESET", "agent-browser")
        return _import_sibling("_chrome_backend")
    if backend in {"windows-uia", "windows_uia", "uia", "win-uia", "win32-uia"}:
        return _import_sibling("_windows_uia_backend")
    return None


def _operation_error(result: Dict[str, Any]) -> Optional[str]:
    error = result.get("error")
    context = result.get("context")
    if not error and isinstance(context, dict):
        audit = context.get("audit")
        nested_result = context.get("result")
        if isinstance(audit, dict):
            error = audit.get("error_code")
        if not error and isinstance(nested_result, dict):
            error = nested_result.get("error_code")
    return str(error) if error else None


def _result_context(result: Dict[str, Any]) -> Dict[str, Any]:
    context = result.get("context")
    return context if isinstance(context, dict) else {}


def _canonical_backend(result: Dict[str, Any]) -> str:
    context = _result_context(result)
    snapshot = context.get("snapshot")
    if isinstance(snapshot, dict):
        metadata = snapshot.get("metadata")
        if isinstance(metadata, dict):
            ui_control = metadata.get("ui_control")
            if isinstance(ui_control, dict) and ui_control.get("backend"):
                return str(ui_control["backend"])
    if context.get("backend"):
        return str(context["backend"])
    selected = os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock").strip().lower()
    if selected in {"windows-uia", "windows_uia", "uia", "win-uia", "win32-uia"}:
        return "windows-ui-control-host"
    if selected in {
        "chrome",
        "chrome-cdp",
        "cdp",
        "edge",
        "msedge",
        "microsoft-edge",
        "agent-browser",
        "agent_browser",
        "agentbrowser",
    }:
        return "chrome-cdp"
    return selected or "mock"


def _artifact_token(value: Any) -> str:
    token = re.sub(r"[^A-Za-z0-9._-]+", "-", str(value)).strip("-._")
    return token[:64] or "default"


def _artifact_dict(reference: Any, **metadata: Any) -> Dict[str, Any]:
    artifact = {
        key: getattr(reference, key)
        for key in (
            "uri",
            "mime",
            "size_bytes",
            "display_name",
            "digest",
            "session_id",
            "correlation_id",
            "created_at",
            "expires_at",
        )
        if getattr(reference, key) is not None
    }
    artifact.update(metadata)
    return artifact


def _attach_capture_artifact(
    operation: str,
    context: Dict[str, Any],
    provenance: Dict[str, Any],
) -> None:
    session_id = str(provenance["session_id"])
    dcc_type = os.environ.get("DCC_MCP_UI_CONTROL_DCC_TYPE") or os.environ.get("DCC_MCP_DCC_TYPE", "ui-control")
    if operation == "snapshot":
        rich = context.get("__rich__")
        if not isinstance(rich, dict) or rich.get("kind") != "image":
            return
        mime = str(rich.get("mime") or "image/png").lower()
        extension = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp"}.get(mime)
        if extension is None:
            return
        snapshot_id = str(provenance.get("snapshot_id") or "snapshot")
        display_name = f"ui-control-snapshot-{_artifact_token(session_id)}-{_artifact_token(snapshot_id)}.{extension}"
        reference = artefact_put_bytes(
            base64.b64decode(str(rich["data"]), validate=True),
            mime=mime,
            display_name=display_name,
            session_id=session_id,
            correlation_id=snapshot_id,
            ttl_secs=_CAPTURE_TTL_SECS,
        )
        artifact = _artifact_dict(
            reference,
            kind="ui_control_snapshot",
            operation=operation,
            backend=provenance["backend"],
            dcc_type=dcc_type,
            snapshot_id=snapshot_id,
        )
        rich.update(
            artifact_uri=artifact["uri"],
            display_name=display_name,
            digest=artifact.get("digest"),
        )
    else:
        clip = context.get("artifact")
        if not isinstance(clip, dict) or not clip.get("manifest_path"):
            return
        recording_id = str(clip.get("recording_id") or "recording")
        display_name = f"ui-control-recording-{_artifact_token(session_id)}-{_artifact_token(recording_id)}.json"
        reference = artefact_put_file(
            str(clip["manifest_path"]),
            mime="application/json",
            display_name=display_name,
            session_id=session_id,
            correlation_id=recording_id,
            ttl_secs=_CAPTURE_TTL_SECS,
        )
        artifact = _artifact_dict(
            reference,
            kind="ui_control_recording_manifest",
            operation=operation,
            backend=provenance["backend"],
            dcc_type=dcc_type,
            recording_id=recording_id,
            manifest_sha256=clip.get("manifest_sha256"),
        )
    context["artifacts"] = [artifact]


def _attach_capture_provenance(
    name: str,
    params: Dict[str, Any],
    result: Dict[str, Any],
) -> Dict[str, Any]:
    operation = name[:-5] if name.endswith("_tool") else name
    if not result.get("success") or operation not in {"snapshot", "record_clip"}:
        return result
    context = _result_context(result)
    if not context:
        return result

    session_id = str(context.get("session_id") or params.get("session_id") or "default")
    provenance: Dict[str, Any] = {
        "tool": f"ui_control__{operation}",
        "backend": _canonical_backend(result),
        "session_id": session_id,
        "pixels_captured": operation == "record_clip" or isinstance(context.get("__rich__"), dict),
    }
    snapshot_id = context.get("snapshot_id")
    if snapshot_id:
        provenance["snapshot_id"] = str(snapshot_id)

    observation = context.get("observation")
    if isinstance(observation, dict):
        for key in (
            "observation_id",
            "process_id",
            "window_handle",
            "capture_backend",
            "width",
            "height",
        ):
            if observation.get(key) is not None:
                provenance[key] = observation[key]
        source_rect = observation.get("source_rect")
        if isinstance(source_rect, (list, tuple)) and len(source_rect) == 4:
            source_width = source_rect[2]
            source_height = source_rect[3]
            if isinstance(source_width, int) and isinstance(source_height, int):
                provenance["source_width"] = source_width
                provenance["source_height"] = source_height
                width = provenance.get("width")
                height = provenance.get("height")
                if isinstance(width, int) and isinstance(height, int):
                    provenance["downscaled"] = width < source_width or height < source_height

    target = context.get("target")
    if isinstance(target, dict):
        for key in ("process_id", "window_handle"):
            if target.get(key) is not None:
                provenance[key] = target[key]

    artifact = context.get("artifact")
    if operation == "record_clip" and isinstance(artifact, dict):
        for key in ("recording_id", "frame_count", "width", "height", "manifest_sha256"):
            if artifact.get(key) is not None:
                provenance[key] = artifact[key]

    context["capture_provenance"] = provenance
    try:
        _attach_capture_artifact(operation, context, provenance)
    except Exception as exc:
        context["artifact_error"] = {
            "code": type(exc).__name__,
            "message": str(exc),
        }
    if operation == "snapshot":
        if provenance["pixels_captured"]:
            size = "{}x{}".format(provenance.get("width", "?"), provenance.get("height", "?"))
            if provenance.get("downscaled"):
                size += " downscaled from {}x{}".format(
                    provenance.get("source_width", "?"),
                    provenance.get("source_height", "?"),
                )
        else:
            size = "accessibility-only"
        result["message"] = "{} [{}; {}; session={}]".format(
            str(result.get("message") or "Captured UI Control snapshot.").rstrip("."),
            provenance["backend"],
            size,
            session_id,
        )
    return result


def _record_operation(name: str, params: Dict[str, Any], result: Dict[str, Any]) -> None:
    """Append one redacted UI Control event to the Admin log stream."""
    if os.environ.get("DCC_MCP_DISABLE_FILE_LOGGING", "0") == "1":
        return
    try:
        from dcc_mcp_core import get_log_dir

        operation = name[:-5] if name.endswith("_tool") else name
        success = bool(result.get("success", False))
        error = _operation_error(result)
        action = str(params.get("action") or operation)
        context = _result_context(result)
        session_id = str(context.get("session_id") or params.get("session_id") or "default")
        snapshot_id = context.get("snapshot_id") or params.get("snapshot_id")
        capture_provenance = context.get("capture_provenance")
        condition = params.get("condition")
        condition_kind = condition.get("kind") if isinstance(condition, dict) else None
        detail = [
            f"backend={_canonical_backend(result)}",
            f"action={action}",
            f"session={session_id}",
        ]
        if condition_kind:
            detail.append(f"condition={condition_kind}")
        if error:
            detail.append(f"error={error}")
        event = {
            "event": "ui_control_operation",
            "tool": f"ui_control__{operation}",
            "dcc_type": os.environ.get("DCC_MCP_UI_CONTROL_DCC_TYPE")
            or os.environ.get("DCC_MCP_DCC_TYPE", "ui-control"),
            "session_id": session_id,
            "backend": _canonical_backend(result),
            "action": action,
            "success": success,
            "error": error,
            "message": f"DCC UI Control {operation} {'succeeded' if success else 'rejected'}",
            "detail": " ".join(detail),
        }
        if snapshot_id:
            event["snapshot_id"] = str(snapshot_id)
        if isinstance(capture_provenance, dict):
            event["pixels_captured"] = bool(capture_provenance.get("pixels_captured"))
            if capture_provenance.get("capture_backend"):
                event["capture_backend"] = capture_provenance["capture_backend"]
        artifacts = context.get("artifacts")
        if isinstance(artifacts, list) and artifacts and isinstance(artifacts[0], dict):
            event["artifacts"] = artifacts[:1]
            event["artifact_uri"] = artifacts[0].get("uri")
            event["artifact_name"] = artifacts[0].get("display_name")
        directory = Path(os.environ.get("DCC_MCP_LOG_DIR") or get_log_dir())
        directory.mkdir(parents=True, exist_ok=True)
        timestamp = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        level = "INFO" if success else "WARN"
        line = f"{timestamp} {level} dcc_mcp_core.ui_control.audit: {json.dumps(event, sort_keys=True)}\n"
        with _AUDIT_LOCK, (directory / f"dcc-mcp-ui-control.{os.getpid()}.log").open("a", encoding="utf-8") as stream:
            stream.write(line)
    except Exception:
        # Observability must never block or change a UI operation.
        return


def _call(name: str, params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    # Older standalone servers execute skill scripts as subprocesses and put
    # the complete tool arguments on stdin.  Read them at the shared boundary
    # so every backend receives the same contract; backend-specific readers
    # previously made mock/CDP work while Windows UIA silently received `{}`.
    call_params = dict(params) if params is not None else _read_subprocess_params()
    backend = _load_backend()
    if backend is None:
        selected = os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock")
        result = skill_error(
            f"Unsupported ui_control backend {selected!r}.",
            "backend_unavailable",
            backend=selected,
            supported_backends=[
                "mock",
                "chrome",
                "chrome-cdp",
                "cdp",
                "edge",
                "agent-browser",
                "windows-uia",
            ],
        )
        _record_operation(name, call_params, result)
        return result
    func: Callable[[Optional[Dict[str, Any]]], Dict[str, Any]] = getattr(backend, name)
    try:
        result = func(call_params)
    except Exception as exc:
        _record_operation(name, call_params, {"success": False, "error": type(exc).__name__})
        raise
    result = _attach_capture_provenance(name, call_params, result)
    _record_operation(name, call_params, result)
    return result


def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__snapshot to the selected backend."""
    return _call("snapshot_tool", params)


def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__find to the selected backend."""
    return _call("find_tool", params)


def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__act to the selected backend."""
    return _call("act_tool", params)


def record_clip_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__record_clip to the selected backend."""
    return _call("record_clip_tool", params)


def system_operation_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__system_operation to the selected backend."""
    return _call("system_operation_tool", params)


def wait_for_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__wait_for to the selected backend."""
    return _call("wait_for_tool", params)


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__stop_computer_use to the selected backend."""
    return _call("stop_computer_use_tool", params)
