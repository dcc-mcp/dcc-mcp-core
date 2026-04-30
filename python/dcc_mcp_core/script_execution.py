"""Reusable script execution capture and result envelopes (#603).

DCC adapters expose ad-hoc script execution tools such as ``execute_python``.
Those tools need the same stdout/stderr capture behaviour and the same
``ToolResult``-shaped return contract, independent of the host application.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import AbstractContextManager
from dataclasses import dataclass
import io
import json
import sys
import traceback
from typing import Any
from typing import TextIO

from dcc_mcp_core.result_envelope import ToolResult


class ScriptExecutionSerializationError(TypeError):
    """Raised when a strict script result cannot be JSON-encoded."""


@dataclass(frozen=True)
class ScriptExecutionParams:
    """Normalized script execution parameters shared by DCC adapters."""

    code: str
    timeout_secs: int | None = None
    code_key: str = "code"
    timeout_key: str | None = None


def normalize_script_execution_params(
    params: Mapping[str, Any],
    *,
    default_timeout_secs: int | None = None,
) -> ScriptExecutionParams:
    """Normalize script execution parameter aliases.

    Adapters can expose one stable implementation while clients pass the
    source as ``code``, ``script``, or ``source`` and the timeout as either
    ``timeout_secs`` or ``timeout``.
    """
    if default_timeout_secs is not None and default_timeout_secs <= 0:
        raise ValueError("default_timeout_secs must be greater than zero")

    code_key = next((key for key in ("code", "script", "source") if params.get(key) is not None), None)
    if code_key is None:
        raise ValueError("Missing required script source: pass one of code, script, or source")

    code = params[code_key]
    if not isinstance(code, str):
        raise TypeError(f"{code_key} must be a string")

    timeout_key = next((key for key in ("timeout_secs", "timeout") if params.get(key) is not None), None)
    timeout_secs = default_timeout_secs
    if timeout_key is not None:
        timeout_value = params[timeout_key]
        if isinstance(timeout_value, bool) or not isinstance(timeout_value, int):
            raise TypeError(f"{timeout_key} must be an integer number of seconds")
        if timeout_value <= 0:
            raise ValueError(f"{timeout_key} must be greater than zero")
        timeout_secs = timeout_value

    return ScriptExecutionParams(
        code=code,
        timeout_secs=timeout_secs,
        code_key=code_key,
        timeout_key=timeout_key,
    )


class _CaptureStream(io.TextIOBase):
    """Capture text writes and optionally tee them to the original stream."""

    def __init__(self, original: TextIO, *, tee: bool) -> None:
        self._original = original
        self._tee = tee
        self._buffer = io.StringIO()

    def write(self, text: str) -> int:
        written = self._buffer.write(text)
        if self._tee:
            self._original.write(text)
        return written

    def flush(self) -> None:
        if self._tee:
            self._original.flush()

    def writable(self) -> bool:
        return True

    def isatty(self) -> bool:
        return False

    def getvalue(self) -> str:
        return self._buffer.getvalue()


class ScriptExecutionCapture(AbstractContextManager["ScriptExecutionCapture"]):
    """Capture ``sys.stdout`` and ``sys.stderr`` during host script execution.

    ``tee=True`` keeps host-console visibility while still collecting output
    for the tool response. This mirrors DCC plugin expectations where artists
    should continue seeing script output in the native console.
    """

    def __init__(self, *, tee: bool = False) -> None:
        self._tee = tee
        self._old_stdout: TextIO | None = None
        self._old_stderr: TextIO | None = None
        self._stdout_capture: _CaptureStream | None = None
        self._stderr_capture: _CaptureStream | None = None

    def __enter__(self) -> ScriptExecutionCapture:
        self._old_stdout = sys.stdout
        self._old_stderr = sys.stderr
        self._stdout_capture = _CaptureStream(sys.stdout, tee=self._tee)
        self._stderr_capture = _CaptureStream(sys.stderr, tee=self._tee)
        sys.stdout = self._stdout_capture
        sys.stderr = self._stderr_capture
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        if self._old_stdout is not None:
            sys.stdout = self._old_stdout
        if self._old_stderr is not None:
            sys.stderr = self._old_stderr

    @property
    def stdout(self) -> str:
        """Captured stdout text."""
        return "" if self._stdout_capture is None else self._stdout_capture.getvalue()

    @property
    def stderr(self) -> str:
        """Captured stderr text."""
        return "" if self._stderr_capture is None else self._stderr_capture.getvalue()


def _assert_json_serializable(value: Any) -> None:
    try:
        json.dumps(value)
    except (TypeError, ValueError) as exc:
        raise ScriptExecutionSerializationError(
            f"Script result is not JSON serializable: {exc}",
        ) from exc


def _repr_json_safe(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, Mapping):
        return {str(key): _repr_json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple, set, frozenset)):
        return [_repr_json_safe(item) for item in value]
    return repr(value)


def _normalize_result(value: Any, *, strict_json: bool, repr_fallback: bool) -> Any:
    try:
        _assert_json_serializable(value)
        return value
    except ScriptExecutionSerializationError:
        if strict_json or not repr_fallback:
            raise

    converted = _repr_json_safe(value)
    _assert_json_serializable(converted)
    return converted


@dataclass(frozen=True)
class ScriptExecutionResult:
    """Factory for standard DCC script execution envelopes."""

    @staticmethod
    def from_value(
        result: Any,
        *,
        stdout: str = "",
        stderr: str = "",
        strict_json: bool = True,
        repr_fallback: bool | None = None,
        message: str = "Script executed successfully",
    ) -> dict[str, Any]:
        """Return a success envelope, or a strict serialization error envelope."""
        use_repr = not strict_json if repr_fallback is None else repr_fallback
        try:
            normalized = _normalize_result(
                result,
                strict_json=strict_json,
                repr_fallback=use_repr,
            )
        except ScriptExecutionSerializationError as exc:
            return ToolResult.fail(
                str(exc),
                error="non_serializable_result",
                stdout=stdout,
                stderr=stderr,
            ).to_dict()

        return ToolResult.ok(
            message,
            result=normalized,
            stdout=stdout,
            stderr=stderr,
        ).to_dict()

    @staticmethod
    def from_exception(
        exc: BaseException,
        *,
        stdout: str = "",
        stderr: str = "",
        message: str | None = None,
    ) -> dict[str, Any]:
        """Return a structured failure envelope with traceback and captured output."""
        return ToolResult.fail(
            message or f"Script execution failed: {exc}",
            error="script_execution_error",
            stdout=stdout,
            stderr=stderr,
            exception_type=type(exc).__name__,
            exception_message=str(exc),
            traceback="".join(traceback.format_exception(type(exc), exc, exc.__traceback__)),
        ).to_dict()


__all__ = [
    "ScriptExecutionCapture",
    "ScriptExecutionParams",
    "ScriptExecutionResult",
    "ScriptExecutionSerializationError",
    "normalize_script_execution_params",
]
