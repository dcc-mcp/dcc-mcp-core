"""Lifecycle and thread-affinity gates for split-phase in-process results."""

from __future__ import annotations

from typing import Any
from typing import Callable

from dcc_mcp_core._server._inprocess_contracts import ContinuationOutcome
from dcc_mcp_core._server._inprocess_contracts import InProcessExecutionContext
from dcc_mcp_core._server._inprocess_contracts import exception_to_error_envelope
from dcc_mcp_core._server._inprocess_results import resolve_execution_result


def resolve_bridge_result(
    result: Any,
    context: InProcessExecutionContext,
    *,
    dispatcher: Any,
    dispatch_raw: Callable[..., Any],
    cancel_token: Any | None,
    current_generation: Callable[[], int | None],
    is_current_generation: Callable[[int], bool],
) -> Any:
    """Resolve a bridge result while enforcing continuation lifecycle gates."""
    if isinstance(result, ContinuationOutcome):
        is_host_thread = getattr(dispatcher, "is_host_thread", None)
        if callable(is_host_thread) and is_host_thread():
            return exception_to_error_envelope(
                RuntimeError("split-phase continuation cannot resolve on the host thread"),
                message="Split-phase continuation must resolve off the host thread",
            )

    generation = current_generation()
    if generation is None and isinstance(result, ContinuationOutcome):
        return exception_to_error_envelope(
            RuntimeError("host execution bridge is shutting down"),
            message="Host execution bridge is shutting down; call rejected.",
        )

    lifecycle_check = (
        (lambda: is_current_generation(generation))
        if isinstance(result, ContinuationOutcome) and generation is not None
        else None
    )
    return resolve_execution_result(
        result,
        context,
        dispatcher=dispatcher,
        dispatch_raw=dispatch_raw,
        cancel_token=cancel_token,
        lifecycle_check=lifecycle_check,
    )
