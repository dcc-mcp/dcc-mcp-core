"""Cooperative cancellation support for DCC-MCP skill scripts.

Skill scripts executed inside a ``tools/call`` request run as regular
Python code and therefore cannot be interrupted by the dispatcher the
way an ``asyncio`` task can.  The MCP spec's ``notifications/cancelled``
message only helps if the running code checks for cancellation at
appropriate points.

This module exposes a tiny, dependency-free API that skill authors can
sprinkle inside long-running loops:

.. code-block:: python

    from dcc_mcp_core import check_cancelled, skill_success

    def run(iterations: int = 100) -> dict:
        for _ in range(iterations):
            check_cancelled()  # raises CancelledError when the caller cancels
            do_one_unit_of_work()
        return skill_success("done")

The dispatcher is expected to install a :class:`CancelToken` via
:func:`set_cancel_token` before invoking the skill and to
:func:`reset_cancel_token` in a ``finally`` block.  When no token is
installed, :func:`check_cancelled` is a no-op, which keeps the helper
safe to call from an interactive REPL or from unit tests.

The state is stored in a :mod:`contextvars` ``ContextVar`` so that
concurrent requests (e.g. under an asyncio dispatcher or in worker
threads with their own ``contextvars.Context``) do not leak cancel
flags into one another.

Dispatcher integration inside the Rust ``ToolDispatcher`` / async
``JobManager`` is deferred to issues #318 and #332; this module only
provides the Python surface so skill authors can start writing
cancellation-aware scripts today.
"""

from __future__ import annotations

# Import built-in modules
import contextvars
import threading

__all__ = [
    "CancelToken",
    "CancelledError",
    "check_cancelled",
    "current_cancel_token",
    "reset_cancel_token",
    "set_cancel_token",
]


class CancelledError(Exception):
    """Raised by :func:`check_cancelled` when the active request was cancelled.

    This is deliberately a plain :class:`Exception` subclass (not
    :class:`concurrent.futures.CancelledError` or
    :class:`asyncio.CancelledError`) because skill scripts may run in
    synchronous contexts that do not import either module.  The
    ``@skill_entry`` decorator's generic ``except Exception`` branch will
    convert an unhandled :class:`CancelledError` into a standard skill
    error dict, so most authors will never need to catch it directly.
    """


class CancelToken:
    """Thread-safe cancellation flag settable by the request dispatcher.

    Instances are cheap; a new token should be created for every request.
    :meth:`cancel` may be called from any thread — typically the HTTP
    listener thread that receives ``notifications/cancelled`` — while
    the request is still executing on a worker thread.  The underlying
    flag is guarded by a :class:`threading.Lock` so concurrent
    ``cancel()`` / ``cancelled`` reads are well-defined.

    Example:
        >>> token = CancelToken()
        >>> token.cancelled
        False
        >>> token.cancel()
        >>> token.cancelled
        True

    """

    __slots__ = ("_cancelled", "_lock")

    def __init__(self) -> None:
        self._cancelled: bool = False
        self._lock = threading.Lock()

    def cancel(self) -> None:
        """Mark the token as cancelled.

        Idempotent — calling :meth:`cancel` multiple times has no
        additional effect.
        """
        with self._lock:
            self._cancelled = True

    @property
    def cancelled(self) -> bool:
        """Whether :meth:`cancel` has been invoked on this token."""
        with self._lock:
            return self._cancelled

    def __bool__(self) -> bool:  # pragma: no cover - trivial
        return self.cancelled

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"CancelToken(cancelled={self.cancelled})"


_current_token: contextvars.ContextVar[CancelToken | None] = contextvars.ContextVar(
    "dcc_mcp_core_cancel_token",
    default=None,
)


def check_cancelled() -> None:
    """Raise :class:`CancelledError` if the active request has been cancelled.

    This is a no-op when invoked outside of a request context (for
    example from an interactive REPL, a unit test, or a DCC host that
    does not install a cancel token).  Skill authors can therefore
    sprinkle ``check_cancelled()`` calls inside loops without making
    the script harder to test in isolation.

    Raises:
        CancelledError: If a :class:`CancelToken` is installed in the
            current context and its :attr:`CancelToken.cancelled`
            property is ``True``.

    """
    token = _current_token.get()
    if token is not None and token.cancelled:
        raise CancelledError("Request cancelled by client")


def current_cancel_token() -> CancelToken | None:
    """Return the :class:`CancelToken` installed in the current context.

    Returns:
        The active token, or ``None`` when no dispatcher has installed
        one.  Useful for advanced callers that want to poll the flag
        without raising (e.g. to flush partial progress before
        returning).

    """
    return _current_token.get()


def set_cancel_token(token: CancelToken | None) -> contextvars.Token:
    """Install *token* as the active cancel token for the current context.

    This function is intended for **dispatcher** use only — skill
    authors should call :func:`check_cancelled` instead.  The return
    value must be passed to :func:`reset_cancel_token` (typically in a
    ``finally`` block) so the contextvar is restored to its previous
    value.

    Args:
        token: The token to install, or ``None`` to explicitly clear
            any inherited token for this context.

    Returns:
        A :class:`contextvars.Token` that records the previous value;
        pass it to :func:`reset_cancel_token`.

    Example:
        >>> token = CancelToken()
        >>> reset = set_cancel_token(token)
        >>> try:
        ...     run_skill()
        ... finally:
        ...     reset_cancel_token(reset)

    """
    return _current_token.set(token)


def reset_cancel_token(reset: contextvars.Token) -> None:
    """Restore the cancel-token contextvar to its previous value.

    Args:
        reset: The token returned by the matching
            :func:`set_cancel_token` call.

    """
    _current_token.reset(reset)
