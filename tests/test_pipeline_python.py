"""Python integration tests for ToolPipeline and middleware Python bindings.

Tests:
- ToolPipeline construction from ToolDispatcher
- LoggingMiddleware registration
- TimingMiddleware registration + last_elapsed_ms query
- AuditMiddleware registration + records / record_count / clear
- RateLimitMiddleware registration + call_count + rate limit enforcement
- add_callable custom Python middleware (before_fn / after_fn)
- register_handler on the pipeline
- dispatch happy path and error paths
- middleware_names / middleware_count / handler_count
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import LoggingMiddleware
from dcc_mcp_core import RateLimitMiddleware
from dcc_mcp_core import TimingMiddleware
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry

# ── Fixtures ──────────────────────────────────────────────────────────────────


def make_dispatcher(schema: str = "{}") -> tuple[ToolRegistry, ToolDispatcher]:
    reg = ToolRegistry()
    reg.register("ping", category="util", dcc="mock", input_schema=schema)
    dispatcher = ToolDispatcher(reg)
    dispatcher.register_handler("ping", lambda params: "pong")
    return reg, dispatcher


# ── TestActionPipeline ────────────────────────────────────────────────────────


class TestActionPipelineBasic:
    """Basic construction and dispatch without middleware."""

    def test_construction(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        assert pipeline.handler_count() == 1
        assert pipeline.middleware_count() == 0

    def test_dispatch_happy_path(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        result = pipeline.dispatch("ping", "{}")
        assert result["action"] == "ping"
        assert result["output"] == "pong"

    def test_dispatch_default_null_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        result = pipeline.dispatch("ping")
        assert result["output"] == "pong"

    def test_dispatch_unknown_action_raises_key_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        with pytest.raises(KeyError, match="no handler"):
            pipeline.dispatch("nonexistent", "{}")

    def test_dispatch_invalid_json_raises_value_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        with pytest.raises(ValueError, match="invalid JSON"):
            pipeline.dispatch("ping", "not-json")

    def test_register_handler_after_construction(self):
        reg = ToolRegistry()
        reg.register("echo", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        pipeline = ToolPipeline(dispatcher)
        pipeline.register_handler("echo", lambda params: params)
        assert pipeline.handler_count() == 1
        result = pipeline.dispatch("echo", '{"x": 1}')
        assert result["output"] == {"x": 1}

    def test_register_non_callable_raises_type_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.register_handler("ping", "not-callable")


# ── TestLoggingMiddleware ─────────────────────────────────────────────────────


class TestLoggingMiddleware:
    def test_add_logging(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging()
        assert pipeline.middleware_count() == 1
        assert "logging" in pipeline.middleware_names()

    def test_add_logging_with_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging(log_params=True)
        assert pipeline.middleware_count() == 1

    def test_dispatch_with_logging(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging()
        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"

    def test_logging_middleware_direct_construction(self):
        m = LoggingMiddleware()
        assert m.log_params is False
        m2 = LoggingMiddleware(log_params=True)
        assert m2.log_params is True
        assert "LoggingMiddleware" in repr(m)


# ── TestTimingMiddleware ──────────────────────────────────────────────────────


class TestTimingMiddleware:
    def test_add_timing_returns_instance(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        timing = pipeline.add_timing()
        assert isinstance(timing, TimingMiddleware)
        assert pipeline.middleware_count() == 1
        assert "timing" in pipeline.middleware_names()

    def test_timing_last_elapsed_after_dispatch(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        timing = pipeline.add_timing()
        # Before any dispatch, elapsed is None
        assert timing.last_elapsed_ms("ping") is None
        pipeline.dispatch("ping", "{}")
        elapsed = timing.last_elapsed_ms("ping")
        assert elapsed is not None
        assert elapsed >= 0

    def test_timing_unknown_action_none(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        timing = pipeline.add_timing()
        assert timing.last_elapsed_ms("does_not_exist") is None

    def test_timing_middleware_direct_construction(self):
        m = TimingMiddleware()
        assert "TimingMiddleware" in repr(m)


# ── TestAuditMiddleware ───────────────────────────────────────────────────────


class TestAuditMiddleware:
    def test_add_audit_returns_instance(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        assert isinstance(audit, AuditMiddleware)
        assert pipeline.middleware_count() == 1
        assert "audit" in pipeline.middleware_names()

    def test_audit_records_after_successful_dispatch(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        assert audit.record_count() == 0
        pipeline.dispatch("ping", "{}")
        assert audit.record_count() == 1
        records = audit.records()
        assert len(records) == 1
        r = records[0]
        assert r["action"] == "ping"
        assert r["success"] is True
        assert r["error"] is None
        assert "timestamp_ms" in r

    def test_audit_records_for_action(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        pipeline.dispatch("ping", "{}")
        assert len(audit.records_for_action("ping")) == 1
        assert len(audit.records_for_action("missing")) == 0

    def test_audit_clear(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        pipeline.dispatch("ping", "{}")
        assert audit.record_count() == 1
        audit.clear()
        assert audit.record_count() == 0

    def test_audit_multiple_dispatches(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        for _ in range(5):
            pipeline.dispatch("ping", "{}")
        assert audit.record_count() == 5

    def test_audit_without_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("ping", "{}")
        # Should still record the entry
        assert audit.record_count() == 1

    def test_audit_middleware_direct_construction(self):
        m = AuditMiddleware()
        assert m.record_count() == 0
        assert "AuditMiddleware" in repr(m)


# ── TestRateLimitMiddleware ───────────────────────────────────────────────────


class TestRateLimitMiddleware:
    def test_add_rate_limit_returns_instance(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        assert isinstance(rl, RateLimitMiddleware)
        assert rl.max_calls == 10
        assert rl.window_ms == 1000
        assert pipeline.middleware_count() == 1

    def test_call_count_increments(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        rl = pipeline.add_rate_limit(max_calls=100, window_ms=60000)
        assert rl.call_count("ping") == 0
        pipeline.dispatch("ping", "{}")
        assert rl.call_count("ping") == 1
        pipeline.dispatch("ping", "{}")
        assert rl.call_count("ping") == 2

    def test_rate_limit_exceeded_raises_runtime_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_rate_limit(max_calls=2, window_ms=60000)
        pipeline.dispatch("ping", "{}")
        pipeline.dispatch("ping", "{}")
        with pytest.raises(RuntimeError, match="rate limit exceeded"):
            pipeline.dispatch("ping", "{}")

    def test_rate_limit_per_action(self):
        reg = ToolRegistry()
        reg.register("ping", dcc="mock")
        reg.register("pong", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("ping", lambda p: "pong")
        dispatcher.register_handler("pong", lambda p: "ping")
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_rate_limit(max_calls=1, window_ms=60000)
        pipeline.dispatch("ping", "{}")
        # pong counter is independent
        pipeline.dispatch("pong", "{}")
        with pytest.raises(RuntimeError):
            pipeline.dispatch("ping", "{}")

    def test_rate_limit_middleware_direct_construction(self):
        m = RateLimitMiddleware(max_calls=5, window_ms=1000)
        assert m.max_calls == 5
        assert m.window_ms == 1000
        assert "RateLimitMiddleware" in repr(m)


# ── TestCallableMiddleware ────────────────────────────────────────────────────


class TestCallableMiddleware:
    def test_add_callable_before_fn(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        called = []
        pipeline.add_callable(before_fn=lambda name: called.append(("before", name)))
        pipeline.dispatch("ping", "{}")
        assert ("before", "ping") in called

    def test_add_callable_after_fn(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        called = []
        pipeline.add_callable(after_fn=lambda name, ok: called.append(("after", name, ok)))
        pipeline.dispatch("ping", "{}")
        assert ("after", "ping", True) in called

    def test_add_callable_both(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        log = []
        pipeline.add_callable(
            before_fn=lambda n: log.append(f"before:{n}"),
            after_fn=lambda n, ok: log.append(f"after:{n}:{ok}"),
        )
        pipeline.dispatch("ping", "{}")
        assert "before:ping" in log
        assert "after:ping:True" in log

    def test_add_callable_non_callable_before_raises(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.add_callable(before_fn="not-callable")

    def test_add_callable_non_callable_after_raises(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.add_callable(after_fn=42)

    def test_add_callable_middleware_count(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_callable(before_fn=lambda n: None)
        assert pipeline.middleware_count() == 1
        assert "python_callable" in pipeline.middleware_names()


# ── TestMultipleMiddleware ────────────────────────────────────────────────────


class TestMultipleMiddleware:
    def test_combined_logging_timing_audit(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging()
        timing = pipeline.add_timing()
        audit = pipeline.add_audit()
        assert pipeline.middleware_count() == 3
        assert pipeline.middleware_names() == ["logging", "timing", "audit"]

        pipeline.dispatch("ping", "{}")
        assert timing.last_elapsed_ms("ping") is not None
        assert audit.record_count() == 1

    def test_middleware_order_preserved(self):
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging()
        pipeline.add_timing()
        pipeline.add_rate_limit(max_calls=100, window_ms=60000)
        pipeline.add_callable(before_fn=lambda n: None)
        assert pipeline.middleware_names() == [
            "logging",
            "timing",
            "rate_limit",
            "python_callable",
        ]


# ── TestCallableMiddlewareEdgeCases ───────────────────────────────────────────


class TestCallableMiddlewareEdgeCases:
    """Edge cases for add_callable: None args, exception propagation, stacking."""

    def test_add_callable_none_none_no_error(self):
        """add_callable(None, None) must not raise and should add 1 middleware."""
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        # Must not raise — both hooks are optional
        pipeline.add_callable(before_fn=None, after_fn=None)
        # A middleware entry is still registered
        assert pipeline.middleware_count() == 1
        # Dispatch still works fine
        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"

    def test_add_callable_after_fn_called_when_handler_raises(self):
        """after_fn is called even when handler raises; success reflects middleware outcome.

        Note: The Rust implementation calls after_fn before re-raising handler exceptions,
        so after_fn receives success=True and the exception still propagates to the caller.
        """
        reg = ToolRegistry()
        reg.register("boom", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("boom", lambda params: (_ for _ in ()).throw(RuntimeError("kaboom")))
        pipeline = ToolPipeline(dispatcher)
        after_log = []
        pipeline.add_callable(after_fn=lambda name, ok: after_log.append((name, ok)))
        with pytest.raises(RuntimeError):
            pipeline.dispatch("boom", "{}")
        # after_fn IS called even when handler raised
        assert len(after_log) == 1
        assert after_log[0][0] == "boom"
        # success=True because the middleware itself succeeded (handler error is re-raised after after_fn)

    def test_stacking_multiple_add_callable_all_called(self):
        """Multiple add_callable calls stack — all hooks are invoked in order."""
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        log: list[str] = []
        pipeline.add_callable(before_fn=lambda n: log.append("A-before"))
        pipeline.add_callable(before_fn=lambda n: log.append("B-before"))
        pipeline.dispatch("ping", "{}")
        assert "A-before" in log
        assert "B-before" in log
        assert log.index("A-before") < log.index("B-before")

    def test_add_callable_before_fn_exception_propagates(self):
        """If before_fn raises, the dispatch propagates the exception."""
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        pipeline.add_callable(before_fn=lambda n: (_ for _ in ()).throw(ValueError("before-fail")))
        # The exception from before_fn should propagate
        with pytest.raises((ValueError, RuntimeError)):
            pipeline.dispatch("ping", "{}")

    def test_add_callable_only_after_fn(self):
        """Only after_fn, no before_fn — dispatch should succeed."""
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        called = []
        pipeline.add_callable(after_fn=lambda name, ok: called.append(ok))
        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"
        assert called == [True]

    def test_add_callable_only_before_fn(self):
        """Only before_fn, no after_fn — dispatch should succeed."""
        _, dispatcher = make_dispatcher()
        pipeline = ToolPipeline(dispatcher)
        called = []
        pipeline.add_callable(before_fn=lambda name: called.append(name))
        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"
        assert "ping" in called


# ── TestActionDispatcherEdgeCases ─────────────────────────────────────────────


class TestActionDispatcherEdgeCases:
    """Edge cases for ToolDispatcher: skip_empty_schema_validation, handler_names, etc."""

    def test_skip_empty_schema_validation_default_true(self):
        reg = ToolRegistry()
        reg.register("noop", dcc="mock")  # empty schema → skip_empty_schema_validation matters
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("noop", lambda p: "ok")
        # Default: skip_empty_schema_validation is True
        assert dispatcher.skip_empty_schema_validation is True

    def test_skip_empty_schema_validation_setter(self):
        reg = ToolRegistry()
        reg.register("noop", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.skip_empty_schema_validation = False
        assert dispatcher.skip_empty_schema_validation is False
        dispatcher.skip_empty_schema_validation = True
        assert dispatcher.skip_empty_schema_validation is True

    def test_handler_names_empty(self):
        reg = ToolRegistry()
        dispatcher = ToolDispatcher(reg)
        assert dispatcher.handler_names() == []

    def test_handler_names_sorted(self):
        reg = ToolRegistry()
        for name in ["zoo", "alpha", "middle"]:
            reg.register(name, dcc="mock")
        dispatcher = ToolDispatcher(reg)
        for name in ["zoo", "alpha", "middle"]:
            dispatcher.register_handler(name, lambda p: None)
        names = dispatcher.handler_names()
        assert names == sorted(names)
        assert set(names) == {"zoo", "alpha", "middle"}

    def test_has_handler_false_before_register(self):
        reg = ToolRegistry()
        reg.register("act", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        assert dispatcher.has_handler("act") is False

    def test_has_handler_true_after_register(self):
        reg = ToolRegistry()
        reg.register("act", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("act", lambda p: None)
        assert dispatcher.has_handler("act") is True

    def test_remove_handler_returns_true_if_existed(self):
        reg = ToolRegistry()
        reg.register("act", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("act", lambda p: None)
        assert dispatcher.remove_handler("act") is True

    def test_remove_handler_returns_false_if_not_existed(self):
        reg = ToolRegistry()
        dispatcher = ToolDispatcher(reg)
        assert dispatcher.remove_handler("nonexistent") is False

    def test_remove_handler_then_dispatch_raises_key_error(self):
        reg = ToolRegistry()
        reg.register("act", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("act", lambda p: "ok")
        dispatcher.remove_handler("act")
        with pytest.raises(KeyError):
            dispatcher.dispatch("act", "{}")

    def test_dispatch_with_null_params_default(self):
        """Dispatching without params_json uses default 'null'."""
        reg = ToolRegistry()
        reg.register("greet", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("greet", lambda p: "hello")
        result = dispatcher.dispatch("greet")
        assert result["output"] == "hello"

    def test_handler_count_after_multiple_registrations(self):
        reg = ToolRegistry()
        for i in range(5):
            reg.register(f"action_{i}", dcc="mock")
        dispatcher = ToolDispatcher(reg)
        for i in range(5):
            dispatcher.register_handler(f"action_{i}", lambda p: None)
        assert dispatcher.handler_count() == 5
        dispatcher.remove_handler("action_0")
        assert dispatcher.handler_count() == 4
