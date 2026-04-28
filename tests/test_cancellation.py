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
from dcc_mcp_core import JobHandle
from dcc_mcp_core import check_cancelled
from dcc_mcp_core import check_dcc_cancelled
from dcc_mcp_core import current_cancel_token
from dcc_mcp_core import current_job
from dcc_mcp_core import reset_cancel_token
from dcc_mcp_core import reset_current_job
from dcc_mcp_core import set_cancel_token
from dcc_mcp_core import set_current_job


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


# ── check_dcc_cancelled / JobHandle (issue #522) ───────────────────────────


class _FakeJob:
    """Minimal :class:`JobHandle` impl used by the per-job cancel tests."""

    def __init__(self) -> None:
        self.cancelled = False


def test_dcc_exports_available() -> None:
    """The four new per-job symbols must be re-exported from the top level."""
    # Import local modules
    import dcc_mcp_core

    for name in (
        "JobHandle",
        "check_dcc_cancelled",
        "current_job",
        "set_current_job",
        "reset_current_job",
    ):
        assert hasattr(dcc_mcp_core, name), name
        assert name in dcc_mcp_core.__all__


def test_check_dcc_cancelled_no_op_without_either_layer() -> None:
    """No token, no job → must not raise."""
    check_dcc_cancelled()


def test_check_dcc_cancelled_honours_mcp_token() -> None:
    """The MCP-side token short-circuits before the per-job check."""
    token = CancelToken()
    reset_token = set_cancel_token(token)
    try:
        token.cancel()
        with pytest.raises(CancelledError, match="Request cancelled"):
            check_dcc_cancelled()
    finally:
        reset_cancel_token(reset_token)


def test_check_dcc_cancelled_honours_per_job_handle() -> None:
    """A cancelled JobHandle raises even when the MCP token is clear."""
    job = _FakeJob()
    reset = set_current_job(job)
    try:
        check_dcc_cancelled()  # not yet cancelled → no-op
        job.cancelled = True
        with pytest.raises(CancelledError, match="Job cancelled by dispatcher"):
            check_dcc_cancelled()
    finally:
        reset_current_job(reset)


def test_check_dcc_cancelled_clears_per_thread() -> None:
    """Per-job handle is contextvar-scoped; threads do not inherit by default."""
    job = _FakeJob()
    job.cancelled = True
    reset = set_current_job(job)
    try:
        seen: list[bool] = []

        def worker() -> None:
            # New OS thread → fresh contextvar copy is empty → no-op.
            try:
                check_dcc_cancelled()
            except CancelledError:
                seen.append(False)
            else:
                seen.append(True)

        t = threading.Thread(target=worker)
        t.start()
        t.join()
        assert seen == [True]
    finally:
        reset_current_job(reset)


def test_jobhandle_protocol_runtime_check() -> None:
    """``isinstance(_, JobHandle)`` works because the protocol is runtime-checkable."""
    job = _FakeJob()
    assert isinstance(job, JobHandle)


def test_set_current_job_returns_token_for_reset() -> None:
    """The return value of set_current_job round-trips through reset_current_job."""
    job_a = _FakeJob()
    job_b = _FakeJob()
    reset_a = set_current_job(job_a)
    reset_b = set_current_job(job_b)
    assert current_job.get() is job_b
    reset_current_job(reset_b)
    assert current_job.get() is job_a
    reset_current_job(reset_a)
    assert current_job.get() is None


def test_check_dcc_cancelled_token_takes_priority_over_job() -> None:
    """When both layers signal cancel, the MCP-token message is reported."""
    job = _FakeJob()
    job.cancelled = True
    token = CancelToken()
    token.cancel()
    reset_t = set_cancel_token(token)
    reset_j = set_current_job(job)
    try:
        with pytest.raises(CancelledError, match="Request cancelled"):
            check_dcc_cancelled()
    finally:
        reset_current_job(reset_j)
        reset_cancel_token(reset_t)
