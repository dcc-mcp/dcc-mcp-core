"""Reusable script execution capture and result envelopes (#603).

DCC adapters expose ad-hoc script execution tools such as ``execute_python``.
Those tools need the same stdout/stderr capture behaviour and the same
``ToolResult``-shaped return contract, independent of the host application.

Additions (2026-05):
- Temp-file execution API to avoid code-string escaping / size limits.
- Persistent script namespace so multi-step workflows can share variables
  (IDE-style execution where later steps see earlier results).
- ``register_dcc_namespace()`` lets a DCC adapter inject its live
  ``__main__`` globals so that scripts can call ``cmds``, ``hou``, etc.
"""

from __future__ import annotations

from collections.abc import Mapping
import contextlib
from contextlib import AbstractContextManager
from dataclasses import dataclass
import hashlib
import io
import json
from pathlib import Path
import sys
import traceback
from typing import Any
from typing import Sequence
from typing import TextIO

from dcc_mcp_core.result_envelope import ToolResult
from dcc_mcp_core.script_materialization import MaterializedScript
from dcc_mcp_core.script_materialization import cleanup_materialized_scripts
from dcc_mcp_core.script_materialization import default_script_materialization_root
from dcc_mcp_core.script_materialization import materialize_script

ScriptMaterializationPolicy = str
_SCRIPT_MATERIALIZATION_POLICIES = {"off", "auto", "require"}


class ScriptExecutionSerializationError(TypeError):
    """Raised when a strict script result cannot be JSON-encoded."""

    pass


@dataclass(frozen=True)
class ScriptExecutionParams:
    """Normalized script execution parameters shared by DCC adapters."""

    code: str
    timeout_secs: int | None = None


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

    return ScriptExecutionParams(code=code, timeout_secs=timeout_secs)


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
    file_path = _first_string(params, "file_path", "script_path")

    if file_path is not None:
        trusted_path = validate_script_file_path(
            file_path,
            trusted_roots=trusted_roots,
            materialization_root=materialization_root,
        )
        code = trusted_path.read_text(encoding="utf-8")
        return FileBackedScriptExecutionParams(
            code=code,
            file_path=str(trusted_path),
            timeout_secs=timeout_secs,
            source="file_path",
            sha256=_hash_text(code),
            bytes=len(code.encode("utf-8")),
        )

    code = _required_code(params)
    if policy == "require":
        raise ValueError(
            "Inline code is not allowed when script_materialization_policy=require; "
            "materialize the script first and pass file_path",
        )
    if policy == "off":
        return FileBackedScriptExecutionParams(
            code=code,
            file_path=None,
            timeout_secs=timeout_secs,
            source="inline",
            sha256=_hash_text(code),
            bytes=len(code.encode("utf-8")),
        )

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
        }
    return dict(materialized_script)


def _file_ref_for_script_path(path: Path, *, sha256: str | None, bytes_: int | None) -> dict[str, Any]:
    resolved = path.resolve()
    file_ref = {
        "uri": resolved.as_uri(),
        "mime": _mime_for_script_path(resolved),
        "size_bytes": bytes_,
        "display_name": resolved.name,
        "digest": f"sha256:{sha256}" if sha256 else None,
        "metadata": {
            "materialization_kind": "script",
            "source": "file_path",
        },
    }
    return {key: value for key, value in file_ref.items() if value is not None}


def _mime_for_script_path(path: Path) -> str:
    suffix = path.suffix.lower()
    if suffix == ".py":
        return "text/x-python"
    if suffix == ".mel":
        return "text/x-mel"
    if suffix == ".js":
        return "text/javascript"
    if suffix == ".ps1":
        return "text/x-powershell"
    return "text/plain"


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

        context = {
            "result": normalized,
            "stdout": stdout,
            "stderr": stderr,
        }
        if materialized_script is not None:
            context["materialized_script"] = _materialized_script_context(materialized_script)
        return ToolResult.ok(message, **context).to_dict()

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
    "FileBackedScriptExecutionParams",
    "ScriptExecutionCapture",
    "ScriptExecutionParams",
    "ScriptExecutionResult",
    "ScriptExecutionSerializationError",
    "allow_script_materialization_root",
    "cleanup_temp_scripts",
    "clear_script_namespace",
    "execute_with_context",
    "get_script_namespace",
    "normalize_file_backed_script_execution_params",
    "normalize_script_execution_params",
    "register_dcc_namespace",
    "validate_script_file_path",
    "write_temp_script",
]


# ---------------------------------------------------------------------------
# Temp-script file management
# ---------------------------------------------------------------------------
# ``write_temp_script(...)`` is the legacy convenience wrapper for callers
# that only need a path string. New code should use
# ``script_materialization.materialize_script(...)`` directly so audit,
# cleanup, reuse, and FileRef-compatible metadata stay available.
#
# Cleanup happens automatically on interpreter exit; adapters can also
# call ``cleanup_temp_scripts()`` explicitly when the DCC server shuts down.

_TEMP_SCRIPT_DIR = Path.home() / ".dcc-mcp-core" / "temp_scripts"


def write_temp_script(
    content: str,
    *,
    suffix: str = ".py",
    prefix: str = "dcc_mcp_",
) -> str:
    """Write *content* to a managed temp file and return the absolute path.

    The file is created through the script materialization store so that
    both the AI agent (producer) and the in-process executor (consumer)
    agree on the location without passing the code over JSON.

    Args:
        content: Python source to write.
        suffix: Filename suffix (default ``.py``).
        prefix: Filename prefix (default ``dcc_mcp_``).

    Returns:
        Absolute path of the created temp file.

    """
    descriptor = materialize_script(
        content,
        dcc_type="generic",
        instance_id="local",
        session_id="default",
        suffix=suffix,
        root=_TEMP_SCRIPT_DIR,
        prefix=prefix,
    )
    return descriptor.file_path


def cleanup_temp_scripts() -> None:
    """Delete all files under the managed temp-script directory.

    Safe to call multiple times; missing directory is ignored.
    Adapters should call this on server shutdown (``register_quit_hook``).
    """
    if not _TEMP_SCRIPT_DIR.is_dir():
        return
    cleanup_materialized_scripts(root=_TEMP_SCRIPT_DIR, include_unexpired=True)
    for p in _TEMP_SCRIPT_DIR.iterdir():
        with contextlib.suppress(Exception):
            if p.is_file() or p.is_symlink():
                p.unlink()


# ---------------------------------------------------------------------------
# Persistent script namespace  (IDE-style variable sharing)
# ---------------------------------------------------------------------------
# ``_SCRIPT_NAMESPACE`` accumulates variables assigned by executed scripts,
# so that a later ``execute_python`` call can reference results from an
# earlier call - the same way an IDE Python console persists state.
#
# ``_DCC_NAMESPACE`` holds the DCC application's live globals
# (``cmds``, ``hou``, ``bpy``, ...).  Adapters populate it once at
# startup via ``register_dcc_namespace(vars(__main__))``.

_SCRIPT_NAMESPACE: dict[str, Any] = {}
_DCC_NAMESPACE: dict[str, Any] = {}


def register_dcc_namespace(ns: dict[str, Any]) -> None:
    """Make *ns* available as the DCC globals during script execution.

    Call once at adapter startup so that scripts can call DCC commands
    without having to ``import pymel as pm`` or
    ``import maya.cmds as cmds`` in every snippet.

    Example (Maya adapter)::

        import __main__
        from dcc_mcp_core.script_execution import register_dcc_namespace
        register_dcc_namespace(vars(__main__))

    """
    global _DCC_NAMESPACE
    _DCC_NAMESPACE = ns


def get_script_namespace() -> dict[str, Any]:
    """Return a copy of the persistent script namespace."""
    return dict(_SCRIPT_NAMESPACE)


def clear_script_namespace() -> None:
    """Reset the persistent script namespace (useful before a fresh workflow)."""
    global _SCRIPT_NAMESPACE
    _SCRIPT_NAMESPACE.clear()


def _make_exec_namespace() -> dict[str, Any]:
    """Build the globals dict for ``exec()``.

    Merge order (later wins):
    1. ``_DCC_NAMESPACE``  - DCC application globals
    2. ``_SCRIPT_NAMESPACE`` - variables from earlier executions
    3. ``__builtins__`` - always available
    """
    ns: dict[str, Any] = {}
    ns.update(_DCC_NAMESPACE)
    ns.update(_SCRIPT_NAMESPACE)
    return ns


def execute_with_context(code: str, *, filename: str = "<execute_python>") -> Any:
    """Execute *code* with DCC globals + persistent namespace.

    Returns the value of the ``result`` variable if the script assigns one,
    otherwise ``None``.

    The persistent namespace is **updated in-place** after execution so
    that newly-assigned variables are visible to the next call.
    """
    ns = _make_exec_namespace()
    local_ns: dict[str, Any] = {}
    exec(compile(code, filename, "exec"), ns, local_ns)
    _SCRIPT_NAMESPACE.update(local_ns)
    return local_ns.get("result")
