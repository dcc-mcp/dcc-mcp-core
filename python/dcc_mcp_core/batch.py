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

import json
import logging
from typing import Any

logger = logging.getLogger(__name__)

__all__ = [
    "EvalContext",
    "batch_dispatch",
]


def batch_dispatch(
    dispatcher: Any,
    calls: list[tuple[str, dict[str, Any]]],
    *,
    aggregate: str = "list",
    stop_on_error: bool = False,
) -> dict[str, Any]:
    """Execute a sequence of tool calls against a local ToolDispatcher.

    Runs all calls sequentially within the same process; intermediate results
    never leave the Python runtime.  Only the final aggregated value is
    returned.

    This is the Python-layer equivalent of the planned ``tools/batch`` MCP
    endpoint (issue #406).  The Rust-level MCP endpoint will call through this
    same logic once implemented.

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

    Example::

        results = batch_dispatch(
            dispatcher,
            [
                ("get_scene_objects", {}),
                ("get_render_stats", {"layer": "beauty"}),
            ],
            aggregate="merge",
        )
        print(results["merged"])  # combined output dict

    """
    results: list[dict[str, Any]] = []
    errors: list[dict[str, Any]] = []
    succeeded = 0

    for idx, (tool_name, arguments) in enumerate(calls):
        try:
            result = dispatcher.dispatch(tool_name, json.dumps(arguments))
            results.append(result)
            output = result.get("output", result)
            if isinstance(output, dict) and output.get("success") is False:
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
            logger.warning("batch_dispatch: tool %r raised: %s", tool_name, exc)
            if stop_on_error:
                break

    summary: dict[str, Any] = {
        "total": len(calls),
        "succeeded": succeeded,
        "errors": errors,
    }

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
    *best-effort* sandbox — it does not provide OS-level isolation.  For
    untrusted user input, combine with ``SandboxPolicy`` and run inside
    a subprocess or container.

    Args:
        dispatcher: ``ToolDispatcher`` instance.
        sandbox: Restrict ``__builtins__`` to a safe subset.  Default ``True``.
        timeout_secs: Maximum wall-clock time for script execution.
            ``None`` means no limit.  Default ``30``.

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
    ) -> None:
        self._dispatcher = dispatcher
        self._sandbox = sandbox
        self._timeout_secs = timeout_secs

    def _make_builtins(self) -> dict[str, Any]:
        import builtins

        safe: dict[str, Any] = {}
        for name in dir(builtins):
            if name not in self._BLOCKED_BUILTINS:
                safe[name] = getattr(builtins, name)
        return safe

    def _dispatch_fn(self, tool_name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        """Dispatch a single tool call from within an eval script."""
        args = arguments or {}
        try:
            return self._dispatcher.dispatch(tool_name, json.dumps(args))
        except Exception as exc:
            return {"action": tool_name, "output": {"success": False, "message": str(exc)}}

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
        ns: dict[str, Any] = {
            "dispatch": self._dispatch_fn,
            "json": json,
        }

        if self._sandbox:
            ns["__builtins__"] = self._make_builtins()

        # Wrap script in a function so `return` works at the top level.
        indented = "\n".join("    " + line for line in script.splitlines())
        wrapped = f"def __dcc_eval_fn__():\n{indented}\n"

        try:
            if self._timeout_secs is not None:
                import signal as _signal

                def _timeout_handler(signum: int, frame: Any) -> None:
                    raise TimeoutError(f"EvalContext script exceeded {self._timeout_secs}s timeout")

                old_handler = None
                try:
                    old_handler = _signal.signal(_signal.SIGALRM, _timeout_handler)  # type: ignore[attr-defined]
                    _signal.alarm(self._timeout_secs)  # type: ignore[attr-defined]
                except AttributeError:
                    pass  # SIGALRM not available on Windows; skip

            try:
                exec(wrapped, ns)
                result = ns["__dcc_eval_fn__"]()
                return result
            finally:
                if self._timeout_secs is not None:
                    try:
                        import signal as _signal2

                        _signal2.alarm(0)  # type: ignore[attr-defined]
                        if old_handler is not None:
                            _signal2.signal(_signal2.SIGALRM, old_handler)  # type: ignore[attr-defined]
                    except AttributeError:
                        pass
        except TimeoutError:
            raise
        except Exception as exc:
            raise RuntimeError(f"EvalContext script failed: {exc}") from exc
