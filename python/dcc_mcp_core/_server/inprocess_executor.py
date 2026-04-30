"""In-process Python skill execution for embedded DCC adapters (issue #521).

Lifts the `_wire_in_process_executor` / `_run_skill_script` pattern that
`dcc-mcp-maya` 0.2.19 implements (~150 LOC in `server.py`) into a
DCC-neutral helper. Every embedded DCC plugin (Maya, Houdini, Unreal,
Blender Python …) needs the exact same flow:

1. Run the skill script in the live DCC interpreter (no subprocess).
2. Route the script through a host dispatcher so it executes on the
   UI thread.
3. Honour the ``main(**params)`` calling convention with the
   ``SystemExit + __mcp_result__`` fallback used by skill authors.
4. Return a JSON-serialisable :class:`ToolResult`-shaped dict.

The actual MCP wiring stays in
:meth:`McpHttpServer.set_in_process_executor` (already shipped, see
issues #464/#465). This module supplies the *executor closure* that
satisfies that callable contract and the dispatcher protocol it routes
through.
"""

# Import built-in modules
from __future__ import annotations

from dataclasses import dataclass
import importlib.util
import json
import logging
from pathlib import Path
import sys
import time
import traceback
from typing import TYPE_CHECKING
from typing import Any
from typing import Callable
from typing import Mapping
import uuid

if TYPE_CHECKING:
    pass

# `typing.Protocol` / `typing.runtime_checkable` are 3.8+. The package
# still claims `requires-python = ">=3.7"`, so on 3.7 we expose
# `BaseDccCallableDispatcher` as a plain duck-typed class with the same
# `dispatch_callable` attribute contract; concrete dispatchers do not
# need to inherit from it either way.
if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    def runtime_checkable(cls):
        return cls

    class Protocol:  # type: ignore[no-redef]
        pass


logger = logging.getLogger(__name__)

__all__ = [
    "BaseDccCallableDispatcher",
    "DeferredToolResult",
    "HostExecutionBridge",
    "InProcessExecutionContext",
    "build_inprocess_executor",
    "exception_to_error_envelope",
    "run_skill_script",
]


@dataclass(frozen=True)
class InProcessExecutionContext:
    """Execution metadata for a single in-process skill-script call."""

    action_name: str = ""
    skill_name: str | None = None
    thread_affinity: str = "any"
    execution: str = "sync"
    timeout_hint_secs: int | None = None


@dataclass
class DeferredToolResult:
    """Deferred completion handle returned by long-running host operations.

    A skill script or direct host callable may return this object after it
    starts a host-native background operation. ``HostExecutionBridge`` polls
    ``check_is_finished`` until it returns a final JSON-serialisable result.
    Returning ``None`` means "still running".
    """

    check_is_finished: Callable[[], Any]
    timeout_secs: float = 3600.0
    poll_interval_secs: float = 0.1
    stdout: str = ""
    stderr: str = ""

    def __post_init__(self) -> None:
        if not callable(self.check_is_finished):
            raise TypeError("check_is_finished must be callable")
        if self.timeout_secs <= 0:
            raise ValueError("timeout_secs must be > 0")
        if self.poll_interval_secs <= 0:
            raise ValueError("poll_interval_secs must be > 0")


def _context_from_kwargs(
    *,
    action_name: str = "",
    skill_name: str | None = None,
    thread_affinity: str = "any",
    execution: str = "sync",
    timeout_hint_secs: int | None = None,
) -> InProcessExecutionContext:
    return InProcessExecutionContext(
        action_name=action_name,
        skill_name=skill_name,
        thread_affinity=thread_affinity or "any",
        execution=execution or "sync",
        timeout_hint_secs=timeout_hint_secs,
    )


def exception_to_error_envelope(exc: BaseException, *, message: str | None = None) -> dict[str, Any]:
    """Render *exc* as a structured ``ToolResult``-shaped error dict.

    The returned envelope mirrors the wire shape clients already receive
    on success — ``success`` / ``message`` / ``error`` (issue #589) — so
    Rust ``CallToolResult`` construction can flag ``isError: true`` from
    the same ``success: false`` heuristic without any extra string
    parsing on the client side.

    The traceback is folded into ``error.traceback`` (single string,
    pre-formatted) so MCP clients can render it inline. Skill authors
    catching exceptions inside ``main`` can reuse this helper to keep
    the envelope shape consistent across in-process and subprocess
    execution.
    """
    msg = message if message is not None else f"Execution failed: {exc}"
    return {
        "success": False,
        "message": msg,
        "error": {
            "type": type(exc).__name__,
            "message": str(exc),
            "traceback": "".join(traceback.format_exception(type(exc), exc, exc.__traceback__)),
        },
    }


def _attach_deferred_streams(result: Any, deferred: DeferredToolResult) -> Any:
    """Attach initial stdout/stderr captured before deferred completion."""
    if not deferred.stdout and not deferred.stderr:
        return result

    meta = {
        "stdout": deferred.stdout,
        "stderr": deferred.stderr,
    }
    if isinstance(result, dict):
        enriched = dict(result)
        existing_meta = enriched.get("_meta")
        merged_meta = dict(existing_meta) if isinstance(existing_meta, dict) else {}
        merged_meta["dcc.deferred"] = meta
        enriched["_meta"] = merged_meta
        return enriched

    return {
        "result": result,
        "_meta": {
            "dcc.deferred": meta,
        },
    }


@runtime_checkable
class BaseDccCallableDispatcher(Protocol):
    """Protocol every DCC dispatcher must satisfy to receive in-process calls.

    The dispatcher submits ``func`` to the DCC's UI / main thread (Maya
    deferred queue, Houdini ``hou.session``, Unreal game thread …) and
    returns the script's result. Implementations are free to be
    synchronous (block on a queue) or to dispatch through a futures
    object internally; from the executor's point of view, the call is
    a plain ``func(*args, **kwargs)`` invocation that may take time.

    Concrete dispatchers do not need to inherit from this protocol —
    duck typing is enough — but tagging implementations explicitly
    enables runtime ``isinstance(dispatcher, BaseDccCallableDispatcher)``
    sanity checks.
    """

    def dispatch_callable(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        """Run *func* on the host's main / UI thread; return the result."""
        ...


@dataclass
class HostExecutionBridge:
    """Adapter-facing bridge for host-owned Python execution.

    The bridge is the single Python object adapters can keep around for
    in-process skill scripts and direct callable dispatch. It deliberately
    wraps the existing ``set_in_process_executor`` callable contract so
    current Rust/PyO3 wiring remains unchanged while adapters get one
    concept to configure.
    """

    dispatcher: BaseDccCallableDispatcher | None = None
    runner: Callable[[str, Mapping[str, Any]], Any] | None = None
    default_thread_affinity: str = "any"
    default_execution: str = "sync"
    default_timeout_hint_secs: int | None = None

    def execution_context(
        self,
        *,
        action_name: str = "",
        skill_name: str | None = None,
        thread_affinity: str | None = None,
        execution: str | None = None,
        timeout_hint_secs: int | None = None,
    ) -> InProcessExecutionContext:
        """Build the normalized metadata envelope passed to dispatchers."""
        return _context_from_kwargs(
            action_name=action_name,
            skill_name=skill_name,
            thread_affinity=thread_affinity or self.default_thread_affinity,
            execution=execution or self.default_execution,
            timeout_hint_secs=timeout_hint_secs if timeout_hint_secs is not None else self.default_timeout_hint_secs,
        )

    def dispatch_callable(
        self,
        func: Callable[..., Any],
        *args: Any,
        action_name: str = "",
        skill_name: str | None = None,
        thread_affinity: str | None = None,
        execution: str | None = None,
        timeout_hint_secs: int | None = None,
        **kwargs: Any,
    ) -> Any:
        """Run a Python callable through the configured host dispatcher."""
        context = self.execution_context(
            action_name=action_name,
            skill_name=skill_name,
            thread_affinity=thread_affinity,
            execution=execution,
            timeout_hint_secs=timeout_hint_secs,
        )
        result = self._dispatch_raw(func, args, kwargs, context)
        return self._resolve_deferred_result(result, context)

    def _dispatch_raw(
        self,
        func: Callable[..., Any],
        args: tuple[Any, ...],
        kwargs: Mapping[str, Any],
        context: InProcessExecutionContext,
    ) -> Any:
        """Dispatch a callable without resolving DeferredToolResult values."""

        def _invoke(*_args: Any, **_kwargs: Any) -> Any:
            return func(*args, **kwargs)

        try:
            if self.dispatcher is None:
                return _invoke()
            return self.dispatcher.dispatch_callable(
                _invoke,
                affinity=context.thread_affinity,
                context=context,
                action_name=context.action_name,
                skill_name=context.skill_name,
                execution=context.execution,
                timeout_hint_secs=context.timeout_hint_secs,
            )
        except Exception as exc:
            logger.exception("Host callable %s failed", getattr(func, "__name__", repr(func)))
            return exception_to_error_envelope(exc)

    def _resolve_deferred_result(
        self,
        result: Any,
        context: InProcessExecutionContext,
    ) -> Any:
        """Poll a DeferredToolResult until it yields a final result."""
        if not isinstance(result, DeferredToolResult):
            return result

        deadline = time.monotonic() + result.timeout_secs
        while True:
            if time.monotonic() >= deadline:
                envelope = exception_to_error_envelope(
                    TimeoutError(f"Deferred tool timed out after {result.timeout_secs:g}s"),
                    message="Deferred tool did not finish before timeout",
                )
                return _attach_deferred_streams(envelope, result)

            try:
                finished = self._dispatch_raw(
                    result.check_is_finished,
                    (),
                    {},
                    context,
                )
            except Exception as exc:  # pragma: no cover - _dispatch_raw normalises
                finished = exception_to_error_envelope(exc)

            if finished is not None:
                if isinstance(finished, DeferredToolResult):
                    envelope = exception_to_error_envelope(
                        TypeError("Nested DeferredToolResult is not supported"),
                        message="Deferred tool returned another deferred result",
                    )
                    return _attach_deferred_streams(envelope, result)
                try:
                    json.dumps(finished)
                except TypeError as exc:
                    envelope = exception_to_error_envelope(
                        exc,
                        message="Deferred tool returned a non-serialisable result",
                    )
                    return _attach_deferred_streams(envelope, result)
                return _attach_deferred_streams(finished, result)

            time.sleep(result.poll_interval_secs)

    def execute_script(
        self,
        script_path: str,
        params: Mapping[str, Any],
        *,
        action_name: str = "",
        skill_name: str | None = None,
        thread_affinity: str | None = None,
        execution: str | None = None,
        timeout_hint_secs: int | None = None,
    ) -> Any:
        """Execute a skill script using the same bridge as direct callables."""
        return self.dispatch_callable(
            self.runner or run_skill_script,
            script_path,
            params,
            action_name=action_name,
            skill_name=skill_name,
            thread_affinity=thread_affinity,
            execution=execution,
            timeout_hint_secs=timeout_hint_secs,
        )

    def as_inprocess_executor(self) -> Callable[..., Any]:
        """Return the callable expected by ``set_in_process_executor``."""

        def _executor(
            script_path: str,
            params: Mapping[str, Any],
            *,
            action_name: str = "",
            skill_name: str | None = None,
            thread_affinity: str = "any",
            execution: str = "sync",
            timeout_hint_secs: int | None = None,
        ) -> Any:
            return self.execute_script(
                script_path,
                params,
                action_name=action_name,
                skill_name=skill_name,
                thread_affinity=thread_affinity,
                execution=execution,
                timeout_hint_secs=timeout_hint_secs,
            )

        return _executor


def run_skill_script(script_path: str, params: Mapping[str, Any]) -> Any:
    """Lazy-import a skill script and call its ``main(**params)``.

    Mirrors the convention skill authors already use:

    * Module is loaded with a unique synthetic name to keep import
      caches from colliding when the same path is loaded twice.
    * ``main`` is the entry point; raise an explicit error if the
      module does not expose it.
    * ``SystemExit`` is intercepted because some DCCs raise it from
      inside ``main`` to bail out of the host's event loop; in that
      case the script is expected to publish a result via
      ``module.__mcp_result__`` before exiting.
    """
    path = Path(script_path)
    if not path.is_file():
        raise FileNotFoundError(f"Skill script not found: {script_path}")

    mod_name = f"_dcc_mcp_inproc_{uuid.uuid4().hex}"
    spec = importlib.util.spec_from_file_location(mod_name, str(path))
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot create import spec for {script_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[mod_name] = module
    try:
        try:
            spec.loader.exec_module(module)
        except SystemExit:
            return getattr(module, "__mcp_result__", None)

        if not hasattr(module, "main"):
            raise AttributeError(
                f"Skill script {script_path!r} does not expose a `main` callable",
            )
        try:
            return module.main(**dict(params))
        except SystemExit:
            return getattr(module, "__mcp_result__", None)
    finally:
        sys.modules.pop(mod_name, None)


def build_inprocess_executor(
    dispatcher: BaseDccCallableDispatcher | None,
    *,
    runner: Callable[[str, Mapping[str, Any]], Any] = run_skill_script,
) -> Callable[..., Any]:
    """Return an executor callable suitable for ``set_in_process_executor``.

    When *dispatcher* is ``None`` (e.g. ``mayapy``, Houdini batch,
    pytest), the executor calls *runner* on the current thread — the
    standalone fallback Maya already implements.

    When *dispatcher* satisfies :class:`BaseDccCallableDispatcher`,
    every script invocation is routed through
    ``dispatcher.dispatch_callable(runner, script_path, params)`` so
    the script executes on the host's UI / main thread regardless of
    which thread MCP request handling lives on.

    Args:
        dispatcher: The host dispatcher, or ``None`` for inline
            execution.
        runner: Override the inner script runner (defaults to
            :func:`run_skill_script`). Mostly useful for tests.

    Returns:
        A callable accepting ``(script_path, params, *, action_name,
        skill_name, thread_affinity, execution, timeout_hint_secs)``. Older
        two-argument callers remain supported because all metadata is optional.

    """
    return HostExecutionBridge(dispatcher=dispatcher, runner=runner).as_inprocess_executor()
