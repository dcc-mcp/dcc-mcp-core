"""Tests for :mod:`dcc_mcp_core.cancellation`."""

from __future__ import annotations

# Import built-in modules
import contextvars
import threading
import time

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import CancelledError
from dcc_mcp_core import CancelToken
from dcc_mcp_core import check_cancelled
from dcc_mcp_core import current_cancel_token
from dcc_mcp_core import reset_cancel_token
from dcc_mcp_core import set_cancel_token


def test_exports_available() -> None:
    """All five public symbols must be importable from the top-level package."""
    # Import local modules
    import dcc_mcp_core

    for name in (
        "CancelToken",
        "CancelledError",
        "check_cancelled",
        "current_cancel_token",
        "reset_cancel_token",
        "set_cancel_token",
    ):
        assert hasattr(dcc_mcp_core, name), name
        assert name in dcc_mcp_core.__all__


def test_no_op_outside_context() -> None:
    """check_cancelled() must be a no-op when no token is installed."""
    assert current_cancel_token() is None
    # Should not raise.
    check_cancelled()
    check_cancelled()


def test_raises_when_cancelled() -> None:
    """Setting and cancelling a token causes check_cancelled() to raise."""
    token = CancelToken()
    assert token.cancelled is False

    reset = set_cancel_token(token)
    try:
        check_cancelled()  # not cancelled yet → no raise
        token.cancel()
        assert token.cancelled is True
        with pytest.raises(CancelledError):
            check_cancelled()
    finally:
        reset_cancel_token(reset)

    assert current_cancel_token() is None


def test_cancel_is_idempotent() -> None:
    """Calling cancel() multiple times has no additional effect."""
    token = CancelToken()
    token.cancel()
    token.cancel()
    token.cancel()
    assert token.cancelled is True


def test_set_reset_contextvar_restores_previous() -> None:
    """reset_cancel_token() must restore the prior ContextVar value."""
    outer = CancelToken()
    inner = CancelToken()

    outer_reset = set_cancel_token(outer)
    try:
        assert current_cancel_token() is outer
        inner_reset = set_cancel_token(inner)
        try:
            assert current_cancel_token() is inner
        finally:
            reset_cancel_token(inner_reset)
        assert current_cancel_token() is outer
    finally:
        reset_cancel_token(outer_reset)
    assert current_cancel_token() is None


def test_two_concurrent_contexts_do_not_leak() -> None:
    """Separate contextvars.Context instances must hold independent tokens."""
    token_a = CancelToken()
    token_b = CancelToken()

    ctx_a = contextvars.copy_context()
    ctx_b = contextvars.copy_context()

    def _install(tok: CancelToken) -> CancelToken | None:
        set_cancel_token(tok)
        return current_cancel_token()

    assert ctx_a.run(_install, token_a) is token_a
    assert ctx_b.run(_install, token_b) is token_b

    # Cancelling token_a must only affect ctx_a.
    token_a.cancel()

    def _check_raises() -> bool:
        try:
            check_cancelled()
            return False
        except CancelledError:
            return True

    assert ctx_a.run(_check_raises) is True
    assert ctx_b.run(_check_raises) is False

    # The outer/test context never installed a token.
    assert current_cancel_token() is None


def test_thread_cross_signalling() -> None:
    """A token cancelled from one thread is observed by the worker thread.

    contextvars.Context is per-thread by default, so the worker thread
    copies the caller's context to inherit the installed token.  The
    underlying CancelToken flag is lock-protected, so the write from
    the main thread is visible to the worker.
    """
    token = CancelToken()
    observed: list[bool] = []

    def _worker(ctx: contextvars.Context) -> None:
        def _run() -> None:
            # Wait briefly so the main thread has time to cancel.
            for _ in range(50):
                if current_cancel_token() is not None and current_cancel_token().cancelled:
                    observed.append(True)
                    return
                time.sleep(0.01)
            observed.append(False)

        ctx.run(_run)

    reset = set_cancel_token(token)
    try:
        ctx = contextvars.copy_context()
        t = threading.Thread(target=_worker, args=(ctx,))
        t.start()
        # Give the worker a chance to start, then cancel.
        time.sleep(0.05)
        token.cancel()
        t.join(timeout=5.0)
    finally:
        reset_cancel_token(reset)

    assert observed == [True]


def test_explicit_none_clears_inherited_token() -> None:
    """Passing None to set_cancel_token() clears an inherited token."""
    token = CancelToken()
    outer = set_cancel_token(token)
    try:
        assert current_cancel_token() is token
        inner = set_cancel_token(None)
        try:
            assert current_cancel_token() is None
            check_cancelled()  # must not raise — no token installed
        finally:
            reset_cancel_token(inner)
        assert current_cancel_token() is token
    finally:
        reset_cancel_token(outer)


def test_cancelled_error_is_exception_subclass() -> None:
    """CancelledError is a plain Exception subclass (not BaseException-only)."""
    assert issubclass(CancelledError, Exception)
    # Ensure @skill_entry's `except Exception` branch can catch it.
    try:
        raise CancelledError("x")
    except Exception as exc:
        assert isinstance(exc, CancelledError)
