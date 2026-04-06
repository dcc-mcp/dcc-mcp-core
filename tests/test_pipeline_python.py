"""Python integration tests for ActionPipeline and middleware Python bindings.

Tests:
- ActionPipeline construction from ActionDispatcher
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

from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import LoggingMiddleware
from dcc_mcp_core import RateLimitMiddleware
from dcc_mcp_core import TimingMiddleware

# ── Fixtures ──────────────────────────────────────────────────────────────────


def make_dispatcher(schema: str = "{}") -> tuple[ActionRegistry, ActionDispatcher]:
    reg = ActionRegistry()
    reg.register("ping", category="util", dcc="mock", input_schema=schema)
    dispatcher = ActionDispatcher(reg)
    dispatcher.register_handler("ping", lambda params: "pong")
    return reg, dispatcher


# ── TestActionPipeline ────────────────────────────────────────────────────────


class TestActionPipelineBasic:
    """Basic construction and dispatch without middleware."""

    def test_construction(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        assert pipeline.handler_count() == 1
        assert pipeline.middleware_count() == 0

    def test_dispatch_happy_path(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        result = pipeline.dispatch("ping", "{}")
        assert result["action"] == "ping"
        assert result["output"] == "pong"

    def test_dispatch_default_null_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        result = pipeline.dispatch("ping")
        assert result["output"] == "pong"

    def test_dispatch_unknown_action_raises_key_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(KeyError, match="no handler"):
            pipeline.dispatch("nonexistent", "{}")

    def test_dispatch_invalid_json_raises_value_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(ValueError, match="invalid JSON"):
            pipeline.dispatch("ping", "not-json")

    def test_register_handler_after_construction(self):
        reg = ActionRegistry()
        reg.register("echo", dcc="mock")
        dispatcher = ActionDispatcher(reg)
        pipeline = ActionPipeline(dispatcher)
        pipeline.register_handler("echo", lambda params: params)
        assert pipeline.handler_count() == 1
        result = pipeline.dispatch("echo", '{"x": 1}')
        assert result["output"] == {"x": 1}

    def test_register_non_callable_raises_type_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.register_handler("ping", "not-callable")


# ── TestLoggingMiddleware ─────────────────────────────────────────────────────


class TestLoggingMiddleware:
    def test_add_logging(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        pipeline.add_logging()
        assert pipeline.middleware_count() == 1
        assert "logging" in pipeline.middleware_names()

    def test_add_logging_with_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        pipeline.add_logging(log_params=True)
        assert pipeline.middleware_count() == 1

    def test_dispatch_with_logging(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
        timing = pipeline.add_timing()
        assert isinstance(timing, TimingMiddleware)
        assert pipeline.middleware_count() == 1
        assert "timing" in pipeline.middleware_names()

    def test_timing_last_elapsed_after_dispatch(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        timing = pipeline.add_timing()
        # Before any dispatch, elapsed is None
        assert timing.last_elapsed_ms("ping") is None
        pipeline.dispatch("ping", "{}")
        elapsed = timing.last_elapsed_ms("ping")
        assert elapsed is not None
        assert elapsed >= 0

    def test_timing_unknown_action_none(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        timing = pipeline.add_timing()
        assert timing.last_elapsed_ms("does_not_exist") is None

    def test_timing_middleware_direct_construction(self):
        m = TimingMiddleware()
        assert "TimingMiddleware" in repr(m)


# ── TestAuditMiddleware ───────────────────────────────────────────────────────


class TestAuditMiddleware:
    def test_add_audit_returns_instance(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        audit = pipeline.add_audit()
        assert isinstance(audit, AuditMiddleware)
        assert pipeline.middleware_count() == 1
        assert "audit" in pipeline.middleware_names()

    def test_audit_records_after_successful_dispatch(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
        audit = pipeline.add_audit()
        pipeline.dispatch("ping", "{}")
        assert len(audit.records_for_action("ping")) == 1
        assert len(audit.records_for_action("missing")) == 0

    def test_audit_clear(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        audit = pipeline.add_audit()
        pipeline.dispatch("ping", "{}")
        assert audit.record_count() == 1
        audit.clear()
        assert audit.record_count() == 0

    def test_audit_multiple_dispatches(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        audit = pipeline.add_audit()
        for _ in range(5):
            pipeline.dispatch("ping", "{}")
        assert audit.record_count() == 5

    def test_audit_without_params(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        assert isinstance(rl, RateLimitMiddleware)
        assert rl.max_calls == 10
        assert rl.window_ms == 1000
        assert pipeline.middleware_count() == 1

    def test_call_count_increments(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        rl = pipeline.add_rate_limit(max_calls=100, window_ms=60000)
        assert rl.call_count("ping") == 0
        pipeline.dispatch("ping", "{}")
        assert rl.call_count("ping") == 1
        pipeline.dispatch("ping", "{}")
        assert rl.call_count("ping") == 2

    def test_rate_limit_exceeded_raises_runtime_error(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        pipeline.add_rate_limit(max_calls=2, window_ms=60000)
        pipeline.dispatch("ping", "{}")
        pipeline.dispatch("ping", "{}")
        with pytest.raises(RuntimeError, match="rate limit exceeded"):
            pipeline.dispatch("ping", "{}")

    def test_rate_limit_per_action(self):
        reg = ActionRegistry()
        reg.register("ping", dcc="mock")
        reg.register("pong", dcc="mock")
        dispatcher = ActionDispatcher(reg)
        dispatcher.register_handler("ping", lambda p: "pong")
        dispatcher.register_handler("pong", lambda p: "ping")
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
        called = []
        pipeline.add_callable(before_fn=lambda name: called.append(("before", name)))
        pipeline.dispatch("ping", "{}")
        assert ("before", "ping") in called

    def test_add_callable_after_fn(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        called = []
        pipeline.add_callable(after_fn=lambda name, ok: called.append(("after", name, ok)))
        pipeline.dispatch("ping", "{}")
        assert ("after", "ping", True) in called

    def test_add_callable_both(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.add_callable(before_fn="not-callable")

    def test_add_callable_non_callable_after_raises(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        with pytest.raises(TypeError):
            pipeline.add_callable(after_fn=42)

    def test_add_callable_middleware_count(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
        pipeline.add_callable(before_fn=lambda n: None)
        assert pipeline.middleware_count() == 1
        assert "python_callable" in pipeline.middleware_names()


# ── TestMultipleMiddleware ────────────────────────────────────────────────────


class TestMultipleMiddleware:
    def test_combined_logging_timing_audit(self):
        _, dispatcher = make_dispatcher()
        pipeline = ActionPipeline(dispatcher)
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
        pipeline = ActionPipeline(dispatcher)
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
