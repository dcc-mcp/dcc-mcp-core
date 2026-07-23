"""Resolve deferred and chunked in-process tool results."""

from __future__ import annotations

import json
import threading
import time
from typing import Any
from typing import Callable
import uuid

from dcc_mcp_core._server._inprocess_contracts import DeferredToolResult
from dcc_mcp_core._server._inprocess_contracts import InProcessExecutionContext
from dcc_mcp_core._server._inprocess_contracts import attach_deferred_streams
from dcc_mcp_core._server._inprocess_contracts import exception_to_error_envelope
from dcc_mcp_core._server._inprocess_contracts import timeout_hint_secs_to_ms
from dcc_mcp_core.chunked_runner import ChunkedRunner


def resolve_execution_result(
    result: Any,
    context: InProcessExecutionContext,
    *,
    dispatcher: Any,
    dispatch_raw: Callable[..., Any],
) -> Any:
    """Resolve a returned chunk runner or deferred result."""
    if isinstance(result, ChunkedRunner):
        return _resolve_chunked_runner(result, context, dispatcher)
    if context.job_strategy == "chunked":
        return exception_to_error_envelope(
            TypeError("job_strategy 'chunked' requires the tool to return ChunkedRunner"),
            message="Chunked tool returned a monolithic result",
        )
    if not isinstance(result, DeferredToolResult):
        return result

    deadline = time.monotonic() + result.timeout_secs
    while True:
        if time.monotonic() >= deadline:
            envelope = exception_to_error_envelope(
                TimeoutError(f"Deferred tool timed out after {result.timeout_secs:g}s"),
                message="Deferred tool did not finish before timeout",
            )
            return attach_deferred_streams(envelope, result)

        try:
            finished = dispatch_raw(result.check_is_finished, (), {}, context)
        except Exception as exc:  # pragma: no cover - dispatch_raw normalises
            finished = exception_to_error_envelope(exc)

        if finished is not None:
            if isinstance(finished, DeferredToolResult):
                envelope = exception_to_error_envelope(
                    TypeError("Nested DeferredToolResult is not supported"),
                    message="Deferred tool returned another deferred result",
                )
                return attach_deferred_streams(envelope, result)
            try:
                json.dumps(finished)
            except TypeError as exc:
                envelope = exception_to_error_envelope(
                    exc,
                    message="Deferred tool returned a non-serialisable result",
                )
                return attach_deferred_streams(envelope, result)
            return attach_deferred_streams(finished, result)

        time.sleep(result.poll_interval_secs)


def _resolve_chunked_runner(
    runner: ChunkedRunner,
    context: InProcessExecutionContext,
    dispatcher: Any,
) -> Any:
    """Submit a returned runner and wait off the host thread."""
    submit = getattr(dispatcher, "submit_chunked_runner", None)
    if not callable(submit):
        return exception_to_error_envelope(
            RuntimeError("host dispatcher does not support chunked jobs"),
            message="Chunked execution is unavailable for this adapter",
        )
    is_host_thread = getattr(dispatcher, "is_host_thread", None)
    if callable(is_host_thread) and is_host_thread():
        return exception_to_error_envelope(
            RuntimeError("cannot wait for a chunked job on the host thread"),
            message="Chunked execution must be submitted from an async tool call",
        )

    completed = threading.Event()
    terminal: dict[str, Any] = {}

    def _on_complete(outcome: dict[str, Any]) -> None:
        terminal.update(outcome)
        completed.set()

    request_id = context.job_id or f"{context.action_name or 'chunked'}:{uuid.uuid4().hex}"
    accepted = submit(
        request_id,
        runner,
        job_id=context.job_id,
        on_complete=_on_complete,
    )
    if not accepted.get("success"):
        return accepted
    timeout_ms = timeout_hint_secs_to_ms(
        context.timeout_hint_secs,
        action_name=context.action_name,
        skill_name=context.skill_name,
        thread_affinity=context.thread_affinity,
        execution=context.execution,
        warn_if_missing=True,
    )
    timeout_secs = None if timeout_ms is None else timeout_ms / 1000.0
    if not completed.wait(timeout_secs):
        runner.cancel()
        return exception_to_error_envelope(
            TimeoutError("chunked job did not finish before timeout"),
            message="Chunked job exceeded timeout_hint_secs; cancellation requested",
        )
    if terminal.get("success"):
        return terminal.get("output")
    error = terminal.get("error") or "chunked job failed"
    return exception_to_error_envelope(RuntimeError(str(error)), message=str(error))
