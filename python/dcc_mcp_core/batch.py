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

from copy import deepcopy
import hashlib
import json
import logging
import types
from typing import Any
import uuid

from dcc_mcp_core import json_dumps

logger = logging.getLogger(__name__)

__all__ = [
    "EvalContext",
    "batch_dispatch",
    "generate_batch_id",
]


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
    idempotency_store: Any | None = None,
    idempotency_namespace: str = "batch",
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
        idempotency_store: Optional durable store exposing
            ``get(key) -> result | None`` and ``put(key, result)``. Successful
            calls are reused on later invocations, so a failed batch resumes
            at its first incomplete call.
        idempotency_namespace: Stable caller-owned namespace for cache keys.

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
        cache_key = _batch_idempotency_key(idempotency_namespace, idx, tool_name, arguments)
        cached = idempotency_store.get(cache_key) if idempotency_store is not None else None
        if isinstance(cached, dict) and _batch_result_succeeded(cached):
            result = deepcopy(cached)
            result["_batch"] = {
                "parent_request_id": parent_request_id,
                "batch_id": _batch_id,
                "sub_request_id": sub_request_id,
                "tool_name": tool_name,
                "index": idx,
                "reused": True,
            }
            results.append(result)
            succeeded += 1
            sub_results.append(
                {
                    "request_id": sub_request_id,
                    "parent_request_id": parent_request_id,
                    "batch_id": _batch_id,
                    "tool_name": tool_name,
                    "index": idx,
                    "success": True,
                    "reused": True,
                }
            )
            continue
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
                if idempotency_store is not None:
                    stored = deepcopy(result)
                    stored.pop("_batch", None)
                    idempotency_store.put(cache_key, stored)
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


def _batch_result_succeeded(result: dict[str, Any]) -> bool:
    """Return whether a dispatcher result is safe to reuse."""
    output = result.get("output", result)
    return not (isinstance(output, dict) and output.get("success") is False)


def _batch_idempotency_key(
    namespace: str,
    index: int,
    tool_name: str,
    arguments: dict[str, Any],
) -> str:
    """Return a canonical content key without retaining raw arguments."""
    canonical = json.dumps(
        {
            "namespace": namespace,
            "index": index,
            "tool": tool_name,
            "arguments": arguments,
        },
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    return f"batch:v1:{hashlib.sha256(canonical).hexdigest()}"


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
        parent_request_id: Optional parent request ID for batch attribution.
        batch_id: Optional batch group identifier. Auto-generated if not set.
        namespace: Optional caller-owned variables retained between runs.
            Restricted builtins are always refreshed when sandboxing is on.

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
        namespace: dict[str, Any] | None = None,
    ) -> None:
        self._dispatcher = dispatcher
        self._sandbox = sandbox
        self._timeout_secs = timeout_secs
        self._parent_request_id = parent_request_id
        self._batch_id = batch_id or generate_batch_id()
        self._call_index = 0
        self._namespace = namespace

    def _make_builtins(self) -> dict[str, Any]:
        import builtins

        safe: dict[str, Any] = {}
        for name in dir(builtins):
            if name not in self._BLOCKED_BUILTINS:
                safe[name] = getattr(builtins, name)
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
        ns = self._namespace if self._namespace is not None else {}
        ns["dispatch"] = self._dispatch_fn
        ns["json"] = json

        if self._sandbox:
            ns["__builtins__"] = self._make_builtins()

        # Wrap script in a function so `return` works at the top level. When a
        # caller provides a session namespace, promote function locals to
        # globals so assignments survive the call without widening builtins.
        indented = "\n".join("    " + line for line in script.splitlines())
        wrapped = f"def __dcc_eval_fn__():\n{indented}\n"
        if self._namespace is not None:
            probe = compile(wrapped, "<dcc_eval>", "exec")
            function_code = next(
                (
                    value
                    for value in probe.co_consts
                    if isinstance(value, types.CodeType) and value.co_name == "__dcc_eval_fn__"
                ),
                None,
            )
            reserved = {"dispatch", "json", "__builtins__", "__dcc_eval_fn__"}
            persistent_names = sorted(
                name
                for name in (function_code.co_varnames if function_code is not None else ())
                if name not in reserved
            )
            if persistent_names:
                declaration = "    global " + ", ".join(persistent_names) + "\n"
                wrapped = f"def __dcc_eval_fn__():\n{declaration}{indented}\n"

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
                return ns["__dcc_eval_fn__"]()
            finally:
                ns.pop("__dcc_eval_fn__", None)
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
