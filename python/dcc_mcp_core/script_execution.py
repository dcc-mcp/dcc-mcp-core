"""Reusable script execution parameter handling, capture, and result envelopes (#603).

DCC adapters expose ad-hoc script execution tools such as ``execute_python``.
Those tools need the same parameter normalization, the same stdout/stderr
capture behaviour, and the same ``ToolResult``-shaped return contract,
independent of the host application.

This module owns the request/response contracts. The persistent execution
context that holds host globals and script variables lives in
``dcc_mcp_core.runtime.script_execution_context``, and the managed
temp-script store lives in ``dcc_mcp_core.runtime.script_tempfile``. Both are
re-exported here so existing imports keep working unchanged.
"""

from __future__ import annotations

from collections.abc import Mapping
import contextlib
from contextlib import AbstractContextManager
from dataclasses import dataclass
from dataclasses import field
import hashlib
import io
import json
from pathlib import Path
import sys
from typing import Any
from typing import Sequence
from typing import TextIO

from dcc_mcp_core.errors import DccMcpError
from dcc_mcp_core.result_envelope import ToolResultEnvelope
from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestExecution
from dcc_mcp_core.runtime.scene_digest import SceneDigestExecutionError
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot
from dcc_mcp_core.runtime.scene_digest import StateDigestProvider
from dcc_mcp_core.runtime.scene_digest_envelope import scene_digest_postcondition as _scene_digest_postcondition
from dcc_mcp_core.runtime.scene_digest_envelope import script_execution_failure as _script_execution_failure
from dcc_mcp_core.runtime.script_execution_context import ScriptExecutionContext
from dcc_mcp_core.runtime.script_execution_context import capture_state_digest
from dcc_mcp_core.runtime.script_execution_context import clear_script_namespace
from dcc_mcp_core.runtime.script_execution_context import execute_with_context
from dcc_mcp_core.runtime.script_execution_context import execute_with_state_digest
from dcc_mcp_core.runtime.script_execution_context import get_default_script_execution_context
from dcc_mcp_core.runtime.script_execution_context import get_script_namespace
from dcc_mcp_core.runtime.script_execution_context import register_dcc_namespace
from dcc_mcp_core.runtime.script_execution_context import register_state_digest_provider
from dcc_mcp_core.runtime.script_execution_context import reset_default_script_execution_context_for_tests
from dcc_mcp_core.runtime.script_execution_context import update_script_namespace
from dcc_mcp_core.runtime.script_execution_helpers import file_ref_for_script_path as _file_ref_for_script_path
from dcc_mcp_core.runtime.script_tempfile import cleanup_temp_scripts
from dcc_mcp_core.runtime.script_tempfile import write_temp_script
from dcc_mcp_core.schema import derive_script_parameters_schema
from dcc_mcp_core.script_materialization import MaterializedScript
from dcc_mcp_core.script_materialization import default_script_materialization_root
from dcc_mcp_core.script_materialization import materialize_script
from dcc_mcp_core.script_materialization import resolve_materialized_script

ScriptMaterializationPolicy = str
_SCRIPT_MATERIALIZATION_POLICIES = {"off", "auto", "require"}


class ScriptExecutionSerializationError(DccMcpError, TypeError):
    """Raised when a strict script result cannot be JSON-encoded."""

    pass


@dataclass(frozen=True)
class ScriptExecutionParams:
    """Normalized script execution parameters shared by DCC adapters."""

    code: str
    timeout_secs: int | None = None
    params: dict[str, Any] = field(default_factory=dict)
    params_provided: bool = False


@dataclass(frozen=True)
class FileBackedScriptExecutionParams:
    """Normalized script execution request after file-backed policy handling."""

    code: str
    file_path: str | None
    timeout_secs: int | None = None
    materialized_script: MaterializedScript | None = None
    source: str = "inline"
    sha256: str | None = None
    bytes: int | None = None
    params: dict[str, Any] = field(default_factory=dict)
    params_provided: bool = False
    parameters_schema: dict[str, Any] | None = None

    @property
    def is_file_backed(self) -> bool:
        """Return true when execution has a concrete host-local file path."""
        return self.file_path is not None

    def materialized_context(self) -> dict[str, Any] | None:
        """Return standardized ToolResult context metadata."""
        if self.materialized_script is not None:
            return _materialized_script_context(self.materialized_script)
        if self.file_path is None:
            return None
        path = Path(self.file_path)
        return {
            "path": self.file_path,
            "file_path": self.file_path,
            "file_ref": _file_ref_for_script_path(path, sha256=self.sha256, bytes_=self.bytes),
            "sha256": self.sha256,
            "bytes": self.bytes,
            "reused": False,
            "source": self.source,
            "parameters_schema": self.parameters_schema,
        }


def normalize_script_execution_params(
    params: Mapping[str, Any],
    *,
    default_timeout_secs: int | None = None,
) -> ScriptExecutionParams:
    """Normalize **inline** script parameters to ``code`` and ``timeout_secs``.

    This helper is for adapters that execute a **string** body. Callers that
    support ``file_path`` / ``script_path`` (run a ``.py`` from disk) must read
    the file first and/or bypass this function — passing only ``file_path`` is
    invalid here because ``code`` is required.
    """
    if default_timeout_secs is not None and default_timeout_secs <= 0:
        raise ValueError("default_timeout_secs must be greater than zero")

    if params.get("code") is None:
        raise ValueError("Missing required 'code' string")

    code = params["code"]
    if not isinstance(code, str):
        raise TypeError("code must be a string")

    timeout_secs = default_timeout_secs
    if params.get("timeout_secs") is not None:
        timeout_value = params["timeout_secs"]
        if isinstance(timeout_value, bool) or not isinstance(timeout_value, int):
            raise TypeError("timeout_secs must be an integer number of seconds")
        if timeout_value <= 0:
            raise ValueError("timeout_secs must be greater than zero")
        timeout_secs = timeout_value

    structured_params, params_provided = _normalize_structured_params(params)
    return ScriptExecutionParams(
        code=code,
        timeout_secs=timeout_secs,
        params=structured_params,
        params_provided=params_provided,
    )


def normalize_file_backed_script_execution_params(
    params: Mapping[str, Any],
    *,
    dcc_type: str,
    instance_id: str,
    session_id: str,
    policy: ScriptMaterializationPolicy = "auto",
    trusted_roots: Sequence[str | Path] = (),
    materialization_root: str | Path | None = None,
    language: str = "python",
    suffix: str = ".py",
    default_timeout_secs: int | None = None,
    ttl_secs: int | None = None,
    tool_call_id: str | None = None,
    correlation_id: str | None = None,
    reuse: bool = False,
    reuse_key: str | None = None,
) -> FileBackedScriptExecutionParams:
    """Normalize script execution params through the file-backed policy.

    ``policy="auto"`` materializes inline ``code`` into the canonical store.
    ``policy="require"`` rejects raw inline code and accepts only explicit
    trusted file paths. ``policy="off"`` preserves legacy inline execution.
    """
    policy = _normalize_materialization_policy(policy)
    timeout_secs = _normalize_timeout(params, default_timeout_secs=default_timeout_secs)
    structured_params, params_provided = _normalize_structured_params(params)
    expected_sha256 = _normalize_expected_sha256(params)
    file_path = _first_string(params, "file_path", "script_path")

    if file_path is not None:
        trusted_path = validate_script_file_path(
            file_path,
            trusted_roots=trusted_roots,
            materialization_root=materialization_root,
        )
        code = trusted_path.read_text(encoding="utf-8")
        descriptor = None
        store_root = default_script_materialization_root(materialization_root).resolve()
        try:
            trusted_path.relative_to(store_root)
        except ValueError:
            pass
        else:
            descriptor = resolve_materialized_script(trusted_path, root=store_root)
        parameters_schema = derive_script_parameters_schema(code)
        if (
            descriptor is not None
            and descriptor.parameters_schema is not None
            and descriptor.parameters_schema != parameters_schema
        ):
            raise ValueError("materialized script parameters_schema does not match the verified script body")
        actual_sha256 = descriptor.sha256 if descriptor is not None else _hash_text(code)
        if expected_sha256 is not None and expected_sha256 != actual_sha256:
            raise ValueError("sha256 does not match the script file")
        _validate_structured_script_params(parameters_schema, structured_params, params_provided=params_provided)
        return FileBackedScriptExecutionParams(
            code=code,
            file_path=str(trusted_path),
            timeout_secs=timeout_secs,
            materialized_script=descriptor,
            source="materialized_file" if descriptor is not None else "file_path",
            sha256=actual_sha256,
            bytes=descriptor.bytes if descriptor is not None else len(code.encode("utf-8")),
            params=structured_params,
            params_provided=params_provided,
            parameters_schema=parameters_schema,
        )

    code = _required_code(params)
    if policy == "require":
        raise ValueError(
            "Inline code is not allowed when script_materialization_policy=require; "
            "materialize the script first and pass file_path",
        )
    if policy == "off":
        if params_provided:
            raise ValueError("params require a file-backed script; use policy=auto or pass file_path")
        actual_sha256 = _hash_text(code)
        if expected_sha256 is not None and expected_sha256 != actual_sha256:
            raise ValueError("sha256 does not match inline code")
        return FileBackedScriptExecutionParams(
            code=code,
            file_path=None,
            timeout_secs=timeout_secs,
            source="inline",
            sha256=actual_sha256,
            bytes=len(code.encode("utf-8")),
            params=structured_params,
            params_provided=params_provided,
        )

    actual_sha256 = _hash_text(code)
    if expected_sha256 is not None and expected_sha256 != actual_sha256:
        raise ValueError("sha256 does not match inline code")
    parameters_schema = derive_script_parameters_schema(code)
    _validate_structured_script_params(parameters_schema, structured_params, params_provided=params_provided)

    descriptor = materialize_script(
        code,
        dcc_type=dcc_type,
        instance_id=instance_id,
        session_id=session_id,
        language=language,
        suffix=suffix,
        ttl_secs=ttl_secs,
        root=materialization_root,
        tool_call_id=tool_call_id,
        correlation_id=correlation_id,
        reuse=reuse,
        reuse_key=reuse_key,
    )
    path = Path(descriptor.file_path)
    return FileBackedScriptExecutionParams(
        code=path.read_text(encoding="utf-8"),
        file_path=descriptor.file_path,
        timeout_secs=timeout_secs,
        materialized_script=descriptor,
        source="materialized",
        sha256=descriptor.sha256,
        bytes=descriptor.bytes,
        params=structured_params,
        params_provided=params_provided,
        parameters_schema=descriptor.parameters_schema,
    )


def validate_script_file_path(
    file_path: str | Path,
    *,
    trusted_roots: Sequence[str | Path] = (),
    materialization_root: str | Path | None = None,
) -> Path:
    """Validate that ``file_path`` exists and belongs to a trusted root."""
    path = Path(file_path).expanduser()
    if not path.is_file():
        raise FileNotFoundError(f"Script file not found: {file_path}")

    resolved = path.resolve()
    roots = [default_script_materialization_root(materialization_root)]
    roots.extend(Path(root).expanduser() for root in trusted_roots)
    for root in roots:
        root_path = root.resolve() if root.exists() else root
        try:
            resolved.relative_to(root_path)
            return resolved
        except ValueError:
            continue
    raise ValueError(f"Script file is outside trusted roots: {file_path}")


def allow_script_materialization_root(
    policy: Any,
    *,
    root: str | Path | None = None,
) -> Path:
    """Add the script materialization root to a sandbox policy allowlist."""
    root_path = default_script_materialization_root(root).resolve()
    root_path.mkdir(parents=True, exist_ok=True)
    if _sandbox_allows_path(policy, root_path / "__dcc_mcp_materialization_probe__.py"):
        return root_path
    allow_paths = getattr(policy, "allow_paths", None)
    if not callable(allow_paths):
        raise TypeError("sandbox policy must expose allow_paths(paths)")
    allow_paths([str(root_path)])
    return root_path


def _sandbox_allows_path(policy: Any, path: Path) -> bool:
    try:
        from dcc_mcp_core import SandboxContext

        return bool(SandboxContext(policy).is_path_allowed(str(path)))
    except Exception:
        return False


def _normalize_materialization_policy(policy: str) -> ScriptMaterializationPolicy:
    if policy not in _SCRIPT_MATERIALIZATION_POLICIES:
        raise ValueError("script_materialization_policy must be one of: off, auto, require")
    return policy  # type: ignore[return-value]


def _normalize_timeout(
    params: Mapping[str, Any],
    *,
    default_timeout_secs: int | None,
) -> int | None:
    if default_timeout_secs is not None and default_timeout_secs <= 0:
        raise ValueError("default_timeout_secs must be greater than zero")
    timeout_secs = default_timeout_secs
    if params.get("timeout_secs") is not None:
        timeout_value = params["timeout_secs"]
        if isinstance(timeout_value, bool) or not isinstance(timeout_value, int):
            raise TypeError("timeout_secs must be an integer number of seconds")
        if timeout_value <= 0:
            raise ValueError("timeout_secs must be greater than zero")
        timeout_secs = timeout_value
    return timeout_secs


def _normalize_structured_params(params: Mapping[str, Any]) -> tuple[dict[str, Any], bool]:
    if "params" not in params:
        return {}, False
    value = params["params"]
    if not isinstance(value, Mapping):
        raise TypeError("params must be an object")
    return dict(value), True


def _normalize_expected_sha256(params: Mapping[str, Any]) -> str | None:
    value = params.get("sha256")
    if value is None:
        return None
    if not isinstance(value, str):
        raise TypeError("sha256 must be a string")
    normalized = (value[7:] if value.startswith("sha256:") else value).lower()
    if len(normalized) != 64 or any(character not in "0123456789abcdef" for character in normalized):
        raise ValueError("sha256 must contain 64 hexadecimal characters")
    return normalized


def _validate_structured_script_params(
    parameters_schema: Mapping[str, Any] | None,
    params: Mapping[str, Any],
    *,
    params_provided: bool,
) -> None:
    if not params_provided:
        return
    if parameters_schema is None:
        raise ValueError("params require a fully typed main(...) entry point")

    from dcc_mcp_core.skills_helper import ToolValidator

    valid, errors = ToolValidator.from_schema_json(json.dumps(parameters_schema)).validate(json.dumps(params))
    if not valid:
        raise ValueError(f"params failed schema validation: {'; '.join(errors)}")


def _required_code(params: Mapping[str, Any]) -> str:
    if params.get("code") is None:
        raise ValueError("Missing required 'code' string")
    code = params["code"]
    if not isinstance(code, str):
        raise TypeError("code must be a string")
    return code


def _first_string(params: Mapping[str, Any], *names: str) -> str | None:
    for name in names:
        value = params.get(name)
        if value is None:
            continue
        if not isinstance(value, str):
            raise TypeError(f"{name} must be a string")
        if value:
            return value
    return None


def _hash_text(code: str) -> str:
    return hashlib.sha256(code.encode("utf-8")).hexdigest()


def _materialized_script_context(
    materialized_script: MaterializedScript | FileBackedScriptExecutionParams | Mapping[str, Any],
) -> dict[str, Any]:
    if isinstance(materialized_script, FileBackedScriptExecutionParams):
        context = materialized_script.materialized_context()
        return {} if context is None else context
    if isinstance(materialized_script, MaterializedScript):
        return {
            "schema_version": 1,
            "producer": "dcc-mcp-core.script_materialization",
            "path": materialized_script.file_path,
            "file_path": materialized_script.file_path,
            "file_ref": materialized_script.file_ref,
            "sha256": materialized_script.sha256,
            "bytes": materialized_script.bytes,
            "reused": materialized_script.reused,
            "expires_at": materialized_script.expires_at,
            "ttl_secs": materialized_script.ttl_secs,
            "session_id": materialized_script.session_id,
            "tool_call_id": materialized_script.tool_call_id,
            "correlation_id": materialized_script.correlation_id,
            "reuse_key": materialized_script.reuse_key,
            "parameters_schema": materialized_script.parameters_schema,
        }
    context = dict(materialized_script)
    context.setdefault("schema_version", 1)
    context.setdefault("producer", "dcc-mcp-core.script_materialization")
    return context


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


class ScriptExecutionCapture(AbstractContextManager):
    """Capture ``sys.stdout`` and ``sys.stderr`` during host script execution.

    ``tee=True`` keeps host-console visibility while still collecting output
    for the tool response. This mirrors DCC plugin expectations where artists
    should continue seeing script output in the native console.

    ``output_capture`` accepts an ``OutputCapture`` (the Rust ``output://``
    ring-buffer object). When supplied, ``ScriptExecutionCapture`` calls
    ``output_capture.set_paused(True)`` on ``__enter__`` and
    ``set_paused(False)`` on ``__exit__``, preventing the ``output://``
    resource from accumulating a mangled duplicate of the output that this
    context already captures cleanly via ``sys.stdout`` replacement
    (issue #856).

    The object only needs to expose a ``set_paused(bool)`` method; it does
    not need to be the exact ``OutputCapture`` class so test doubles work.
    """

    def __init__(self, *, tee: bool = False, output_capture: Any = None) -> None:
        self._tee = tee
        self._output_capture = output_capture
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
        # Suspend the output:// ring buffer so Maya Script Editor output
        # during this script body does not produce a mangled duplicate in
        # the response envelope (issue #856).
        if self._output_capture is not None:
            with contextlib.suppress(Exception):
                self._output_capture.set_paused(True)
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        if self._old_stdout is not None:
            sys.stdout = self._old_stdout
        if self._old_stderr is not None:
            sys.stderr = self._old_stderr
        # Always resume, even on exception, so spontaneous Maya warnings
        # between calls keep reaching the output:// resource.
        if self._output_capture is not None:
            with contextlib.suppress(Exception):
                self._output_capture.set_paused(False)

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
        materialized_script: MaterializedScript | FileBackedScriptExecutionParams | Mapping[str, Any] | None = None,
        postcondition: Mapping[str, Any] | None = None,
        scene_digest_before: SceneDigestSnapshot | None = None,
        scene_digest_after: SceneDigestSnapshot | None = None,
        verified: bool | None = None,
    ) -> dict[str, Any]:
        """Return a success envelope, or a strict serialization error envelope."""
        digest_evidence = _scene_digest_postcondition(
            scene_digest_before,
            scene_digest_after,
            postcondition=postcondition,
            verified=verified,
        )
        if isinstance(digest_evidence, dict) and digest_evidence.get("success") is False:
            if not isinstance(digest_evidence.get("message"), str) or not isinstance(digest_evidence.get("error"), str):
                return ToolResultEnvelope.fail(
                    "Scene digest evidence could not be normalized",
                    error="invalid_scene_digest_evidence",
                ).to_dict()
            return digest_evidence
        use_repr = not strict_json if repr_fallback is None else repr_fallback
        try:
            normalized = _normalize_result(
                result,
                strict_json=strict_json,
                repr_fallback=use_repr,
            )
        except ScriptExecutionSerializationError as exc:
            serialization_failure = ToolResultEnvelope.fail(
                str(exc),
                error="non_serializable_result",
                stdout=stdout,
                stderr=stderr,
            ).to_dict()
            if digest_evidence is not None:
                serialization_failure["postcondition"] = digest_evidence
            return serialization_failure

        context = {
            "result": normalized,
            "stdout": stdout,
            "stderr": stderr,
        }
        if materialized_script is not None:
            context["materialized_script"] = _materialized_script_context(materialized_script)
        return ToolResultEnvelope.ok(message, postcondition=digest_evidence, **context).to_dict()

    @staticmethod
    def from_outcome(
        outcome: SceneDigestExecution,
        *,
        stdout: str = "",
        stderr: str = "",
        strict_json: bool = True,
        repr_fallback: bool | None = None,
        message: str = "Script executed successfully",
        materialized_script: MaterializedScript | FileBackedScriptExecutionParams | Mapping[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Build an envelope from one before/after execution outcome."""
        if not isinstance(outcome, SceneDigestExecution):
            raise TypeError("outcome must be a SceneDigestExecution")
        return ScriptExecutionResult.from_value(
            outcome.value,
            stdout=stdout,
            stderr=stderr,
            strict_json=strict_json,
            repr_fallback=repr_fallback,
            message=message,
            materialized_script=materialized_script,
            scene_digest_before=outcome.scene_digest_before,
            scene_digest_after=outcome.scene_digest_after,
        )

    @staticmethod
    def from_exception(
        exc: BaseException,
        *,
        stdout: str = "",
        stderr: str = "",
        message: str | None = None,
        scene_digest_before: SceneDigestSnapshot | None = None,
        scene_digest_after: SceneDigestSnapshot | None = None,
        readback_error: SceneDigestError | None = None,
    ) -> dict[str, Any]:
        """Return a structured failure envelope with traceback and captured output."""
        return _script_execution_failure(
            exc,
            stdout=stdout,
            stderr=stderr,
            message=message,
            scene_digest_before=scene_digest_before,
            scene_digest_after=scene_digest_after,
            readback_error=readback_error,
        )


__all__ = [
    "FileBackedScriptExecutionParams",
    "SceneDigestError",
    "SceneDigestExecution",
    "SceneDigestExecutionError",
    "SceneDigestSnapshot",
    "ScriptExecutionCapture",
    "ScriptExecutionContext",
    "ScriptExecutionParams",
    "ScriptExecutionResult",
    "ScriptExecutionSerializationError",
    "StateDigestProvider",
    "allow_script_materialization_root",
    "capture_state_digest",
    "cleanup_temp_scripts",
    "clear_script_namespace",
    "execute_with_context",
    "execute_with_state_digest",
    "get_default_script_execution_context",
    "get_script_namespace",
    "normalize_file_backed_script_execution_params",
    "normalize_script_execution_params",
    "register_dcc_namespace",
    "register_state_digest_provider",
    "reset_default_script_execution_context_for_tests",
    "update_script_namespace",
    "validate_script_file_path",
    "write_temp_script",
]
