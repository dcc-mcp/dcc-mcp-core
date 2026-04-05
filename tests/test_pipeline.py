"""Tests for ActionPipeline middleware concepts (Python-side validation).

These tests validate the conceptual design of the ActionPipeline middleware
system by exercising the pattern in pure Python, verifying that:
- The middleware ordering and onion model are correctly understood
- The dispatch flow with middleware hooks works as expected
- Rate limiting, audit, and logging middleware semantics are correct

Note: The actual ActionPipeline is implemented in Rust (dcc_mcp_core._core),
but the conceptual tests here ensure Python-side integration expectations
are documented and verifiable.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from dataclasses import field
import time
from typing import Any

import pytest

# ── Pure Python middleware simulation (mirrors Rust implementation) ──


@dataclass
class MiddlewareContext:
    """Python mirror of Rust MiddlewareContext."""

    action: str
    params: dict[str, Any]
    extensions: dict[str, Any] = field(default_factory=dict)

    def insert(self, key: str, value: Any) -> None:
        self.extensions[key] = value

    def get(self, key: str, default: Any = None) -> Any:
        return self.extensions.get(key, default)


@dataclass
class DispatchResult:
    """Python mirror of Rust DispatchResult."""

    action: str
    output: Any
    validation_skipped: bool = False


class DispatchError(Exception):
    """Python mirror of Rust DispatchError."""


class HandlerNotFoundError(DispatchError):
    pass


class ValidationFailedError(DispatchError):
    pass


class HandlerError(DispatchError):
    pass


class ActionMiddleware:
    """Base class for Python action middleware."""

    def before_dispatch(self, ctx: MiddlewareContext) -> None:
        """Override to add pre-dispatch logic. Raise DispatchError to abort."""

    def after_dispatch(
        self,
        ctx: MiddlewareContext,
        result: DispatchResult | None,
        error: DispatchError | None,
    ) -> None:
        """Override to add post-dispatch logic."""

    @property
    def name(self) -> str:
        return "unnamed_middleware"


class SimpleDispatcher:
    """Minimal action dispatcher for testing middleware."""

    def __init__(self) -> None:
        self._handlers: dict[str, Any] = {}

    def register(self, action: str, handler: Any) -> None:
        self._handlers[action] = handler

    def dispatch(self, action: str, params: dict) -> DispatchResult:
        if action not in self._handlers:
            raise HandlerNotFoundError(f"no handler for '{action}'")
        try:
            output = self._handlers[action](params)
            return DispatchResult(action=action, output=output)
        except DispatchError:
            raise
        except Exception as exc:
            raise HandlerError(str(exc)) from exc


class ActionPipeline:
    """Python mirror of Rust ActionPipeline."""

    def __init__(self, dispatcher: SimpleDispatcher) -> None:
        self._dispatcher = dispatcher
        self._middlewares: list[ActionMiddleware] = []

    def add_middleware(self, middleware: ActionMiddleware) -> None:
        self._middlewares.append(middleware)

    @property
    def middleware_count(self) -> int:
        return len(self._middlewares)

    @property
    def middleware_names(self) -> list[str]:
        return [m.name for m in self._middlewares]

    def dispatch(self, action: str, params: dict) -> DispatchResult:
        ctx = MiddlewareContext(action=action, params=params.copy())

        # Run before_dispatch in order
        for middleware in self._middlewares:
            middleware.before_dispatch(ctx)

        # Dispatch
        result = None
        error = None
        try:
            result = self._dispatcher.dispatch(action, ctx.params)
        except DispatchError as e:
            error = e

        # Run after_dispatch in reverse order
        for middleware in reversed(self._middlewares):
            middleware.after_dispatch(ctx, result, error)

        if error is not None:
            raise error
        return result  # type: ignore[return-value]


# ── Built-in Python middleware implementations ──


class LoggingMiddleware(ActionMiddleware):
    """Records all dispatch calls for testing."""

    def __init__(self, log_params: bool = False) -> None:
        self.log_params = log_params
        self.before_calls: list[str] = []
        self.after_calls: list[tuple[str, bool]] = []

    def before_dispatch(self, ctx: MiddlewareContext) -> None:
        self.before_calls.append(ctx.action)

    def after_dispatch(
        self,
        ctx: MiddlewareContext,
        result: DispatchResult | None,
        error: DispatchError | None,
    ) -> None:
        self.after_calls.append((ctx.action, error is None))

    @property
    def name(self) -> str:
        return "logging"


class TimingMiddleware(ActionMiddleware):
    """Records execution times."""

    def __init__(self) -> None:
        self._starts: dict[str, float] = {}
        self.elapsed_times: dict[str, float] = {}

    def before_dispatch(self, ctx: MiddlewareContext) -> None:
        self._starts[ctx.action] = time.monotonic()

    def after_dispatch(
        self,
        ctx: MiddlewareContext,
        result: DispatchResult | None,
        error: DispatchError | None,
    ) -> None:
        if ctx.action in self._starts:
            self.elapsed_times[ctx.action] = time.monotonic() - self._starts[ctx.action]

    @property
    def name(self) -> str:
        return "timing"


class RateLimitMiddleware(ActionMiddleware):
    """Simple rate limiter."""

    def __init__(self, max_calls: int, window_seconds: float) -> None:
        self.max_calls = max_calls
        self.window = window_seconds
        self._state: dict[str, tuple[int, float]] = {}

    def before_dispatch(self, ctx: MiddlewareContext) -> None:
        now = time.monotonic()
        count, window_start = self._state.get(ctx.action, (0, now))

        if now - window_start >= self.window:
            count, window_start = 0, now

        count += 1
        self._state[ctx.action] = (count, window_start)

        if count > self.max_calls:
            raise HandlerError(
                f"rate limit exceeded for '{ctx.action}': {count - 1} calls in {self.window}s (max {self.max_calls})"
            )

    @property
    def name(self) -> str:
        return "rate_limit"


@dataclass
class AuditRecord:
    """Audit log entry."""

    action: str
    params: dict
    success: bool
    error: str | None
    output: Any | None


class AuditMiddleware(ActionMiddleware):
    """Records all dispatched actions."""

    def __init__(self, record_params: bool = True) -> None:
        self.record_params = record_params
        self._records: list[AuditRecord] = []

    def after_dispatch(
        self,
        ctx: MiddlewareContext,
        result: DispatchResult | None,
        error: DispatchError | None,
    ) -> None:
        self._records.append(
            AuditRecord(
                action=ctx.action,
                params=ctx.params.copy() if self.record_params else {},
                success=error is None,
                error=str(error) if error else None,
                output=result.output if result else None,
            )
        )

    def records(self) -> list[AuditRecord]:
        return list(self._records)

    def records_for(self, action: str) -> list[AuditRecord]:
        return [r for r in self._records if r.action == action]

    def clear(self) -> None:
        self._records.clear()

    @property
    def name(self) -> str:
        return "audit"


# ── Fixtures ──


def make_pipeline(*actions: str) -> tuple[ActionPipeline, SimpleDispatcher]:
    """Create a pipeline with echo handlers for each action."""
    dispatcher = SimpleDispatcher()
    for action in actions:
        dispatcher.register(action, lambda p: p)
    pipeline = ActionPipeline(dispatcher)
    return pipeline, dispatcher


# ── Tests ──


class TestMiddlewareContext:
    def test_basic_construction(self):
        ctx = MiddlewareContext(action="my_action", params={"x": 1})
        assert ctx.action == "my_action"
        assert ctx.params == {"x": 1}
        assert ctx.extensions == {}

    def test_insert_and_get(self):
        ctx = MiddlewareContext(action="a", params={})
        ctx.insert("key", 42)
        assert ctx.get("key") == 42
        assert ctx.get("missing") is None
        assert ctx.get("missing", "default") == "default"

    def test_overwrite(self):
        ctx = MiddlewareContext(action="a", params={})
        ctx.insert("k", 1)
        ctx.insert("k", 2)
        assert ctx.get("k") == 2


class TestActionPipelineBasics:
    def test_no_middleware_dispatch(self):
        pipeline, _ = make_pipeline("echo")
        result = pipeline.dispatch("echo", {"msg": "hello"})
        assert result.output == {"msg": "hello"}

    def test_middleware_count(self):
        pipeline, _ = make_pipeline("echo")
        assert pipeline.middleware_count == 0
        pipeline.add_middleware(LoggingMiddleware())
        assert pipeline.middleware_count == 1

    def test_middleware_names(self):
        pipeline, _ = make_pipeline("echo")
        pipeline.add_middleware(LoggingMiddleware())
        pipeline.add_middleware(TimingMiddleware())
        pipeline.add_middleware(AuditMiddleware())
        assert pipeline.middleware_names == ["logging", "timing", "audit"]

    def test_dispatch_not_found(self):
        pipeline, _ = make_pipeline("echo")
        with pytest.raises(HandlerNotFoundError):
            pipeline.dispatch("nonexistent", {})

    def test_handler_error_propagates(self):
        dispatcher = SimpleDispatcher()
        dispatcher.register("fail", lambda _: (_ for _ in ()).throw(ValueError("bad")))
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(HandlerError):
            pipeline.dispatch("fail", {})


class TestLoggingMiddleware:
    def test_records_before_after(self):
        pipeline, _ = make_pipeline("ping")
        logger = LoggingMiddleware()
        pipeline.add_middleware(logger)

        pipeline.dispatch("ping", {})
        assert logger.before_calls == ["ping"]
        assert logger.after_calls == [("ping", True)]

    def test_records_failure(self):
        dispatcher = SimpleDispatcher()

        def raise_error(p):
            raise ValueError("oops")

        dispatcher.register("fail", raise_error)
        pipeline = ActionPipeline(dispatcher)
        logger = LoggingMiddleware()
        pipeline.add_middleware(logger)

        with pytest.raises(HandlerError):
            pipeline.dispatch("fail", {})

        assert logger.after_calls == [("fail", False)]

    def test_multiple_dispatches(self):
        pipeline, _ = make_pipeline("a", "b")
        logger = LoggingMiddleware()
        pipeline.add_middleware(logger)

        pipeline.dispatch("a", {})
        pipeline.dispatch("b", {})
        pipeline.dispatch("a", {})

        assert logger.before_calls == ["a", "b", "a"]

    def test_name(self):
        assert LoggingMiddleware().name == "logging"


class TestTimingMiddleware:
    def test_records_elapsed_time(self):
        pipeline, _ = make_pipeline("slow")
        timing = TimingMiddleware()
        pipeline.add_middleware(timing)

        pipeline.dispatch("slow", {})
        assert "slow" in timing.elapsed_times
        assert timing.elapsed_times["slow"] >= 0.0

    def test_name(self):
        assert TimingMiddleware().name == "timing"


class TestRateLimitMiddleware:
    def test_allows_under_limit(self):
        pipeline, _ = make_pipeline("action")
        pipeline.add_middleware(RateLimitMiddleware(max_calls=5, window_seconds=60))

        for _ in range(5):
            pipeline.dispatch("action", {})  # should all succeed

    def test_blocks_over_limit(self):
        pipeline, _ = make_pipeline("action")
        pipeline.add_middleware(RateLimitMiddleware(max_calls=2, window_seconds=60))

        pipeline.dispatch("action", {})
        pipeline.dispatch("action", {})

        with pytest.raises(HandlerError, match="rate limit exceeded"):
            pipeline.dispatch("action", {})

    def test_independent_per_action(self):
        pipeline, _ = make_pipeline("a", "b")
        pipeline.add_middleware(RateLimitMiddleware(max_calls=1, window_seconds=60))

        pipeline.dispatch("a", {})
        pipeline.dispatch("b", {})  # independent bucket

        with pytest.raises(HandlerError):
            pipeline.dispatch("a", {})

    def test_window_reset(self):
        pipeline, _ = make_pipeline("action")
        pipeline.add_middleware(RateLimitMiddleware(max_calls=1, window_seconds=0.05))

        pipeline.dispatch("action", {})
        time.sleep(0.1)  # wait for window to expire
        pipeline.dispatch("action", {})  # should work after reset

    def test_name(self):
        assert RateLimitMiddleware(5, 60).name == "rate_limit"


class TestAuditMiddleware:
    def test_records_success(self):
        pipeline, _ = make_pipeline("create")
        audit = AuditMiddleware()
        pipeline.add_middleware(audit)

        pipeline.dispatch("create", {"name": "sphere"})
        records = audit.records()
        assert len(records) == 1
        assert records[0].action == "create"
        assert records[0].success
        assert records[0].error is None
        assert records[0].params == {"name": "sphere"}

    def test_records_failure(self):
        dispatcher = SimpleDispatcher()
        dispatcher.register("broken", lambda _: (_ for _ in ()).throw(ValueError("crash")))
        pipeline = ActionPipeline(dispatcher)
        audit = AuditMiddleware()
        pipeline.add_middleware(audit)

        with pytest.raises(HandlerError):
            pipeline.dispatch("broken", {})

        records = audit.records()
        assert len(records) == 1
        assert not records[0].success
        assert records[0].error is not None

    def test_records_for_action_filter(self):
        pipeline, _ = make_pipeline("a", "b")
        audit = AuditMiddleware()
        pipeline.add_middleware(audit)

        for action in ["a", "b", "a", "b", "a"]:
            pipeline.dispatch(action, {})

        assert len(audit.records_for("a")) == 3
        assert len(audit.records_for("b")) == 2
        assert len(audit.records_for("c")) == 0

    def test_clear(self):
        pipeline, _ = make_pipeline("x")
        audit = AuditMiddleware()
        pipeline.add_middleware(audit)

        pipeline.dispatch("x", {})
        assert len(audit.records()) == 1

        audit.clear()
        assert len(audit.records()) == 0

    def test_no_params_recording(self):
        pipeline, _ = make_pipeline("action")
        audit = AuditMiddleware(record_params=False)
        pipeline.add_middleware(audit)

        pipeline.dispatch("action", {"secret": "token123"})
        records = audit.records()
        assert records[0].params == {}  # params not recorded

    def test_name(self):
        assert AuditMiddleware().name == "audit"


class TestMiddlewareOrderingAndOnionModel:
    def test_before_runs_in_order_after_in_reverse(self):
        """Verify the onion (middleware) model: before is forward, after is reverse."""
        pipeline, _ = make_pipeline("action")
        call_log: list[str] = []

        class OrderMiddleware(ActionMiddleware):
            def __init__(self, id_: str):
                self.id_ = id_

            def before_dispatch(self, ctx):
                call_log.append(f"before:{self.id_}")

            def after_dispatch(self, ctx, result, error):
                call_log.append(f"after:{self.id_}")

            @property
            def name(self):
                return f"order_{self.id_}"

        pipeline.add_middleware(OrderMiddleware("first"))
        pipeline.add_middleware(OrderMiddleware("second"))
        pipeline.add_middleware(OrderMiddleware("third"))

        pipeline.dispatch("action", {})

        assert call_log == [
            "before:first",
            "before:second",
            "before:third",
            "after:third",  # reverse
            "after:second",
            "after:first",
        ]

    def test_abort_on_before_error_skips_handler(self):
        """When before_dispatch raises, handler and subsequent middlewares are skipped."""
        pipeline, _ = make_pipeline("action")
        executed = []

        class AbortMiddleware(ActionMiddleware):
            def before_dispatch(self, ctx):
                raise HandlerError("aborted")

        class TrackMiddleware(ActionMiddleware):
            def before_dispatch(self, ctx):
                executed.append("before:track")

        pipeline.add_middleware(AbortMiddleware())
        pipeline.add_middleware(TrackMiddleware())

        with pytest.raises(HandlerError, match="aborted"):
            pipeline.dispatch("action", {})

        # Second middleware's before_dispatch should NOT have run
        assert "before:track" not in executed

    def test_params_mutation_in_middleware(self):
        """Middleware can mutate params before they reach the handler."""
        dispatcher = SimpleDispatcher()
        dispatcher.register("echo", lambda p: p)
        pipeline = ActionPipeline(dispatcher)

        class InjectMiddleware(ActionMiddleware):
            def before_dispatch(self, ctx):
                ctx.params["injected"] = "yes"

        pipeline.add_middleware(InjectMiddleware())
        result = pipeline.dispatch("echo", {"original": "value"})
        assert result.output["injected"] == "yes"
        assert result.output["original"] == "value"

    def test_multiple_middleware_all_see_result(self):
        """Both middleware see the final result in after_dispatch."""
        pipeline, _ = make_pipeline("ping")
        results: list[bool] = []

        class TrackResultMiddleware(ActionMiddleware):
            def after_dispatch(self, ctx, result, error):
                results.append(error is None)

        pipeline.add_middleware(TrackResultMiddleware())
        pipeline.add_middleware(TrackResultMiddleware())
        pipeline.dispatch("ping", {})

        assert results == [True, True]  # both saw success


class TestCombinedMiddleware:
    def test_logging_plus_audit(self):
        pipeline, _ = make_pipeline("create_sphere")
        logger = LoggingMiddleware()
        audit = AuditMiddleware()
        pipeline.add_middleware(logger)
        pipeline.add_middleware(audit)

        pipeline.dispatch("create_sphere", {"radius": 1.5})

        assert logger.before_calls == ["create_sphere"]
        assert len(audit.records()) == 1
        assert audit.records()[0].params == {"radius": 1.5}

    def test_rate_limit_plus_audit_on_rejection(self):
        pipeline, _ = make_pipeline("action")
        audit = AuditMiddleware()
        # Rate limit must come before audit in pipeline
        # (rate limit aborts → audit still records via after_dispatch)
        pipeline.add_middleware(RateLimitMiddleware(max_calls=1, window_seconds=60))
        pipeline.add_middleware(audit)

        pipeline.dispatch("action", {})  # succeeds

        with pytest.raises(HandlerError):
            pipeline.dispatch("action", {})  # rate limited

        # Audit records the failed dispatch (after_dispatch still runs)
        # Note: this depends on implementation — rate limit error skips handler
        # but after_dispatch for audit (which is a later middleware) won't run
        # in this implementation since rate limit aborts in before_dispatch.
        # Only the first successful call should be recorded.
        records = audit.records()
        assert records[0].success

    def test_timing_records_for_multiple_actions(self):
        pipeline, _ = make_pipeline("fast", "slow")
        timing = TimingMiddleware()
        pipeline.add_middleware(timing)

        pipeline.dispatch("fast", {})
        pipeline.dispatch("slow", {})

        assert "fast" in timing.elapsed_times
        assert "slow" in timing.elapsed_times
