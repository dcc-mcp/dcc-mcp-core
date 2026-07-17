"""In-process Python skill execution for embedded DCC adapters (issue #521).

Every embedded DCC plugin follows the same flow:

1. Run the skill script in the live DCC interpreter (no subprocess).
2. Route the script through a host dispatcher so it executes on the
   UI thread.
3. Honour the ``main(**params)`` calling convention with the
   ``SystemExit + __mcp_result__`` fallback used by skill authors.
4. Return a JSON-serialisable :class:`ToolResult`-shaped dict.

MCP wiring remains in :meth:`McpHttpServer.set_in_process_executor`;
this module supplies its executor closure and dispatcher protocol.
"""

# Import built-in modules
from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import importlib.machinery
import importlib.util
import inspect
import json
import logging
import os
from pathlib import Path
import sys
import threading
import time
from typing import Any
from typing import Callable
from typing import Mapping
from typing import Sequence
import uuid

from dcc_mcp_core._server._inprocess_contracts import BaseDccCallableDispatcher
from dcc_mcp_core._server._inprocess_contracts import DeferredToolResult
from dcc_mcp_core._server._inprocess_contracts import InProcessExecutionContext
from dcc_mcp_core._server._inprocess_contracts import attach_deferred_streams as _attach_deferred_streams
from dcc_mcp_core._server._inprocess_contracts import context_from_kwargs as _context_from_kwargs
from dcc_mcp_core._server._inprocess_contracts import exception_to_error_envelope
from dcc_mcp_core._server._inprocess_contracts import is_host_queue_dispatcher as _is_host_queue_dispatcher
from dcc_mcp_core._server._inprocess_contracts import resolve_sandbox_action_name as _resolve_sandbox_action_name
from dcc_mcp_core._server._inprocess_contracts import sandbox_denied_envelope
from dcc_mcp_core.script_execution import FileBackedScriptExecutionParams
from dcc_mcp_core.script_execution import normalize_file_backed_script_execution_params

logger = logging.getLogger(__name__)

_MAX_TIMEOUT_MS = 3_600_000
_SCRIPT_PACKAGE_LOCK = threading.RLock()
_SCRIPT_PACKAGE_CONDITION = threading.Condition(_SCRIPT_PACKAGE_LOCK)
_SCRIPT_PACKAGE_PREFIX = "_dcc_mcp_skill_"
_SCRIPT_PACKAGE_ACTIVE_CALLS: dict[str, int] = {}
_SCRIPT_PACKAGE_STOP_REQUESTS: dict[str, int] = {}
_SCRIPT_PACKAGE_CLEARING: set[str] = set()
_SCRIPT_PACKAGE_DEFERRED_CLEAR: set[str] = set()
_SCRIPT_PATH_REFS: dict[str, tuple[int, bool]] = {}
# ponytail: one shared deadline keeps server shutdown bounded across all owned
# packages; add a per-host setting only if real DCCs need different ceilings.
_SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS = 1.0

_SCRIPT_PACKAGE_LOCK = threading.RLock()
_SCRIPT_PACKAGE_CONDITION = threading.Condition(_SCRIPT_PACKAGE_LOCK)
_SCRIPT_PACKAGE_PREFIX = "_dcc_mcp_skill_"
_SCRIPT_PACKAGE_ACTIVE_CALLS: dict[str, int] = {}
_SCRIPT_PACKAGE_STOP_REQUESTS: dict[str, int] = {}
_SCRIPT_PACKAGE_CLEARING: set[str] = set()
_SCRIPT_PACKAGE_DEFERRED_CLEAR: set[str] = set()
_SCRIPT_PATH_REFS: dict[str, tuple[int, bool]] = {}
# ponytail: one shared deadline keeps server shutdown bounded across all owned
# packages; add a per-host setting only if real DCCs need different ceilings.
_SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS = 1.0


def timeout_hint_secs_to_ms(
    timeout_hint_secs: int | None,
    *,
    action_name: str = "",
    skill_name: str | None = None,
    thread_affinity: str = "main",
    execution: str = "sync",
    warn_if_missing: bool = True,
) -> int | None:
    """Convert a tools.yaml ``timeout_hint_secs`` value to dispatcher ``timeout_ms``.

    Returns ``None`` when the hint is absent so the host dispatcher keeps its
    own default. Logs a structured warning for async main-affinity actions
    that omit the hint (issue #999).
    """
    if timeout_hint_secs is None:
        if (
            warn_if_missing
            and (thread_affinity or "any").lower() == "main"
            and (execution or "sync").lower() == "async"
        ):
            logger.warning(
                "timeout_hint_secs missing for async main-affinity action; dispatcher will use its default ceiling",
                extra={
                    "action_name": action_name,
                    "skill_name": skill_name,
                    "thread_affinity": thread_affinity,
                    "execution": execution,
                },
            )
        return None
    if timeout_hint_secs <= 0:
        return None
    return min(int(timeout_hint_secs) * 1000, _MAX_TIMEOUT_MS)


__all__ = [
    "BaseDccCallableDispatcher",
    "DeferredToolResult",
    "HostExecutionBridge",
    "InProcessExecutionContext",
    "build_inprocess_executor",
    "clear_script_package",
    "exception_to_error_envelope",
    "run_skill_script",
    "sandbox_denied_envelope",
    "timeout_hint_secs_to_ms",
]


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
    host_dispatcher: Any | None = None
    runner: Callable[[str, Mapping[str, Any]], Any] | None = None
    sandbox_context: Any | None = None
    default_thread_affinity: str = "any"
    default_execution: str = "sync"
    default_timeout_hint_secs: int | None = None
    script_materialization_policy: str = "auto"
    script_materialization_root: str | Path | None = None
    trusted_script_roots: tuple[str | Path, ...] = ()
    _package_owner: str = field(default_factory=lambda: uuid.uuid4().hex, init=False, repr=False)
    _owned_script_dirs: set[str] = field(default_factory=set, init=False, repr=False)
    _script_dir_cleanup_generations: dict[str, int] = field(default_factory=dict, init=False, repr=False)
    _owned_script_dirs_lock: threading.RLock = field(default_factory=threading.RLock, init=False, repr=False)
    _admission: threading.RLock = field(default_factory=threading.RLock, init=False, repr=False)
    _accepting_calls: bool = field(default=True, init=False, repr=False)
    _admission_generation: int = field(default=0, init=False, repr=False)

    def close_script_admission(self) -> None:
        """Reject new bridge calls while server shutdown drains admitted work."""
        with self._admission:
            if self._accepting_calls:
                self._accepting_calls = False
                self._admission_generation += 1

    def resume_script_execution(self) -> None:
        """Reopen bridge admission for a new server start lifecycle."""
        with self._admission:
            self._accepting_calls = True

    def _begin_call(self) -> int | None:
        with self._admission:
            if not self._accepting_calls:
                return None
            return self._admission_generation

    def _current_generation(self) -> int | None:
        with self._admission:
            if not self._accepting_calls:
                return None
            return self._admission_generation

    def _is_current_generation(self, generation: int) -> bool:
        with self._admission:
            return self._accepting_calls and self._admission_generation == generation

    @staticmethod
    def _shutdown_error() -> dict[str, Any]:
        return exception_to_error_envelope(
            RuntimeError("host execution bridge is shutting down"),
            message="Host execution bridge is shutting down; call rejected.",
        )

    @staticmethod
    def _script_cleared_error() -> dict[str, Any]:
        return exception_to_error_envelope(
            RuntimeError("script package was cleared while the call was queued"),
            message="Script package was cleared while the call was queued; call rejected.",
        )

    def resolve_host_dispatcher(self) -> Any | None:
        """Return the dispatcher that should back HTTP main-thread routing.

        ``host_dispatcher`` lets adapters pass a Rust-backed
        ``QueueDispatcher`` / ``BlockingDispatcher`` alongside a custom
        Python ``dispatch_callable`` implementation. When ``dispatcher`` is
        already one of those queue dispatchers, the bridge reuses it for both
        in-process skill execution and HTTP ``tools/call`` routing.
        """
        if _is_host_queue_dispatcher(self.host_dispatcher):
            return self.host_dispatcher
        if _is_host_queue_dispatcher(self.dispatcher):
            return self.dispatcher
        attach_http_dispatcher = getattr(self.dispatcher, "attach_http_dispatcher", None)
        if callable(attach_http_dispatcher):
            try:
                from dcc_mcp_core._core import QueueDispatcher
            except ImportError:
                return None
            native_dispatcher = QueueDispatcher()
            attach_http_dispatcher(native_dispatcher)
            self.host_dispatcher = native_dispatcher
            return native_dispatcher
        return None

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

    def prepare_script_execution_params(
        self,
        params: Mapping[str, Any],
        *,
        dcc_type: str,
        instance_id: str,
        session_id: str,
        policy: str | None = None,
        trusted_roots: Sequence[str | Path] = (),
        materialization_root: str | Path | None = None,
        language: str = "python",
        suffix: str = ".py",
        ttl_secs: int | None = None,
        tool_call_id: str | None = None,
        correlation_id: str | None = None,
        reuse: bool = False,
        reuse_key: str | None = None,
    ) -> FileBackedScriptExecutionParams:
        """Normalize inline/file script params through the shared materializer."""
        roots = (*self.trusted_script_roots, *trusted_roots)
        root = materialization_root if materialization_root is not None else self.script_materialization_root
        return normalize_file_backed_script_execution_params(
            params,
            dcc_type=dcc_type,
            instance_id=instance_id,
            session_id=session_id,
            policy=policy or self.script_materialization_policy,
            trusted_roots=roots,
            materialization_root=root,
            language=language,
            suffix=suffix,
            ttl_secs=ttl_secs,
            tool_call_id=tool_call_id,
            correlation_id=correlation_id,
            reuse=reuse,
            reuse_key=reuse_key,
        )

    def _dispatch_raw(
        self,
        func: Callable[..., Any],
        args: tuple[Any, ...],
        kwargs: Mapping[str, Any],
        context: InProcessExecutionContext,
    ) -> Any:
        """Dispatch a callable without resolving DeferredToolResult values."""
        generation = self._begin_call()
        if generation is None:
            return self._shutdown_error()

        def _invoke(*_args: Any, **_kwargs: Any) -> Any:
            if not self._is_current_generation(generation):
                return self._shutdown_error()
            return func(*args, **kwargs)

        try:
            if self.dispatcher is None:
                return _invoke()
            is_host_thread = getattr(self.dispatcher, "is_host_thread", None)
            if callable(is_host_thread) and is_host_thread():
                return _invoke()
            dispatch_callable = getattr(self.dispatcher, "dispatch_callable", None)
            if callable(dispatch_callable):
                return dispatch_callable(
                    _invoke,
                    affinity=context.thread_affinity,
                    context=context,
                    action_name=context.action_name,
                    skill_name=context.skill_name,
                    execution=context.execution,
                    timeout_hint_secs=context.timeout_hint_secs,
                )
            if _is_host_queue_dispatcher(self.dispatcher):
                return self._dispatch_via_host_queue(self.dispatcher, _invoke, context)
            raise TypeError(
                "HostExecutionBridge dispatcher must expose dispatch_callable(...) "
                "or the QueueDispatcher/BlockingDispatcher post/tick API"
            )
        except Exception as exc:
            logger.exception("Host callable %s failed", getattr(func, "__name__", repr(func)))
            return exception_to_error_envelope(exc)

    def _dispatch_via_host_queue(
        self,
        dispatcher: Any,
        func: Callable[[], Any],
        context: InProcessExecutionContext,
    ) -> Any:
        """Run ``func`` through a Rust-backed host queue dispatcher."""
        handle = dispatcher.post(func)
        timeout_ms = timeout_hint_secs_to_ms(
            context.timeout_hint_secs,
            action_name=context.action_name,
            skill_name=context.skill_name,
            thread_affinity=context.thread_affinity,
            execution=context.execution,
            warn_if_missing=False,
        )
        timeout_secs = None if timeout_ms is None else timeout_ms / 1000.0
        return handle.wait(timeout=timeout_secs)

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
        generation = self._current_generation()
        if generation is None:
            return self._shutdown_error()
        script_admission = self._remember_script_dir(script_path)
        resolved_action = _resolve_sandbox_action_name(action_name, script_path)
        if self.sandbox_context is not None:
            return self._execute_script_sandboxed(
                script_path,
                params,
                action_name=resolved_action,
                skill_name=skill_name,
                thread_affinity=thread_affinity,
                execution=execution,
                timeout_hint_secs=timeout_hint_secs,
                admission_generation=generation,
                script_admission=script_admission,
            )
        return self.dispatch_callable(
            self._script_runner(generation, script_admission),
            script_path,
            params,
            action_name=action_name,
            skill_name=skill_name,
            thread_affinity=thread_affinity,
            execution=execution,
            timeout_hint_secs=timeout_hint_secs,
        )

    def _script_runner(
        self,
        generation: int,
        script_admission: tuple[str, int] | None,
    ) -> Callable[[str, Mapping[str, Any]], Any]:
        if self.runner is not None and self.runner is not run_skill_script:
            runner = self.runner

            def _run_custom(script_path: str, params: Mapping[str, Any]) -> Any:
                if not self._is_current_generation(generation):
                    return self._shutdown_error()
                if not self._is_current_script_generation(script_admission):
                    return self._script_cleared_error()
                return runner(script_path, params)

            return _run_custom

        def _run_owned(script_path: str, params: Mapping[str, Any]) -> Any:
            return self._run_owned_script(script_path, params, generation, script_admission)

        return _run_owned

    def _remember_script_dir(self, script_path: str) -> tuple[str, int] | None:
        path = Path(script_path).resolve()
        if path.is_file():
            with self._owned_script_dirs_lock:
                location = os.path.normcase(str(path.parent))
                self._owned_script_dirs.add(location)
                return location, self._script_dir_cleanup_generations.get(location, 0)
        return None

    def _is_current_script_generation(self, admission: tuple[str, int] | None) -> bool:
        if admission is None:
            return True
        location, generation = admission
        with self._owned_script_dirs_lock:
            return self._script_dir_cleanup_generations.get(location, 0) == generation

    def _run_owned_script(
        self,
        script_path: str,
        params: Mapping[str, Any],
        generation: int,
        script_admission: tuple[str, int] | None,
    ) -> Any:
        try:
            return run_skill_script(
                script_path,
                params,
                package_owner=self._package_owner,
                admission_check=lambda: (
                    self._is_current_generation(generation) and self._is_current_script_generation(script_admission)
                ),
            )
        except RuntimeError:
            if not self._is_current_generation(generation):
                return self._shutdown_error()
            if not self._is_current_script_generation(script_admission):
                return self._script_cleared_error()
            raise

    def request_stop_script_packages(self) -> int:
        """Signal cooperative cancellation without waiting for package unload."""
        with self._owned_script_dirs_lock:
            locations = list(self._owned_script_dirs)
        return sum(_request_script_package_stop(location, package_owner=self._package_owner) for location in locations)

    def shutdown_script_execution(self) -> int:
        """Invalidate queued calls, request stop, then unload owned packages."""
        self.close_script_admission()
        return self.clear_script_packages()

    def clear_script_package(
        self,
        script_dir: str | Path,
        *,
        timeout_secs: float | None = None,
    ) -> int:
        """Clean one private package previously executed by this bridge."""
        location = os.path.normcase(str(Path(script_dir).resolve()))
        with self._owned_script_dirs_lock:
            if location not in self._owned_script_dirs:
                return 0
            self._script_dir_cleanup_generations[location] = self._script_dir_cleanup_generations.get(location, 0) + 1
        return clear_script_package(
            location,
            package_owner=self._package_owner,
            timeout_secs=timeout_secs,
        )

    def _clear_script_locations(self, locations: Sequence[str]) -> int:
        deadline = time.monotonic() + _SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS
        cleared = 0
        for location in locations:
            remaining = max(0.0, deadline - time.monotonic())
            cleared += self.clear_script_package(location, timeout_secs=remaining)
        return cleared

    def clear_script_packages_under(self, skill_dir: str | Path) -> int:
        """Clean owned script packages located inside one skill directory."""
        root = Path(skill_dir).resolve()
        with self._owned_script_dirs_lock:
            locations = [
                location
                for location in self._owned_script_dirs
                if Path(location) == root or root in Path(location).parents
            ]
        return self._clear_script_locations(locations)

    def clear_script_packages(self) -> int:
        """Clean every private script package owned by this bridge."""
        with self._owned_script_dirs_lock:
            locations = list(self._owned_script_dirs)
        return self._clear_script_locations(locations)

    def _execute_script_sandboxed(
        self,
        script_path: str,
        params: Mapping[str, Any],
        *,
        action_name: str,
        skill_name: str | None,
        thread_affinity: str | None,
        execution: str | None,
        timeout_hint_secs: int | None,
        admission_generation: int,
        script_admission: tuple[str, int] | None,
    ) -> Any:
        """Run a skill script inside :class:`SandboxContext` when configured."""
        context = self.execution_context(
            action_name=action_name,
            skill_name=skill_name,
            thread_affinity=thread_affinity,
            execution=execution,
            timeout_hint_secs=timeout_hint_secs,
        )
        params_json = json.dumps(dict(params))

        def _sandbox_handler(_params: Mapping[str, Any]) -> Any:
            return self._dispatch_raw(
                self._script_runner(admission_generation, script_admission),
                (script_path, _params),
                {},
                context,
            )

        try:
            result = self.sandbox_context.execute_with_handler(
                action_name,
                params_json,
                _sandbox_handler,
            )
        except RuntimeError as exc:
            return sandbox_denied_envelope(exc, action_name=action_name)
        return self._resolve_deferred_result(result, context)

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


def _call_script_main(main: Callable[..., Any], params: Mapping[str, Any]) -> Any:
    """Invoke a skill ``main`` using the best supported calling convention."""
    params_dict = {key: value for key, value in params.items() if not key.startswith("_")}
    try:
        signature = inspect.signature(main)
    except (TypeError, ValueError):
        return main(**params_dict)

    parameters = list(signature.parameters.values())
    if any(param.kind is inspect.Parameter.VAR_KEYWORD for param in parameters):
        return main(**params_dict)

    keyword_names = {
        param.name
        for param in parameters
        if param.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY)
    }
    if set(params_dict).issubset(keyword_names):
        return main(**params_dict)

    positional = [
        param
        for param in parameters
        if param.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    ]
    required = [
        param
        for param in parameters
        if param.default is inspect.Parameter.empty
        and param.kind
        in (
            inspect.Parameter.POSITIONAL_ONLY,
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
            inspect.Parameter.KEYWORD_ONLY,
        )
    ]
    if len(positional) == 1 and len(required) <= 1:
        return main(params_dict)

    return main(**params_dict)


def _script_package_name(script_dir: Path, package_owner: str | None = None) -> str:
    location = os.path.normcase(str(script_dir.resolve()))
    package_key = f"{package_owner or 'shared'}:{location}"
    return f"{_SCRIPT_PACKAGE_PREFIX}{uuid.uuid5(uuid.NAMESPACE_URL, package_key).hex}"


def _ensure_script_package(script_dir: Path, package_owner: str | None = None) -> str:
    """Return the stable private package namespace for one script directory."""
    location = os.path.normcase(str(script_dir.resolve()))
    package_name = _script_package_name(script_dir, package_owner)
    with _SCRIPT_PACKAGE_LOCK:
        package = sys.modules.get(package_name)
        if package is None:
            spec = importlib.machinery.ModuleSpec(package_name, loader=None, is_package=True)
            spec.submodule_search_locations = [location]
            package = importlib.util.module_from_spec(spec)
            sys.modules[package_name] = package
        elif location not in tuple(getattr(package, "__path__", ())):
            raise ImportError(f"Private skill package collision for {location}")
        package.__dcc_mcp_script_dir__ = location
    return package_name


def _begin_script_call(package_name: str, script_dir: str) -> None:
    with _SCRIPT_PACKAGE_CONDITION:
        while package_name in _SCRIPT_PACKAGE_CLEARING:
            _SCRIPT_PACKAGE_CONDITION.wait()
        _SCRIPT_PACKAGE_ACTIVE_CALLS[package_name] = _SCRIPT_PACKAGE_ACTIVE_CALLS.get(package_name, 0) + 1

        refs, added = _SCRIPT_PATH_REFS.get(script_dir, (0, False))
        if refs == 0:
            added = script_dir not in sys.path
            if added:
                sys.path.insert(0, script_dir)
        _SCRIPT_PATH_REFS[script_dir] = (refs + 1, added)


def _end_script_call(package_name: str, script_dir: str) -> None:
    finish_deferred_clear = False
    with _SCRIPT_PACKAGE_CONDITION:
        active = _SCRIPT_PACKAGE_ACTIVE_CALLS.get(package_name, 0)
        if active <= 1:
            _SCRIPT_PACKAGE_ACTIVE_CALLS.pop(package_name, None)
            if not _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name) and package_name in _SCRIPT_PACKAGE_DEFERRED_CLEAR:
                _SCRIPT_PACKAGE_DEFERRED_CLEAR.remove(package_name)
                finish_deferred_clear = True
        else:
            _SCRIPT_PACKAGE_ACTIVE_CALLS[package_name] = active - 1

        refs, added = _SCRIPT_PATH_REFS.get(script_dir, (0, False))
        if refs <= 1:
            _SCRIPT_PATH_REFS.pop(script_dir, None)
            if added and script_dir in sys.path:
                sys.path.remove(script_dir)
        else:
            _SCRIPT_PATH_REFS[script_dir] = (refs - 1, added)
        _SCRIPT_PACKAGE_CONDITION.notify_all()

    if finish_deferred_clear:
        _finish_script_package_clear(package_name)


def _stash_legacy_modules(package_name: str, script_dir: str, before: set[str]) -> None:
    """Move newly imported bare sibling modules under the private package."""
    root = Path(script_dir).resolve()
    for name in set(sys.modules).difference(before):
        if name.startswith(f"{package_name}."):
            continue
        module = sys.modules.get(name)
        module_file = getattr(module, "__file__", None)
        if not module_file:
            continue
        location = Path(module_file).resolve()
        if location != root and root not in location.parents:
            continue
        alias_key = uuid.uuid5(uuid.NAMESPACE_URL, name).hex
        sys.modules[f"{package_name}._legacy_{alias_key}"] = module
        sys.modules.pop(name, None)


def _finish_script_package_clear(package_name: str) -> int:
    """Run cleanup hooks and unload a package after its last call returns."""
    with _SCRIPT_PACKAGE_LOCK:
        module_names = [name for name in sys.modules if name == package_name or name.startswith(f"{package_name}.")]
    try:
        for name in sorted(module_names, reverse=True):
            module = sys.modules.get(name)
            cleanup = getattr(module, "cleanup", None)
            if callable(cleanup):
                try:
                    cleanup()
                except Exception:
                    logger.warning("Skill package cleanup failed for %s", name, exc_info=True)
    finally:
        with _SCRIPT_PACKAGE_CONDITION:
            for name in module_names:
                sys.modules.pop(name, None)
            importlib.invalidate_caches()
            _SCRIPT_PACKAGE_CLEARING.discard(package_name)
            _SCRIPT_PACKAGE_DEFERRED_CLEAR.discard(package_name)
            _SCRIPT_PACKAGE_STOP_REQUESTS.pop(package_name, None)
            _SCRIPT_PACKAGE_CONDITION.notify_all()
    return len(module_names)


def clear_script_package(
    script_dir: str | Path,
    *,
    package_owner: str | None = None,
    timeout_secs: float | None = None,
) -> int:
    """Request stop and unload a private package within a bounded wait."""
    package_name = _script_package_name(Path(script_dir), package_owner)
    timeout = _SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS if timeout_secs is None else max(0.0, timeout_secs)
    deadline = time.monotonic() + timeout
    with _SCRIPT_PACKAGE_CONDITION:
        while package_name in _SCRIPT_PACKAGE_CLEARING:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                logger.warning(
                    "Skill package unload is still pending for %s; modules remain loaded until active calls return",
                    package_name,
                )
                return 0
            _SCRIPT_PACKAGE_CONDITION.wait(remaining)
        _SCRIPT_PACKAGE_CLEARING.add(package_name)
    _request_script_package_stop(script_dir, package_owner=package_owner)
    with _SCRIPT_PACKAGE_CONDITION:
        while _SCRIPT_PACKAGE_ACTIVE_CALLS.get(package_name, 0) or _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name, 0):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                active = _SCRIPT_PACKAGE_ACTIVE_CALLS.get(package_name, 0)
                stop_requests = _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name, 0)
                _SCRIPT_PACKAGE_DEFERRED_CLEAR.add(package_name)
                logger.warning(
                    "Timed out after %.3fs waiting to unload skill package %s "
                    "(%d active call(s), %d stop request(s)); "
                    "modules remain loaded and cleanup will run after the last call returns",
                    timeout,
                    package_name,
                    active,
                    stop_requests,
                )
                return 0
            _SCRIPT_PACKAGE_CONDITION.wait(remaining)
    return _finish_script_package_clear(package_name)


def _request_script_package_stop(
    script_dir: str | Path,
    *,
    package_owner: str | None = None,
) -> int:
    package_name = _script_package_name(Path(script_dir), package_owner)
    with _SCRIPT_PACKAGE_LOCK:
        active_modules = [
            sys.modules[name] for name in sys.modules if name == package_name or name.startswith(f"{package_name}.")
        ]
        stop_hooks = []
        for module in active_modules:
            request_stop = getattr(module, "request_stop", None)
            if callable(request_stop):
                stop_hooks.append((module.__name__, request_stop))
        if stop_hooks:
            _SCRIPT_PACKAGE_STOP_REQUESTS[package_name] = _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name, 0) + 1

    if not stop_hooks:
        return len(active_modules)

    def _invoke_stop_hooks() -> None:
        finish_deferred_clear = False
        try:
            for module_name, request_stop in stop_hooks:
                try:
                    request_stop()
                except Exception:
                    logger.warning("Skill package stop request failed for %s", module_name, exc_info=True)
        finally:
            with _SCRIPT_PACKAGE_CONDITION:
                pending = _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name, 0)
                if pending <= 1:
                    _SCRIPT_PACKAGE_STOP_REQUESTS.pop(package_name, None)
                else:
                    _SCRIPT_PACKAGE_STOP_REQUESTS[package_name] = pending - 1
                if (
                    not _SCRIPT_PACKAGE_ACTIVE_CALLS.get(package_name)
                    and not _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name)
                    and package_name in _SCRIPT_PACKAGE_DEFERRED_CLEAR
                ):
                    _SCRIPT_PACKAGE_DEFERRED_CLEAR.remove(package_name)
                    finish_deferred_clear = True
                _SCRIPT_PACKAGE_CONDITION.notify_all()
            if finish_deferred_clear:
                _finish_script_package_clear(package_name)

    try:
        threading.Thread(
            target=_invoke_stop_hooks,
            name="dcc-mcp-skill-stop",
            daemon=True,
        ).start()
    except Exception:
        with _SCRIPT_PACKAGE_CONDITION:
            pending = _SCRIPT_PACKAGE_STOP_REQUESTS.get(package_name, 0)
            if pending <= 1:
                _SCRIPT_PACKAGE_STOP_REQUESTS.pop(package_name, None)
            else:
                _SCRIPT_PACKAGE_STOP_REQUESTS[package_name] = pending - 1
            _SCRIPT_PACKAGE_CONDITION.notify_all()
        logger.warning("Failed to start skill package stop request for %s", package_name, exc_info=True)
    return len(active_modules)


def run_skill_script(
    script_path: str,
    params: Mapping[str, Any],
    *,
    package_owner: str | None = None,
    admission_check: Callable[[], bool] | None = None,
) -> Any:
    """Lazy-import a skill script and call its ``main`` entry point.

    Mirrors the convention skill authors already use:

    * Each resolved script directory owns a stable private package, so
      relative helper imports share state without colliding with other skills.
      The entry module itself uses a unique transient name for every call.
    * ``main`` is the entry point; both ``main(**params)`` and legacy
      ``main(params)`` script conventions are supported.
    * ``SystemExit`` is intercepted because some DCCs raise it from
      inside ``main`` to bail out of the host's event loop; in that
      case the script is expected to publish a result via
      ``module.__mcp_result__`` before exiting.
    """
    path = Path(script_path).resolve()
    if not path.is_file():
        raise FileNotFoundError(f"Skill script not found: {script_path}")

    package_name = _script_package_name(path.parent, package_owner)
    script_dir = str(path.parent)
    mod_name = ""
    call_started = False
    try:
        module_exited = False
        with _SCRIPT_PACKAGE_CONDITION:
            while package_name in _SCRIPT_PACKAGE_CLEARING:
                _SCRIPT_PACKAGE_CONDITION.wait()
            if admission_check is not None and not admission_check():
                raise RuntimeError("host execution bridge is shutting down")
            _ensure_script_package(path.parent, package_owner)
            mod_name = f"{package_name}._entry_{uuid.uuid4().hex}"
            spec = importlib.util.spec_from_file_location(mod_name, str(path))
            if spec is None or spec.loader is None:
                raise ImportError(f"Cannot create import spec for {script_path}")
            module = importlib.util.module_from_spec(spec)
            sys.modules[mod_name] = module
            _begin_script_call(package_name, script_dir)
            call_started = True
            before_modules = set(sys.modules)
            original_path_index = sys.path.index(script_dir)
            if original_path_index:
                sys.path.pop(original_path_index)
                sys.path.insert(0, script_dir)
            try:
                try:
                    spec.loader.exec_module(module)
                except SystemExit:
                    module_exited = True
            finally:
                _stash_legacy_modules(package_name, script_dir, before_modules)
                if original_path_index:
                    if script_dir in sys.path:
                        sys.path.remove(script_dir)
                    sys.path.insert(min(original_path_index, len(sys.path)), script_dir)

        if module_exited:
            return getattr(module, "__mcp_result__", None)

        if not hasattr(module, "main"):
            raise AttributeError(
                f"Skill script {script_path!r} does not expose a `main` callable",
            )
        try:
            return _call_script_main(module.main, params)
        except SystemExit:
            return getattr(module, "__mcp_result__", None)
    finally:
        if call_started:
            with _SCRIPT_PACKAGE_LOCK:
                sys.modules.pop(mod_name, None)
            _end_script_call(package_name, script_dir)


def build_inprocess_executor(
    dispatcher: BaseDccCallableDispatcher | None,
    *,
    runner: Callable[[str, Mapping[str, Any]], Any] = run_skill_script,
    sandbox_context: Any | None = None,
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
        sandbox_context: Optional :class:`SandboxContext` that enforces
            policy and records audit entries before running scripts.

    Returns:
        A callable accepting ``(script_path, params, *, action_name,
        skill_name, thread_affinity, execution, timeout_hint_secs)``. Older
        two-argument callers remain supported because all metadata is optional.

    """
    return HostExecutionBridge(
        dispatcher=dispatcher,
        runner=runner,
        sandbox_context=sandbox_context,
    ).as_inprocess_executor()
