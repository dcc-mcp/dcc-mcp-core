"""Deep tests for ActionDispatcher and ActionPipeline.

Covers:
- ActionDispatcher.dispatch() success/failure/schema validation paths
- ActionDispatcher.handler_count/handler_names/has_handler/remove_handler
- ActionDispatcher.skip_empty_schema_validation property
- ActionPipeline.middleware_names() after adding each middleware type
- ActionPipeline.handler_count() before/after registering handlers
- ActionPipeline.dispatch() full round-trip with audit/timing/rate-limit
- AuditMiddleware.records() field structure
- TimingMiddleware.last_elapsed_ms() after dispatch
- RateLimitMiddleware.call_count() increment
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import RateLimitMiddleware
from dcc_mcp_core import TimingMiddleware

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_registry_dispatcher(name: str = "ping", schema: str = ""):
    """Return a (registry, dispatcher) pair with one registered action."""
    reg = ActionRegistry()
    reg.register(name, input_schema=schema)
    d = ActionDispatcher(reg)
    return reg, d


# ---------------------------------------------------------------------------
# ActionDispatcher - happy path
# ---------------------------------------------------------------------------


class TestActionDispatcherHappyPath:
    def test_dispatch_returns_output(self):
        _, d = _make_registry_dispatcher("echo")
        d.register_handler("echo", lambda p: p)
        result = d.dispatch("echo", '{"x": 1}')
        assert result["action"] == "echo"
        assert result["output"] == {"x": 1}

    def test_dispatch_returns_string_output(self):
        _, d = _make_registry_dispatcher("greet")
        d.register_handler("greet", lambda p: "hello")
        result = d.dispatch("greet", "{}")
        assert result["output"] == "hello"

    def test_dispatch_returns_list_output(self):
        _, d = _make_registry_dispatcher("list")
        d.register_handler("list", lambda p: [1, 2, 3])
        result = d.dispatch("list", "{}")
        assert result["output"] == [1, 2, 3]

    def test_dispatch_returns_none_output(self):
        _, d = _make_registry_dispatcher("noop")
        d.register_handler("noop", lambda p: None)
        result = d.dispatch("noop", "{}")
        assert result["output"] is None

    def test_dispatch_action_key_matches_name(self):
        _, d = _make_registry_dispatcher("create_sphere")
        d.register_handler("create_sphere", lambda p: True)
        result = d.dispatch("create_sphere", "{}")
        assert result["action"] == "create_sphere"

    def test_dispatch_validation_skipped_when_no_schema(self):
        _, d = _make_registry_dispatcher("cmd", schema="")
        d.register_handler("cmd", lambda p: "ok")
        result = d.dispatch("cmd", "{}")
        assert result["validation_skipped"] is True

    def test_dispatch_with_params_none_json(self):
        _, d = _make_registry_dispatcher("op")
        d.register_handler("op", lambda p: "done")
        result = d.dispatch("op", "null")
        assert result["output"] == "done"

    def test_dispatch_params_passed_as_dict(self):
        received = {}

        def capture(params):
            received.update(params)
            return True

        _, d = _make_registry_dispatcher("capture")
        d.register_handler("capture", capture)
        d.dispatch("capture", '{"radius": 2.0, "name": "sphere1"}')
        assert received["radius"] == pytest.approx(2.0)
        assert received["name"] == "sphere1"


# ---------------------------------------------------------------------------
# ActionDispatcher - schema validation path
# ---------------------------------------------------------------------------


class TestActionDispatcherSchemaValidation:
    def test_valid_params_schema_not_skipped(self):
        schema = json.dumps(
            {
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number", "minimum": 0.0}},
            }
        )
        _, d = _make_registry_dispatcher("sphere", schema=schema)
        d.register_handler("sphere", lambda p: p["radius"])
        result = d.dispatch("sphere", '{"radius": 1.5}')
        assert result["validation_skipped"] is False
        assert result["output"] == pytest.approx(1.5)

    def test_invalid_params_raises_value_error(self):
        schema = json.dumps(
            {
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number"}},
            }
        )
        _, d = _make_registry_dispatcher("sphere2", schema=schema)
        d.register_handler("sphere2", lambda p: True)
        with pytest.raises((ValueError, RuntimeError)):
            d.dispatch("sphere2", "{}")

    def test_skip_empty_schema_validation_default_true(self):
        _, d = _make_registry_dispatcher("x")
        assert d.skip_empty_schema_validation is True

    def test_skip_empty_schema_validation_setter(self):
        _, d = _make_registry_dispatcher("y")
        d.skip_empty_schema_validation = False
        assert d.skip_empty_schema_validation is False
        d.skip_empty_schema_validation = True
        assert d.skip_empty_schema_validation is True


# ---------------------------------------------------------------------------
# ActionDispatcher - error path
# ---------------------------------------------------------------------------


class TestActionDispatcherErrorPath:
    def test_dispatch_no_handler_raises_key_error(self):
        _, d = _make_registry_dispatcher("unregistered")
        with pytest.raises(KeyError):
            d.dispatch("unregistered", "{}")

    def test_dispatch_handler_raises_runtime_error(self):
        _, d = _make_registry_dispatcher("boom")

        def raise_handler(params):
            raise ValueError("bad input")

        d.register_handler("boom", raise_handler)
        with pytest.raises(RuntimeError):
            d.dispatch("boom", "{}")

    def test_dispatch_unknown_action_not_in_registry_raises(self):
        _, d = _make_registry_dispatcher("x")
        with pytest.raises((KeyError, RuntimeError)):
            d.dispatch("not_registered_at_all", "{}")


# ---------------------------------------------------------------------------
# ActionDispatcher - handler management
# ---------------------------------------------------------------------------


class TestActionDispatcherHandlerManagement:
    def test_register_handler_increments_count(self):
        _, d = _make_registry_dispatcher("a")
        assert d.handler_count() == 0
        d.register_handler("a", lambda p: None)
        assert d.handler_count() == 1

    def test_has_handler_true_after_register(self):
        _, d = _make_registry_dispatcher("b")
        d.register_handler("b", lambda p: None)
        assert d.has_handler("b") is True

    def test_has_handler_false_before_register(self):
        _, d = _make_registry_dispatcher("c")
        assert d.has_handler("c") is False

    def test_remove_handler_returns_true(self):
        _, d = _make_registry_dispatcher("d")
        d.register_handler("d", lambda p: None)
        removed = d.remove_handler("d")
        assert removed is True

    def test_remove_handler_decrements_count(self):
        _, d = _make_registry_dispatcher("e")
        d.register_handler("e", lambda p: None)
        d.remove_handler("e")
        assert d.handler_count() == 0

    def test_remove_handler_not_existing_returns_false(self):
        _, d = _make_registry_dispatcher("f")
        removed = d.remove_handler("f")
        assert removed is False

    def test_handler_names_sorted(self):
        reg = ActionRegistry()
        for name in ["zebra", "alpha", "mango"]:
            reg.register(name)
        d = ActionDispatcher(reg)
        for _n in ["zebra", "alpha", "mango"]:
            d.register_handler(_n, lambda p, n=_n: n)
        names = d.handler_names()
        assert names == sorted(names)
        assert set(names) == {"zebra", "alpha", "mango"}

    def test_non_callable_handler_raises_type_error(self):
        _, d = _make_registry_dispatcher("g")
        with pytest.raises(TypeError):
            d.register_handler("g", "not_a_callable")  # type: ignore[arg-type]


# ---------------------------------------------------------------------------
# ActionPipeline - middleware_names()
# ---------------------------------------------------------------------------


class TestActionPipelineMiddlewareNames:
    def _make_pipeline(self) -> ActionPipeline:
        reg = ActionRegistry()
        reg.register("ping")
        d = ActionDispatcher(reg)
        d.register_handler("ping", lambda p: "pong")
        return ActionPipeline(d)

    def test_empty_pipeline_has_no_names(self):
        pl = self._make_pipeline()
        assert pl.middleware_names() == []
        assert pl.middleware_count() == 0

    def test_add_logging_adds_logging_name(self):
        pl = self._make_pipeline()
        pl.add_logging()
        assert "logging" in pl.middleware_names()
        assert pl.middleware_count() == 1

    def test_add_timing_adds_timing_name(self):
        pl = self._make_pipeline()
        pl.add_timing()
        assert "timing" in pl.middleware_names()

    def test_add_audit_adds_audit_name(self):
        pl = self._make_pipeline()
        pl.add_audit()
        assert "audit" in pl.middleware_names()

    def test_add_rate_limit_adds_rate_limit_name(self):
        pl = self._make_pipeline()
        pl.add_rate_limit(max_calls=10, window_ms=1000)
        assert "rate_limit" in pl.middleware_names()

    def test_add_callable_adds_python_callable_name(self):
        pl = self._make_pipeline()
        pl.add_callable(before_fn=lambda a: None)
        assert "python_callable" in pl.middleware_names()

    def test_all_five_middleware_names(self):
        pl = self._make_pipeline()
        pl.add_logging()
        pl.add_timing()
        pl.add_audit()
        pl.add_rate_limit(max_calls=100, window_ms=5000)
        pl.add_callable(before_fn=lambda a: None, after_fn=lambda a, s: None)
        names = pl.middleware_names()
        assert set(names) == {"logging", "timing", "audit", "rate_limit", "python_callable"}
        assert pl.middleware_count() == 5

    def test_middleware_count_increments(self):
        pl = self._make_pipeline()
        assert pl.middleware_count() == 0
        pl.add_logging()
        assert pl.middleware_count() == 1
        pl.add_timing()
        assert pl.middleware_count() == 2


# ---------------------------------------------------------------------------
# ActionPipeline - handler_count()
# ---------------------------------------------------------------------------


class TestActionPipelineHandlerCount:
    def test_handler_count_initially_mirrors_dispatcher(self):
        reg = ActionRegistry()
        reg.register("a")
        d = ActionDispatcher(reg)
        d.register_handler("a", lambda p: None)
        pl = ActionPipeline(d)
        assert pl.handler_count() == 1

    def test_register_handler_on_pipeline_increments_count(self):
        reg = ActionRegistry()
        reg.register("a")
        reg.register("b")
        d = ActionDispatcher(reg)
        d.register_handler("a", lambda p: None)
        pl = ActionPipeline(d)
        count_before = pl.handler_count()
        pl.register_handler("b", lambda p: None)
        assert pl.handler_count() == count_before + 1


# ---------------------------------------------------------------------------
# ActionPipeline - full dispatch round-trip
# ---------------------------------------------------------------------------


class TestActionPipelineDispatchRoundTrip:
    def _setup(self):
        reg = ActionRegistry()
        reg.register("create_sphere")
        d = ActionDispatcher(reg)
        d.register_handler("create_sphere", lambda p: {"name": "sphere1"})
        pl = ActionPipeline(d)
        return pl

    def test_dispatch_returns_correct_output(self):
        pl = self._setup()
        result = pl.dispatch("create_sphere", "{}")
        assert result["output"] == {"name": "sphere1"}
        assert result["action"] == "create_sphere"

    def test_audit_records_after_dispatch(self):
        pl = self._setup()
        audit: AuditMiddleware = pl.add_audit()
        pl.dispatch("create_sphere", "{}")
        records = audit.records()
        assert len(records) == 1
        assert records[0]["action"] == "create_sphere"
        assert records[0]["success"] is True

    def test_audit_record_has_timestamp_ms(self):
        pl = self._setup()
        audit: AuditMiddleware = pl.add_audit()
        pl.dispatch("create_sphere", "{}")
        r = audit.records()[0]
        assert "timestamp_ms" in r
        assert isinstance(r["timestamp_ms"], int)
        assert r["timestamp_ms"] > 0

    def test_audit_records_for_action(self):
        pl = self._setup()
        audit: AuditMiddleware = pl.add_audit()
        pl.dispatch("create_sphere", "{}")
        specific = audit.records_for_action("create_sphere")
        assert len(specific) == 1

    def test_timing_last_elapsed_ms_is_set(self):
        pl = self._setup()
        timing: TimingMiddleware = pl.add_timing()
        assert timing.last_elapsed_ms("create_sphere") is None
        pl.dispatch("create_sphere", "{}")
        elapsed = timing.last_elapsed_ms("create_sphere")
        assert elapsed is not None
        assert elapsed >= 0

    def test_rate_limit_call_count_increments(self):
        pl = self._setup()
        rl: RateLimitMiddleware = pl.add_rate_limit(max_calls=100, window_ms=5000)
        assert rl.call_count("create_sphere") == 0
        pl.dispatch("create_sphere", "{}")
        assert rl.call_count("create_sphere") == 1
        pl.dispatch("create_sphere", "{}")
        assert rl.call_count("create_sphere") == 2

    def test_callable_middleware_before_fn_called(self):
        pl = self._setup()
        called = []
        pl.add_callable(before_fn=lambda a: called.append(a))
        pl.dispatch("create_sphere", "{}")
        assert "create_sphere" in called

    def test_callable_middleware_after_fn_called(self):
        pl = self._setup()
        after_calls = []
        pl.add_callable(after_fn=lambda a, s: after_calls.append((a, s)))
        pl.dispatch("create_sphere", "{}")
        assert len(after_calls) == 1
        assert after_calls[0][0] == "create_sphere"
        assert after_calls[0][1] is True
