"""Shared contracts and result envelopes for in-process DCC execution."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import traceback
from typing import Any
from typing import Callable

from dcc_mcp_core._typing_compat import Protocol
from dcc_mcp_core._typing_compat import runtime_checkable


@dataclass(frozen=True)
class InProcessExecutionContext:
    """Execution metadata for a single in-process skill-script call."""

    action_name: str = ""
    skill_name: str | None = None
    thread_affinity: str = "any"
    execution: str = "sync"
    timeout_hint_secs: int | None = None
    job_id: str | None = None
    cancel_token: Any | None = None


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


def context_from_kwargs(
    *,
    action_name: str = "",
    skill_name: str | None = None,
    thread_affinity: str = "any",
    execution: str = "sync",
    timeout_hint_secs: int | None = None,
    job_id: str | None = None,
    cancel_token: Any | None = None,
) -> InProcessExecutionContext:
    return InProcessExecutionContext(
        action_name=action_name,
        skill_name=skill_name,
        thread_affinity=thread_affinity or "any",
        execution=execution or "sync",
        timeout_hint_secs=timeout_hint_secs,
        job_id=job_id or None,
        cancel_token=cancel_token,
    )


def sandbox_denied_envelope(exc: BaseException, *, action_name: str = "") -> dict[str, Any]:
    """Structured denial envelope when :class:`SandboxContext` rejects an action."""
    msg = str(exc)
    detail = f"Sandbox denied action '{action_name}': {msg}" if action_name else f"Sandbox denied action: {msg}"
    return {
        "success": False,
        "message": detail,
        "error": {
            "type": "SandboxDenied",
            "message": msg,
            "action": action_name or None,
        },
    }


def resolve_sandbox_action_name(action_name: str, script_path: str) -> str:
    if action_name:
        return action_name
    return Path(script_path).stem


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


def attach_deferred_streams(result: Any, deferred: DeferredToolResult) -> Any:
    """Attach initial stdout/stderr captured before deferred completion."""
    if not deferred.stdout and not deferred.stderr:
        return result

    meta = {"stdout": deferred.stdout, "stderr": deferred.stderr}
    if isinstance(result, dict):
        enriched = dict(result)
        existing_meta = enriched.get("_meta")
        merged_meta = dict(existing_meta) if isinstance(existing_meta, dict) else {}
        merged_meta["dcc.deferred"] = meta
        enriched["_meta"] = merged_meta
        return enriched

    return {"result": result, "_meta": {"dcc.deferred": meta}}


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


def is_host_queue_dispatcher(dispatcher: Any | None) -> bool:
    """Return ``True`` for the Rust-backed host dispatcher Python surface."""
    if dispatcher is None:
        return False
    return callable(getattr(dispatcher, "post", None)) and callable(getattr(dispatcher, "tick", None))


for _public_contract in (
    BaseDccCallableDispatcher,
    DeferredToolResult,
    InProcessExecutionContext,
    exception_to_error_envelope,
    sandbox_denied_envelope,
):
    _public_contract.__module__ = "dcc_mcp_core._server.inprocess_executor"
