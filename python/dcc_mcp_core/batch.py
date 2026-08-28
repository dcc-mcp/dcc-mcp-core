"""Programmatic (batch) tool calling helpers for dcc-mcp-core.

Issue #406 — server-side batch execution to reduce round-trips and token usage.

This module provides two Python-level helpers:

1. :func:`batch_dispatch` — execute multiple tool calls sequentially using a
   local ``ToolDispatcher``, returning only the aggregated results.  Nothing
   reaches the model context until the batch completes.

2. :class:`EvalContext` — a lightweight sandbox that exposes ``dispatch()``
   to a sandboxed script string, mirroring the planned ``dcc_mcp_core__eval``
   MCP built-in tool.

These are **pure-Python** helpers that work independently of the MCP HTTP
layer.  The corresponding MCP-level ``tools/batch`` and ``dcc_mcp_core__eval``
built-in tools are planned for a future Rust release (see issue #406).

batch_dispatch now records parent_request_id and batch_id for each
sub-call so the observability system can trace the full session → request →
batch child → tool/skill chain.

Typical usage
-------------
::

    from dcc_mcp_core import ToolDispatcher, ToolRegistry
    from dcc_mcp_core.batch import batch_dispatch, EvalContext

    registry = ToolRegistry()
    # ... register tools ...
    dispatcher = ToolDispatcher(registry)

    # Batch: sequential calls, single aggregated result
    results = batch_dispatch(
        dispatcher,
        [
            ("get_scene_objects", {}),
            ("get_render_stats", {"layer": "beauty"}),
        ],
        aggregate="merge",
        parent_request_id="batch-req-42",
    )

    # Eval: script calls dispatcher, only stdout / return value comes back
    ctx = EvalContext(dispatcher, sandbox=True)
    output = ctx.run('''
result = {}
for layer in ["beauty", "specular", "diffuse"]:
    r = dispatch("get_render_stats", {"layer": layer})
    result[layer] = r.get("output", {})
return result
''')
"""

from __future__ import annotations
import __future__

import ast
import json
import logging
from typing import Any
from typing import Callable
import uuid

from dcc_mcp_core import json_dumps
from dcc_mcp_core.schema import _SCRIPT_ANNOTATION_MODULES

logger = logging.getLogger(__name__)

__all__ = [
    "EvalContext",
    "batch_dispatch",
    "generate_batch_id",
]


_REFLECTIVE_ACCESS_ERROR = "reflective dunder access is not allowed"


class _SandboxCallable:
    """Callable facade that does not expose a bound method or Python closure."""

    __slots__ = ("_callback",)

    def __init__(self, callback: Callable[..., Any]) -> None:
        object.__setattr__(self, "_callback", callback)

    def __call__(self, *args: Any, **kwargs: Any) -> Any:
        callback = object.__getattribute__(self, "_callback")
        return callback(*args, **kwargs)

    def __getattribute__(self, name: str) -> Any:
        if name.startswith("_"):
            raise AttributeError(_REFLECTIVE_ACCESS_ERROR)
        return object.__getattribute__(self, name)


class _SandboxNamespace:
    """Read-only public-name facade for sandbox helper modules."""

    __slots__ = ("_values",)

    def __init__(self, values: dict[str, Any]) -> None:
        object.__setattr__(self, "_values", dict(values))

    def __getattribute__(self, name: str) -> Any:
        if name.startswith("_"):
            raise AttributeError(_REFLECTIVE_ACCESS_ERROR)
        values = object.__getattribute__(self, "_values")
        try:
            return values[name]
        except KeyError as exc:
            raise AttributeError(name) from exc


def _subscript_string_key(node: ast.Subscript) -> str | None:
    slice_node = node.slice
    index_type = getattr(ast, "Index", None)
    if index_type is not None and isinstance(slice_node, index_type):
        slice_node = slice_node.value
    if isinstance(slice_node, ast.Constant) and isinstance(slice_node.value, str):
        return slice_node.value
    if type(slice_node).__name__ == "Str":  # pragma: no cover - Python 3.7 AST
        return slice_node.s
    return None


def _validate_sandbox_source(source: str) -> None:
    tree = ast.parse(source, mode="exec")
    for node in ast.walk(tree):
        if isinstance(node, ast.Attribute) and node.attr.startswith("_"):
            raise ValueError(_REFLECTIVE_ACCESS_ERROR)
        if isinstance(node, ast.Name) and node.id.startswith("__"):
            raise ValueError(_REFLECTIVE_ACCESS_ERROR)
        if isinstance(node, ast.Subscript):
            key = _subscript_string_key(node)
            if key is not None and key.startswith("__"):
                raise ValueError(_REFLECTIVE_ACCESS_ERROR)


def _sandbox_json() -> _SandboxNamespace:
    return _SandboxNamespace(
        {
            "dumps": _SandboxCallable(json.dumps),
            "loads": _SandboxCallable(json.loads),
        },
    )


def generate_batch_id() -> str:
    """Generate a unique batch group identifier.

    Returns:
        A short UUID string suitable for correlating all sub-calls
        from a single ``call_batch`` invocation.

    """
    return str(uuid.uuid4())


def batch_dispatch(
    dispatcher: Any,
    calls: list[tuple[str, dict[str, Any]]],
    *,
    aggregate: str = "list",
    stop_on_error: bool = False,
    parent_request_id: str | None = None,
    batch_id: str | None = None,
) -> dict[str, Any]:
    """Execute a sequence of tool calls against a local ToolDispatcher.

    Runs all calls sequentially within the same process; intermediate results
    never leave the Python runtime.  Only the final aggregated value is
    returned.

    This is the Python-layer equivalent of the planned ``tools/batch`` MCP
    endpoint (issue #406).  The Rust-level MCP endpoint will call through this
    same logic once implemented.

    Each sub-call is tagged with ``parent_request_id`` and
    ``batch_id`` in the result metadata so the observability system can
    reconstruct the full trace chain: session → request → batch child →
    tool/skill.

    Args:
        dispatcher: A ``ToolDispatcher`` instance.  Must expose
            ``.dispatch(name, json_str) -> dict``.
        calls: Ordered list of ``(tool_name, arguments_dict)`` pairs.
        aggregate: How to combine results.

            - ``"list"`` (default) — return a list of individual results.
            - ``"merge"`` — merge every ``output`` dict into a single dict
              (later keys win on collision).
            - ``"last"`` — return only the last result.

        stop_on_error: When ``True``, abort the batch on the first tool call
            that returns ``success == False`` or raises an exception.
            Default ``False`` (collect all results).
        parent_request_id: The request ID of the batch call itself.
            Each sub-call's result will include this in its metadata.
        batch_id: A unique identifier for this batch group. Auto-generated
            if not provided.

    Returns:
        A dict with keys:

        - ``"results"`` — list of individual ``dispatch`` return values
          (present for ``aggregate="list"``).
        - ``"merged"`` — single merged dict (present for ``aggregate="merge"``).
        - ``"last"`` — final result dict (present for ``aggregate="last"``).
        - ``"errors"`` — list of ``{index, tool, error}`` records for calls
          that raised or returned a failure.
        - ``"total"`` — total number of calls attempted.
        - ``"succeeded"`` — number of calls that returned success.
        - ``"batch_id"`` — the batch group identifier (always present).
        - ``"parent_request_id"`` — the parent request ID (if provided).

    Example::

        results = batch_dispatch(
            dispatcher,
            [
                ("get_scene_objects", {}),
                ("get_render_stats", {"layer": "beauty"}),
            ],
            aggregate="merge",
            parent_request_id="batch-req-42",
        )
        print(results["merged"])  # combined output dict
        print(results["batch_id"])  # unique batch group ID

    """
    _batch_id = batch_id or generate_batch_id()
    results: list[dict[str, Any]] = []
    errors: list[dict[str, Any]] = []
    succeeded = 0
    sub_results: list[dict[str, Any]] = []

    for idx, (tool_name, arguments) in enumerate(calls):
        sub_request_id = f"{_batch_id}-{idx}"
        try:
            result = dispatcher.dispatch(tool_name, json_dumps(arguments))
            # Attach batch attribution metadata
            result["_batch"] = {
                "parent_request_id": parent_request_id,
                "batch_id": _batch_id,
                "sub_request_id": sub_request_id,
                "tool_name": tool_name,
                "index": idx,
            }
            results.append(result)
            sub_results.append(
                {
                    "request_id": sub_request_id,
                    "parent_request_id": parent_request_id,
                    "batch_id": _batch_id,
                    "tool_name": tool_name,
                    "index": idx,
                    "success": True,
                }
            )
            output = result.get("output", result)
            if isinstance(output, dict) and output.get("success") is False:
                sub_results[-1]["success"] = False
                sub_results[-1]["error"] = output.get("message", "unknown")
                errors.append({"index": idx, "tool": tool_name, "error": output.get("message", "unknown")})
                if stop_on_error:
                    logger.warning("batch_dispatch: stopping at index %d (tool=%s, stop_on_error=True)", idx, tool_name)
                    break
            else:
                succeeded += 1
        except Exception as exc:
            err_info = {"index": idx, "tool": tool_name, "error": str(exc)}
            errors.append(err_info)
            results.append({"action": tool_name, "output": {"success": False, "message": str(exc)}})
            sub_results.append(
                {
                    "request_id": sub_request_id,
                    "parent_request_id": parent_request_id,
                    "batch_id": _batch_id,
                    "tool_name": tool_name,
                    "index": idx,
                    "success": False,
                    "error": str(exc),
                }
            )
            logger.warning("batch_dispatch: tool %r raised: %s", tool_name, exc)
            if stop_on_error:
                break

    summary: dict[str, Any] = {
        "total": len(calls),
        "succeeded": succeeded,
        "errors": errors,
        "batch_id": _batch_id,
        "sub_results": sub_results,
    }

    if parent_request_id:
        summary["parent_request_id"] = parent_request_id

    if aggregate == "list":
        summary["results"] = results
    elif aggregate == "merge":
        merged: dict[str, Any] = {}
        for r in results:
            output = r.get("output", r)
            if isinstance(output, dict):
                merged.update(output)
        summary["merged"] = merged
    elif aggregate == "last":
        summary["last"] = results[-1] if results else {}
    else:
        summary["results"] = results

    return summary


class EvalContext:
    """Sandboxed script execution context with access to a ToolDispatcher.

    Mirrors the planned ``dcc_mcp_core__eval`` MCP built-in tool (issue #406).
    Accepts a Python script string and executes it in a restricted namespace,
    exposing only a ``dispatch(name, args)`` function.

    Intermediate values stay in-process; only the script's ``return``
    statement (or its final expression) is surfaced to the caller.

    Security note
    -------------
    When ``sandbox=True`` (default), the script is run with a restricted
    ``__builtins__`` that removes dangerous built-ins (``open``, ``exec``,
    ``eval``, ``__import__``, ``compile``, ``getattr``, ``setattr``,
    ``delattr``, ``vars``, ``dir``, ``globals``, ``locals``).  This is a
    *best-effort* sandbox — helper modules and dispatch are exposed through
    narrow facades, and reflective private/dunder traversal fails closed, but
    this does not provide OS-level isolation.  For untrusted user input,
    combine with ``SandboxPolicy`` and run inside a subprocess or container.

    Args:
        dispatcher: ``ToolDispatcher`` instance.
        sandbox: Restrict ``__builtins__`` to a safe subset.  Default ``True``.
        timeout_secs: Maximum wall-clock time for script execution.
            ``None`` means no limit.  Default ``30``.
        parent_request_id: Optional parent request ID for batch attribution.
        batch_id: Optional batch group identifier. Auto-generated if not set.

    Example::

        ctx = EvalContext(dispatcher)
        output = ctx.run('''
    frames = []
    for i in range(1, 11):
        r = dispatch("get_frame_data", {"frame": i})
        if r.get("output", {}).get("has_keyframe"):
            frames.append(i)
    return frames
    ''')
        print(output)  # [2, 5, 8] — only keyframe numbers, nothing else

    """

    _BLOCKED_BUILTINS = frozenset(
        [
            "open",
            "exec",
            "eval",
            "__import__",
            "compile",
            "getattr",
            "setattr",
            "delattr",
            "vars",
            "dir",
            "globals",
            "locals",
        ]
    )

    def __init__(
        self,
        dispatcher: Any,
        *,
        sandbox: bool = True,
        timeout_secs: int | None = 30,
        parent_request_id: str | None = None,
        batch_id: str | None = None,
    ) -> None:
        self._dispatcher = dispatcher
        self._sandbox = sandbox
        self._timeout_secs = timeout_secs
        self._parent_request_id = parent_request_id
        self._batch_id = batch_id or generate_batch_id()
        self._call_index = 0

    def _make_builtins(self) -> dict[str, Any]:
        import builtins

        safe: dict[str, Any] = {}
        for name in dir(builtins):
            if name not in self._BLOCKED_BUILTINS:
                safe[name] = getattr(builtins, name)

        # Both execution paths postpone annotations. These are inert names for
        # the schema-supported subset, not runtime typing objects or backports.
        annotation_symbols = {
            name: name for name in ("Annotated", "Any", "Dict", "List", "Literal", "Optional", "Tuple", "Union")
        }
        typing_proxy = _SandboxNamespace(annotation_symbols)

        def _import_allowed(name: str, *args: Any, **kwargs: Any) -> Any:
            if name not in _SCRIPT_ANNOTATION_MODULES:
                raise ImportError(f"sandbox import is not allowed: {name}")
            return typing_proxy

        safe["__import__"] = _SandboxCallable(_import_allowed)
        return safe

    def _dispatch_fn(self, tool_name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        """Dispatch a single tool call from within an eval script.

        Attaches batch attribution metadata to each sub-call result.
        """
        args = arguments or {}
        sub_request_id = f"{self._batch_id}-{self._call_index}"
        self._call_index += 1
        try:
            result = self._dispatcher.dispatch(tool_name, json_dumps(args))
            result["_batch"] = {
                "parent_request_id": self._parent_request_id,
                "batch_id": self._batch_id,
                "sub_request_id": sub_request_id,
                "tool_name": tool_name,
                "index": self._call_index - 1,
            }
            return result
        except Exception as exc:
            return {
                "action": tool_name,
                "output": {"success": False, "message": str(exc)},
                "_batch": {
                    "parent_request_id": self._parent_request_id,
                    "batch_id": self._batch_id,
                    "sub_request_id": sub_request_id,
                    "tool_name": tool_name,
                    "index": self._call_index - 1,
                    "error": str(exc),
                },
            }

    def _script_namespace(self, params: dict[str, Any] | None = None) -> dict[str, Any]:
        """Build globals for trusted or sandboxed script execution."""
        if self._sandbox:
            ns: dict[str, Any] = {
                "dispatch": _SandboxCallable(self._dispatch_fn),
                "json": _sandbox_json(),
                "__builtins__": self._make_builtins(),
            }
        else:
            ns = {
                "dispatch": self._dispatch_fn,
                "json": json,
            }
        if params is not None:
            ns["__dcc_params__"] = dict(params)
        return ns

    def run_callable(self, callback: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
        """Run a trusted callable on the current thread under this deadline.

        Safe signal preemption is used only when the runtime supports it. On
        runtimes such as Windows, the callable remains synchronous so a timed
        out DCC action can never continue mutating host state in a background
        worker; an overrun is reported after the callable returns.
        """
        if self._timeout_secs is None:
            return callback(*args, **kwargs)

        import threading
        import time

        timeout_secs = self._timeout_secs
        signal_module: Any | None = None
        old_handler: Any | None = None
        timer_installed = False
        deadline_triggered = False
        if threading.current_thread() is threading.main_thread():
            try:
                import signal

                if hasattr(signal, "SIGALRM"):
                    signal_module = signal

                    def _timeout_handler(signum: int, frame: Any) -> None:
                        nonlocal deadline_triggered
                        deadline_triggered = True
                        raise TimeoutError(f"EvalContext call exceeded {timeout_secs}s timeout")

                    old_handler = signal.signal(signal.SIGALRM, _timeout_handler)
                    if hasattr(signal, "setitimer") and hasattr(signal, "ITIMER_REAL"):
                        signal.setitimer(signal.ITIMER_REAL, timeout_secs)
                    else:
                        signal.alarm(timeout_secs)
                    timer_installed = True
            except (AttributeError, OSError, ValueError):
                signal_module = None
                old_handler = None
                timer_installed = False

        started = time.monotonic()

        def _deadline_exceeded() -> bool:
            return deadline_triggered or time.monotonic() - started > timeout_secs

        try:
            try:
                result = callback(*args, **kwargs)
            except BaseException as exc:
                if _deadline_exceeded():
                    raise TimeoutError(f"EvalContext call exceeded the {timeout_secs}s timeout") from exc
                raise
            if _deadline_exceeded():
                detail = (
                    ""
                    if timer_installed
                    else "; safe preemption is unavailable on this runtime, so execution completed synchronously"
                )
                raise TimeoutError(f"EvalContext call exceeded the {timeout_secs}s timeout{detail}")
            return result
        finally:
            if timer_installed and signal_module is not None:
                if hasattr(signal_module, "setitimer") and hasattr(signal_module, "ITIMER_REAL"):
                    signal_module.setitimer(signal_module.ITIMER_REAL, 0)
                else:
                    signal_module.alarm(0)
                if old_handler is not None:
                    signal_module.signal(signal_module.SIGALRM, old_handler)

    def run(self, script: str) -> Any:
        """Execute a Python script string and return its result.

        The script may use ``dispatch(tool_name, args_dict)`` to call any
        registered tool.  Use a ``return <expr>`` statement to return a value;
        the last expression is NOT implicitly returned (unlike a REPL).

        Args:
            script: Python source to execute.  May use ``return`` at the
                top level to surface a value.

        Returns:
            Whatever the script returns, or ``None`` if there is no
            ``return`` statement.

        Raises:
            RuntimeError: If the script raises an unhandled exception.
            TimeoutError: If ``timeout_secs`` is set and the script exceeds it.

        """
        ns = self._script_namespace()

        # Wrap script in a function so `return` works at the top level.
        indented = "\n".join("    " + line for line in script.splitlines())
        wrapped = f"def __dcc_eval_fn__():\n{indented}\n"
        if self._sandbox:
            _validate_sandbox_source(wrapped)

        def _execute() -> Any:
            try:
                compiled = compile(
                    wrapped,
                    "<dcc-eval>",
                    "exec",
                    flags=__future__.annotations.compiler_flag,
                    dont_inherit=True,
                )
                exec(compiled, ns)
                return ns["__dcc_eval_fn__"]()
            except TimeoutError:
                raise
            except Exception as exc:
                raise RuntimeError(f"EvalContext script failed: {exc}") from exc

        try:
            return self.run_callable(_execute)
        except TimeoutError:
            raise

    def run_entrypoint(self, script: str, params: dict[str, Any]) -> Any:
        """Execute module source and call its ``main(**params)`` in this context.

        This deliberately shares the same dispatcher, restricted builtins, and
        synchronous deadline as :meth:`run`; structured parameters must not
        select a more trusted execution environment.
        """
        ns = self._script_namespace(params)
        if self._sandbox:
            _validate_sandbox_source(script)

        def _execute() -> Any:
            try:
                compiled = compile(
                    script,
                    "<dcc-entrypoint>",
                    "exec",
                    flags=__future__.annotations.compiler_flag,
                    dont_inherit=True,
                )
                exec(compiled, ns)
                entrypoint = ns.get("main")
                if not callable(entrypoint):
                    raise TypeError("script must define a callable main")
                return entrypoint(**ns["__dcc_params__"])
            except TimeoutError:
                raise
            except Exception as exc:
                raise RuntimeError(f"EvalContext script failed: {exc}") from exc

        return self.run_callable(_execute)
