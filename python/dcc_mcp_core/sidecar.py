"""Reusable sidecar action dispatch for script-backed DCC skills.

This module owns the DCC-neutral part of a Qt sidecar ``dispatch`` handler:
validate the payload, locate the active adapter server, resolve the script
source for the requested action, execute through an adapter-provided hook, and
return a JSON-safe result envelope. Transport stays in
``dcc_mcp_core.qt_dispatcher`` / ``qtserver://``; host-specific execution stays
inside each adapter.
"""

from __future__ import annotations

from dataclasses import dataclass
import inspect
import os
from pathlib import Path
import traceback
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Mapping
from typing import Sequence

ERROR_SERVER_NOT_RUNNING = "server-not-running"
ERROR_PAYLOAD_MALFORMED = "payload-malformed"
ERROR_UNKNOWN_ACTION = "unknown-action"
ERROR_NO_SOURCE_FILE = "no-source-file"
ERROR_DISPATCH_FAILED = "dispatch-failed"


@dataclass(frozen=True)
class SidecarDispatchRequest:
    """Normalized request passed to adapter-specific sidecar executors."""

    dcc_name: str
    server: Any
    action: str
    args: Mapping[str, Any]
    request_id: str | None
    script_path: str
    source_file: str
    skill_name: str = ""
    thread_affinity: str = ""
    execution: str = ""
    timeout_hint_secs: int | None = None
    payload: Mapping[str, Any] | None = None
    action_metadata: Mapping[str, Any] | None = None


@dataclass(frozen=True)
class _ValidatedPayload:
    action: str
    args: Mapping[str, Any]
    request_id: str | None
    explicit_source: str | None


@dataclass(frozen=True)
class _ResolvedSource:
    script_path: str
    source_file: str
    skill_name: str = ""
    thread_affinity: str = ""
    execution: str = ""
    timeout_hint_secs: int | None = None
    metadata: Mapping[str, Any] | None = None


class SidecarActionDispatcher:
    """Dispatch script-backed sidecar actions through adapter-owned hooks.

    ``SidecarActionDispatcher`` is intentionally transport-agnostic. Use it as
    the ``dispatch_handler`` for :func:`dcc_mcp_core.qt_dispatcher.start_qt_server`
    or from an existing sidecar RPC endpoint; do not use it as a replacement
    for ``HostRpcClient`` when the adapter already talks to a host-native
    command protocol directly.
    """

    def __init__(
        self,
        dcc_name: str,
        *,
        server_provider: Callable[[], Any] | None = None,
        action_resolver: Callable[..., Any] | None = None,
        executor: Callable[[SidecarDispatchRequest], Any] | None = None,
        bundled_skill_roots: Sequence[os.PathLike | str] = (),
    ) -> None:
        self.dcc_name = str(dcc_name or "unknown")
        self.server_provider = server_provider
        self.action_resolver = action_resolver
        self.executor = executor
        self.bundled_skill_roots = tuple(Path(root) for root in bundled_skill_roots)

    @staticmethod
    def maya_executor(
        execute_in_process: Callable[[Any, str, Mapping[str, Any], str], Any],
    ) -> Callable[[SidecarDispatchRequest], Any]:
        """Adapt ``execute_in_process(server, script_path, args, action_name)``."""

        def _executor(request: SidecarDispatchRequest) -> Any:
            return execute_in_process(request.server, request.script_path, request.args, request.action)

        return _executor

    @staticmethod
    def script_executor(
        run_skill_script: Callable[[str, Mapping[str, Any]], Any],
    ) -> Callable[[SidecarDispatchRequest], Any]:
        """Adapt ``run_skill_script(script_path, args)`` style executors."""

        def _executor(request: SidecarDispatchRequest) -> Any:
            return run_skill_script(request.script_path, request.args)

        return _executor

    def dispatch_payload(self, payload: Mapping[str, Any]) -> dict[str, Any]:
        """Validate and execute a sidecar dispatch payload."""
        validated = self._validate_payload(payload)
        if _is_error_envelope(validated):
            return validated

        server = self._get_server(validated)
        if _is_error_envelope(server):
            return server

        resolved = self._resolve_source(validated, server)
        if _is_error_envelope(resolved):
            return resolved

        request = SidecarDispatchRequest(
            dcc_name=self.dcc_name,
            server=server,
            action=validated.action,
            args=validated.args,
            request_id=validated.request_id,
            script_path=resolved.script_path,
            source_file=resolved.source_file,
            skill_name=resolved.skill_name,
            thread_affinity=resolved.thread_affinity,
            execution=resolved.execution,
            timeout_hint_secs=resolved.timeout_hint_secs,
            payload=payload,
            action_metadata=resolved.metadata,
        )

        try:
            result = self._execute(request)
        except Exception as exc:
            return self._error(
                ERROR_DISPATCH_FAILED,
                "Sidecar action dispatch failed",
                action=validated.action,
                request_id=validated.request_id,
                error_type=type(exc).__name__,
                error_message=str(exc),
                traceback="".join(traceback.format_exception(type(exc), exc, exc.__traceback__)),
            )
        return self._normalize_result(result, request)

    def _validate_payload(self, payload: Mapping[str, Any]) -> _ValidatedPayload | dict[str, Any]:
        if not isinstance(payload, Mapping):
            return self._error(ERROR_PAYLOAD_MALFORMED, "Sidecar dispatch payload must be an object")

        action_raw = payload.get("action")
        if not isinstance(action_raw, str) or not action_raw.strip():
            return self._error(
                ERROR_PAYLOAD_MALFORMED,
                "Sidecar dispatch payload must include a non-empty action",
                reason="missing-action",
            )
        action = action_raw.strip()

        args_raw = payload.get("args", {})
        if args_raw is None:
            args_raw = {}
        if not isinstance(args_raw, Mapping):
            return self._error(
                ERROR_PAYLOAD_MALFORMED,
                "Sidecar dispatch args must be an object",
                action=action,
                reason="invalid-args",
            )

        request_id_raw = payload.get("request_id")
        if request_id_raw is not None and not isinstance(request_id_raw, str):
            return self._error(
                ERROR_PAYLOAD_MALFORMED,
                "Sidecar dispatch request_id must be a string when provided",
                action=action,
                reason="invalid-request-id",
            )

        explicit_source = self._first_payload_source(payload, action=action, request_id=request_id_raw)
        if isinstance(explicit_source, dict):
            return explicit_source

        return _ValidatedPayload(
            action=action,
            args=dict(args_raw),
            request_id=request_id_raw,
            explicit_source=explicit_source,
        )

    def _first_payload_source(
        self,
        payload: Mapping[str, Any],
        *,
        action: str,
        request_id: str | None,
    ) -> str | None | dict[str, Any]:
        for key in ("script_path", "source_file"):
            raw = payload.get(key)
            if raw is None:
                continue
            if not isinstance(raw, str) or not raw.strip():
                return self._error(
                    ERROR_PAYLOAD_MALFORMED,
                    f"Sidecar dispatch {key} must be a non-empty string when provided",
                    action=action,
                    request_id=request_id,
                    reason=f"invalid-{key.replace('_', '-')}",
                )
            return raw.strip()
        return None

    def _get_server(self, payload: _ValidatedPayload) -> Any:
        if self.server_provider is None:
            return self._error(
                ERROR_SERVER_NOT_RUNNING,
                f"{self.dcc_name} server is not running",
                action=payload.action,
                request_id=payload.request_id,
                reason="missing-server-provider",
            )
        try:
            server = self.server_provider()
        except Exception as exc:
            return self._error(
                ERROR_SERVER_NOT_RUNNING,
                f"{self.dcc_name} server is not running",
                action=payload.action,
                request_id=payload.request_id,
                error_type=type(exc).__name__,
                error_message=str(exc),
            )
        if server is None:
            return self._error(
                ERROR_SERVER_NOT_RUNNING,
                f"{self.dcc_name} server is not running",
                action=payload.action,
                request_id=payload.request_id,
            )
        return server

    def _resolve_source(
        self,
        payload: _ValidatedPayload,
        server: Any,
    ) -> _ResolvedSource | dict[str, Any]:
        if payload.explicit_source:
            return _ResolvedSource(
                script_path=self._resolve_script_path(payload.explicit_source),
                source_file=payload.explicit_source,
            )

        if self.action_resolver is None:
            return self._error(
                ERROR_UNKNOWN_ACTION,
                f"Unknown sidecar action: {payload.action}",
                action=payload.action,
                request_id=payload.request_id,
            )

        raw = self._call_action_resolver(payload, server)

        if raw is None:
            return self._error(
                ERROR_UNKNOWN_ACTION,
                f"Unknown sidecar action: {payload.action}",
                action=payload.action,
                request_id=payload.request_id,
            )

        resolved = self._resolved_from_raw(raw)
        if resolved is None:
            # Fallback: builtin inline-handler tools (e.g. dcc_diagnostics__*)
            # have no source_file but are callable through the server's
            # registered handlers.  GH-1696
            if hasattr(server, "call_tool"):
                return _ResolvedSource(
                    script_path="",
                    source_file="",
                    skill_name="builtin",
                )
            return self._error(
                ERROR_NO_SOURCE_FILE,
                f"Sidecar action has no source file: {payload.action}",
                action=payload.action,
                request_id=payload.request_id,
            )
        return resolved

    def _call_action_resolver(self, payload: _ValidatedPayload, server: Any) -> Any:
        resolver = self.action_resolver
        if resolver is None:
            return None
        try:
            signature = inspect.signature(resolver)
        except (TypeError, ValueError):
            return resolver(payload.action)

        parameters = signature.parameters
        accepts_kwargs = any(param.kind is inspect.Parameter.VAR_KEYWORD for param in parameters.values())
        kwargs = {}
        if accepts_kwargs or "server" in parameters:
            kwargs["server"] = server
        if accepts_kwargs or "payload" in parameters:
            kwargs["payload"] = payload
        return resolver(payload.action, **kwargs)

    def _resolved_from_raw(self, raw: Any) -> _ResolvedSource | None:
        if isinstance(raw, (str, os.PathLike)):
            source_file = os.fspath(raw)
            if not source_file.strip():
                return None
            return _ResolvedSource(
                script_path=self._resolve_script_path(source_file),
                source_file=source_file,
            )

        metadata = _mapping_from_raw(raw)
        if metadata is None:
            return None

        source_file = _first_string(metadata, "script_path", "source_file", "path")
        if not source_file:
            return None

        return _ResolvedSource(
            script_path=self._resolve_script_path(source_file),
            source_file=source_file,
            skill_name=_first_string(metadata, "skill_name", "skill") or "",
            thread_affinity=_first_string(metadata, "thread_affinity", "affinity") or "",
            execution=_first_string(metadata, "execution") or "",
            timeout_hint_secs=_optional_int(metadata.get("timeout_hint_secs")),
            metadata=metadata,
        )

    def _resolve_script_path(self, source_file: str) -> str:
        path = Path(source_file)
        if path.is_absolute():
            return str(path)

        candidates = []
        for root in self.bundled_skill_roots:
            candidates.append(root / source_file)
            if path.parts[:1] != ("scripts",):
                candidates.append(root / "scripts" / source_file)
        for candidate in candidates:
            if candidate.is_file():
                return str(candidate)
        return source_file

    def _execute(self, request: SidecarDispatchRequest) -> Any:
        # Builtin inline-handler tools have no source_file; delegate to the
        # server's registered handler.  GH-1696
        if not request.source_file and hasattr(request.server, "call_tool"):
            return request.server.call_tool(request.action, request.args)

        if self.executor is not None:
            return self.executor(request)

        from dcc_mcp_core._server.inprocess_executor import run_skill_script

        return run_skill_script(request.script_path, request.args)

    def _normalize_result(self, result: Any, request: SidecarDispatchRequest) -> dict[str, Any]:
        safe_result = _json_safe(result)
        if isinstance(safe_result, dict) and isinstance(safe_result.get("success"), bool):
            return safe_result
        return {
            "success": True,
            "message": "Sidecar action dispatched",
            "context": {
                "dcc_name": self.dcc_name,
                "action": request.action,
                "request_id": request.request_id,
                "script_path": request.script_path,
                "result": safe_result,
            },
        }

    def _error(self, code: str, message: str, **context: Any) -> dict[str, Any]:
        clean_context = {
            key: value
            for key, value in {
                "dcc_name": self.dcc_name,
                **context,
            }.items()
            if value not in (None, "", {})
        }
        result: dict[str, Any] = {
            "success": False,
            "message": message,
            "error": code,
        }
        if clean_context:
            result["context"] = _json_safe(clean_context)
        return result


def _mapping_from_raw(raw: Any) -> Mapping[str, Any] | None:
    if isinstance(raw, Mapping):
        return raw

    values: dict[str, Any] = {}
    for key in (
        "script_path",
        "source_file",
        "path",
        "skill_name",
        "skill",
        "thread_affinity",
        "affinity",
        "execution",
        "timeout_hint_secs",
    ):
        if hasattr(raw, key):
            values[key] = getattr(raw, key)
    return values or None


def _is_error_envelope(value: Any) -> bool:
    return isinstance(value, Mapping) and value.get("success") is False and isinstance(value.get("error"), str)


def _first_string(mapping: Mapping[str, Any], *keys: str) -> str:
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
        if isinstance(value, os.PathLike):
            text = os.fspath(value)
            if text.strip():
                return text.strip()
    return ""


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _json_safe(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, os.PathLike):
        return os.fspath(value)
    if isinstance(value, Mapping):
        return {str(key): _json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_safe(item) for item in value]
    if isinstance(value, set):
        return [_json_safe(item) for item in sorted(value, key=repr)]
    if isinstance(value, Iterable) and not isinstance(value, (bytes, bytearray)):
        items: list[Any] = []
        for item in value:
            items.append(_json_safe(item))
        return items
    if isinstance(value, (bytes, bytearray)):
        return value.decode("utf-8", errors="replace")
    return repr(value)


__all__ = [
    "ERROR_DISPATCH_FAILED",
    "ERROR_NO_SOURCE_FILE",
    "ERROR_PAYLOAD_MALFORMED",
    "ERROR_SERVER_NOT_RUNNING",
    "ERROR_UNKNOWN_ACTION",
    "SidecarActionDispatcher",
    "SidecarDispatchRequest",
]
