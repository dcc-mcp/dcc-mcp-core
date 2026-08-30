"""Resolve deferred and chunked in-process tool results."""

from __future__ import annotations

import inspect
import json
import threading
import time
from typing import Any
from typing import Callable
import uuid

from dcc_mcp_core._server._inprocess_contracts import ContinuationOutcome
from dcc_mcp_core._server._inprocess_contracts import DeferredToolResult
from dcc_mcp_core._server._inprocess_contracts import InProcessExecutionContext
from dcc_mcp_core._server._inprocess_contracts import attach_deferred_streams
from dcc_mcp_core._server._inprocess_contracts import exception_to_error_envelope
from dcc_mcp_core._server._inprocess_contracts import timeout_hint_secs_to_ms
from dcc_mcp_core.cancellation import DccMcpCancelledError
from dcc_mcp_core.cancellation import reset_cancel_token
from dcc_mcp_core.cancellation import set_cancel_token
from dcc_mcp_core.chunked_runner import ChunkedRunner


class _ContinuationProbe:
    """Cooperative cancellation/deadline probe for split-phase callbacks.

    The probe is always passed to a continuation, including synchronous
    MCP/REST calls that do not have a server cancellation token.  This keeps
    the callback contract identical across all routes and gives callbacks a
    deadline gate before durable work.
    """

    __slots__ = ("_cancel_token", "_deadline", "_lifecycle_check")

    def __init__(
        self,
        cancel_token: Any | None,
        deadline: float,
        lifecycle_check: Callable[[], bool] | None,
    ) -> None:
        self._cancel_token = cancel_token
        self._deadline = deadline
        self._lifecycle_check = lifecycle_check

    @property
    def cancelled(self) -> bool:
        token = self._cancel_token
        if token is not None and bool(getattr(token, "cancelled", False)):
            return True
        if time.monotonic() >= self._deadline:
            return True
        return self._lifecycle_check is not None and not self._lifecycle_check()

    @property
    def job_id(self) -> str | None:
        job_id = getattr(self._cancel_token, "job_id", None)
        return job_id if isinstance(job_id, str) else None

    def check(self) -> None:
        if self.cancelled:
            raise DccMcpCancelledError("Split-phase continuation cancelled or timed out")


def resolve_execution_result(
    result: Any,
    context: InProcessExecutionContext,
    *,
    dispatcher: Any,
    dispatch_raw: Callable[..., Any],
    cancel_token: Any | None = None,
    lifecycle_check: Callable[[], bool] | None = None,
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
        if isinstance(result, dict) and "_dcc_mcp_split_phase" in result:
            return exception_to_error_envelope(
                ValueError("reserved split-phase marker cannot be supplied as a plain mapping"),
                message="Malformed split-phase marker rejected",
            )
        if isinstance(result, ContinuationOutcome):
            return _resolve_continuation(
                result,
                context,
                cancel_token=cancel_token,
                lifecycle_check=lifecycle_check,
            )
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


def _resolve_continuation(
    outcome: ContinuationOutcome,
    context: InProcessExecutionContext,
    *,
    cancel_token: Any | None = None,
    lifecycle_check: Callable[[], bool] | None = None,
) -> Any:
    """Run a split-phase continuation off the host dispatch closure.

    ``dispatch_raw`` has already returned before this function is entered,
    which is the key main-affinity invariant.  The continuation is one-shot;
    nested outcomes are rejected so an adapter cannot accidentally retain the
    host lane through recursive hand-offs.
    """
    if not outcome.claim():
        return exception_to_error_envelope(
            RuntimeError("split-phase continuation already consumed"),
            message="Split-phase continuation replay rejected",
        )
    started = time.monotonic()
    try:
        # Cancellation is checked both before submit and at the commit seam.
        probe = _ContinuationProbe(
            cancel_token,
            started + outcome.timeout_secs,
            lifecycle_check,
        )
        probe.check()
        # Every continuation must expose a probe; otherwise a blocking
        # callback could perform durable work after its deadline or lifecycle
        # has been cancelled. Refuse such callbacks before they run (fail
        # closed rather than guessing at side effects).
        continuation = outcome.continuation
        accepts_probe = False
        try:
            signature = inspect.signature(continuation)
            accepts_probe = any(
                parameter.kind in (parameter.POSITIONAL_ONLY, parameter.POSITIONAL_OR_KEYWORD)
                for parameter in signature.parameters.values()
            ) or any(parameter.kind == parameter.VAR_POSITIONAL for parameter in signature.parameters.values())
        except (TypeError, ValueError):
            accepts_probe = False
        if not accepts_probe:
            return exception_to_error_envelope(
                TypeError("continuation does not accept a cancellation probe"),
                message="Uncancellable continuation rejected before commit",
            )
        reset = set_cancel_token(probe)
        try:
            finished = continuation(probe)
        finally:
            reset_cancel_token(reset)
        probe.check()
    except DccMcpCancelledError as exc:
        return exception_to_error_envelope(exc, message="Split-phase continuation cancelled before commit")
    except Exception as exc:
        return exception_to_error_envelope(exc, message="Split-phase continuation failed")

    if time.monotonic() - started > outcome.timeout_secs:
        return exception_to_error_envelope(
            TimeoutError(f"Continuation timed out after {outcome.timeout_secs:g}s"),
            message="Split-phase continuation exceeded timeout",
        )
    if isinstance(finished, (ContinuationOutcome, DeferredToolResult)):
        return exception_to_error_envelope(
            TypeError("Nested continuation outcomes are not supported"),
            message="Nested split-phase continuation rejected",
        )
    try:
        json.dumps(finished)
    except TypeError as exc:
        return exception_to_error_envelope(
            exc,
            message="Split-phase continuation returned a non-serialisable result",
        )
    return finished


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
