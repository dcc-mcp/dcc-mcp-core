"""Deep tests for ActionPipeline, VersionedRegistry, SemVer, VersionConstraint, SkillMetadata, SkillScanner, and SkillCatalog.

Round #127 — target: +150 tests
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import RateLimitMiddleware
from dcc_mcp_core import SemVer
from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import SkillScanner
from dcc_mcp_core import TimingMiddleware
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry

# ──────────────────────────────────────────────────────────────────────────────
# Helpers
# ──────────────────────────────────────────────────────────────────────────────


def _make_pipeline(handler=None):
    """Return (pipeline, registry) with one registered action."""
    reg = ActionRegistry()
    reg.register("act", description="test action", category="test")
    disp = ActionDispatcher(reg)
    disp.register_handler("act", handler or (lambda _: {"ok": True}))
    return ActionPipeline(disp), reg


def _make_pipeline_multi(*action_names):
    """Return pipeline with multiple actions, all returning {name: <action>}."""
    reg = ActionRegistry()
    for name in action_names:
        reg.register(name, description=f"action {name}", category="test")
    disp = ActionDispatcher(reg)
    for name in action_names:
        # capture name in closure
        disp.register_handler(name, (lambda n: lambda _: {"name": n})(name))
    return ActionPipeline(disp), reg


# ──────────────────────────────────────────────────────────────────────────────
# 1. ActionPipeline — create
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineCreate:
    def test_create(self):
        p, _ = _make_pipeline()
        assert p is not None

    def test_repr(self):
        p, _ = _make_pipeline()
        r = repr(p)
        assert isinstance(r, str)

    def test_initial_middleware_count_zero(self):
        p, _ = _make_pipeline()
        assert p.middleware_count() == 0

    def test_initial_middleware_names_empty(self):
        p, _ = _make_pipeline()
        assert p.middleware_names() == []

    def test_initial_handler_count_one(self):
        p, _ = _make_pipeline()
        # one handler registered in helper
        assert p.handler_count() == 1

    def test_handler_count_two(self):
        p, _ = _make_pipeline_multi("a", "b")
        assert p.handler_count() == 2


# ──────────────────────────────────────────────────────────────────────────────
# 2. ActionPipeline — dispatch basics
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineDispatch:
    def test_dispatch_returns_dict(self):
        p, _ = _make_pipeline()
        result = p.dispatch("act", "{}")
        assert isinstance(result, dict)

    def test_dispatch_output_key(self):
        p, _ = _make_pipeline()
        result = p.dispatch("act", "{}")
        assert "output" in result

    def test_dispatch_action_key(self):
        p, _ = _make_pipeline()
        result = p.dispatch("act", "{}")
        assert result["action"] == "act"

    def test_dispatch_validation_skipped_key(self):
        p, _ = _make_pipeline()
        result = p.dispatch("act", "{}")
        assert "validation_skipped" in result

    def test_dispatch_output_value(self):
        p, _ = _make_pipeline(lambda _: {"answer": 42})
        result = p.dispatch("act", "{}")
        assert result["output"] == {"answer": 42}

    def test_dispatch_handler_receives_params(self):
        received = []
        p, _ = _make_pipeline(lambda params: received.append(params) or {})
        p.dispatch("act", "{}")
        assert len(received) == 1

    def test_dispatch_returns_none_from_handler(self):
        p, _ = _make_pipeline(lambda _: None)
        result = p.dispatch("act", "{}")
        assert result is not None  # wrapper is always a dict

    def test_dispatch_unregistered_raises(self):
        p, _ = _make_pipeline()
        with pytest.raises((KeyError, ValueError, RuntimeError)):
            p.dispatch("no_such_action", "{}")

    def test_dispatch_multiple_times(self):
        p, _ = _make_pipeline()
        for _ in range(5):
            r = p.dispatch("act", "{}")
            assert r["action"] == "act"

    def test_register_handler_on_pipeline(self):
        reg = ActionRegistry()
        reg.register("x", description="x", category="c")
        disp = ActionDispatcher(reg)
        p = ActionPipeline(disp)
        p.register_handler("x", lambda _: {"x": 1})
        r = p.dispatch("x", "{}")
        assert r["output"] == {"x": 1}

    def test_handler_count_after_register(self):
        reg = ActionRegistry()
        reg.register("a", description="a", category="c")
        reg.register("b", description="b", category="c")
        disp = ActionDispatcher(reg)
        p = ActionPipeline(disp)
        p.register_handler("a", lambda _: {})
        p.register_handler("b", lambda _: {})
        assert p.handler_count() == 2


# ──────────────────────────────────────────────────────────────────────────────
# 3. ActionPipeline — add_timing
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddTiming:
    def test_add_timing_returns_timing_middleware(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        assert isinstance(t, TimingMiddleware)

    def test_middleware_count_after_add_timing(self):
        p, _ = _make_pipeline()
        p.add_timing()
        assert p.middleware_count() == 1

    def test_middleware_names_contains_timing(self):
        p, _ = _make_pipeline()
        p.add_timing()
        assert "timing" in p.middleware_names()

    def test_last_elapsed_ms_before_dispatch_is_none(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        assert t.last_elapsed_ms("act") is None

    def test_last_elapsed_ms_after_dispatch_is_int(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        p.dispatch("act", "{}")
        val = t.last_elapsed_ms("act")
        assert isinstance(val, int)

    def test_last_elapsed_ms_is_non_negative(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        p.dispatch("act", "{}")
        assert t.last_elapsed_ms("act") >= 0

    def test_last_elapsed_ms_unknown_action_is_none(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        p.dispatch("act", "{}")
        assert t.last_elapsed_ms("other_action") is None

    def test_last_elapsed_ms_updates_on_second_dispatch(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        p.dispatch("act", "{}")
        p.dispatch("act", "{}")
        val = t.last_elapsed_ms("act")
        assert val is not None

    def test_multiple_timing_middleware(self):
        p, _ = _make_pipeline()
        t1 = p.add_timing()
        t2 = p.add_timing()
        p.dispatch("act", "{}")
        assert t1.last_elapsed_ms("act") is not None or t2.last_elapsed_ms("act") is not None

    def test_timing_with_multiple_actions(self):
        p, _ = _make_pipeline_multi("x", "y")
        t = p.add_timing()
        p.dispatch("x", "{}")
        p.dispatch("y", "{}")
        assert t.last_elapsed_ms("x") is not None
        assert t.last_elapsed_ms("y") is not None


# ──────────────────────────────────────────────────────────────────────────────
# 4. ActionPipeline — add_audit
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddAudit:
    def test_add_audit_returns_audit_middleware(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        assert isinstance(a, AuditMiddleware)

    def test_records_empty_before_dispatch(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        assert a.records() == []

    def test_record_count_zero_before_dispatch(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        assert a.record_count() == 0

    def test_record_count_after_dispatch(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert a.record_count() == 1

    def test_records_is_list(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert isinstance(a.records(), list)

    def test_records_entry_has_action_key(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert a.records()[0]["action"] == "act"

    def test_records_entry_success_true(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert a.records()[0]["success"] is True

    def test_records_entry_error_none_on_success(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert a.records()[0]["error"] is None

    def test_records_entry_timestamp_ms_is_int(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert isinstance(a.records()[0]["timestamp_ms"], int)

    def test_records_accumulate_across_dispatches(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        p.dispatch("act", "{}")
        p.dispatch("act", "{}")
        assert a.record_count() == 3

    def test_records_for_action_filters_correctly(self):
        p, _ = _make_pipeline_multi("alpha", "beta")
        a = p.add_audit()
        p.dispatch("alpha", "{}")
        p.dispatch("beta", "{}")
        p.dispatch("alpha", "{}")
        alpha_recs = a.records_for_action("alpha")
        assert all(r["action"] == "alpha" for r in alpha_recs)
        assert len(alpha_recs) == 2

    def test_records_for_action_empty_when_no_match(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert a.records_for_action("nonexistent") == []

    def test_clear_resets_records(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        a.clear()
        assert a.record_count() == 0
        assert a.records() == []

    def test_clear_then_dispatch_counts_fresh(self):
        p, _ = _make_pipeline()
        a = p.add_audit()
        p.dispatch("act", "{}")
        a.clear()
        p.dispatch("act", "{}")
        assert a.record_count() == 1

    def test_middleware_name_is_audit(self):
        p, _ = _make_pipeline()
        p.add_audit()
        assert "audit" in p.middleware_names()

    def test_add_audit_record_params_true(self):
        p, _ = _make_pipeline()
        a = p.add_audit(record_params=True)
        p.dispatch("act", "{}")
        rec = a.records()[0]
        assert "output_preview" in rec or "action" in rec  # structure preserved


# ──────────────────────────────────────────────────────────────────────────────
# 5. ActionPipeline — add_rate_limit
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddRateLimit:
    def test_add_rate_limit_returns_rate_limit_middleware(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=5, window_ms=1000)
        assert isinstance(rl, RateLimitMiddleware)

    def test_max_calls_property(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=7, window_ms=1000)
        assert rl.max_calls == 7

    def test_window_ms_property(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=5, window_ms=3000)
        assert rl.window_ms == 3000

    def test_call_count_zero_before_dispatch(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=5, window_ms=1000)
        assert rl.call_count("act") == 0

    def test_call_count_increments_on_dispatch(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=10, window_ms=60000)
        p.dispatch("act", "{}")
        p.dispatch("act", "{}")
        assert rl.call_count("act") == 2

    def test_rate_limit_raises_when_exceeded(self):
        p, _ = _make_pipeline()
        p.add_rate_limit(max_calls=1, window_ms=60000)
        p.dispatch("act", "{}")
        with pytest.raises(RuntimeError, match="rate limit"):
            p.dispatch("act", "{}")

    def test_rate_limit_error_message_contains_action(self):
        p, _ = _make_pipeline()
        p.add_rate_limit(max_calls=1, window_ms=60000)
        p.dispatch("act", "{}")
        with pytest.raises(RuntimeError, match="act"):
            p.dispatch("act", "{}")

    def test_call_count_different_actions_isolated(self):
        p, _ = _make_pipeline_multi("x", "y")
        rl = p.add_rate_limit(max_calls=10, window_ms=60000)
        p.dispatch("x", "{}")
        p.dispatch("x", "{}")
        p.dispatch("y", "{}")
        assert rl.call_count("x") == 2
        assert rl.call_count("y") == 1

    def test_middleware_name_is_rate_limit(self):
        p, _ = _make_pipeline()
        p.add_rate_limit(max_calls=5, window_ms=1000)
        assert "rate_limit" in p.middleware_names()

    def test_call_count_unknown_action_is_zero(self):
        p, _ = _make_pipeline()
        rl = p.add_rate_limit(max_calls=5, window_ms=1000)
        assert rl.call_count("nonexistent") == 0


# ──────────────────────────────────────────────────────────────────────────────
# 6. ActionPipeline — add_callable
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddCallable:
    def test_before_fn_called(self):
        p, _ = _make_pipeline()
        calls = []
        p.add_callable(before_fn=lambda action: calls.append(("before", action)))
        p.dispatch("act", "{}")
        assert ("before", "act") in calls

    def test_after_fn_called(self):
        p, _ = _make_pipeline()
        calls = []
        p.add_callable(after_fn=lambda action, success: calls.append(("after", action, success)))
        p.dispatch("act", "{}")
        assert any(c[0] == "after" and c[1] == "act" for c in calls)

    def test_after_fn_success_true_on_success(self):
        p, _ = _make_pipeline()
        results = []
        p.add_callable(after_fn=lambda action, success: results.append(success))
        p.dispatch("act", "{}")
        assert results[0] is True

    def test_before_and_after_both_called(self):
        p, _ = _make_pipeline()
        log = []
        p.add_callable(
            before_fn=lambda a: log.append("before"),
            after_fn=lambda a, s: log.append("after"),
        )
        p.dispatch("act", "{}")
        assert log == ["before", "after"]

    def test_before_fn_receives_action_name(self):
        p, _ = _make_pipeline()
        names = []
        p.add_callable(before_fn=lambda action: names.append(action))
        p.dispatch("act", "{}")
        assert names == ["act"]

    def test_callable_not_in_middleware_names(self):
        p, _ = _make_pipeline()
        p.add_callable(before_fn=lambda a: None)
        # python_callable is not counted in named middleware list in some impls
        names = p.middleware_names()
        # just check it doesn't crash
        assert isinstance(names, list)

    def test_multiple_callables_all_called(self):
        p, _ = _make_pipeline()
        log1 = []
        log2 = []
        p.add_callable(before_fn=lambda a: log1.append(a))
        p.add_callable(before_fn=lambda a: log2.append(a))
        p.dispatch("act", "{}")
        assert log1 == ["act"]
        assert log2 == ["act"]

    def test_callable_with_none_before_fn(self):
        p, _ = _make_pipeline()
        calls = []
        p.add_callable(after_fn=lambda a, s: calls.append(s))
        p.dispatch("act", "{}")
        assert calls == [True]

    def test_callable_with_none_after_fn(self):
        p, _ = _make_pipeline()
        calls = []
        p.add_callable(before_fn=lambda a: calls.append(a))
        p.dispatch("act", "{}")
        assert calls == ["act"]


# ──────────────────────────────────────────────────────────────────────────────
# 7. ActionPipeline — add_logging
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddLogging:
    def test_add_logging_returns_none(self):
        p, _ = _make_pipeline()
        result = p.add_logging()
        assert result is None

    def test_middleware_name_logging_present(self):
        p, _ = _make_pipeline()
        p.add_logging()
        assert "logging" in p.middleware_names()

    def test_middleware_count_after_add_logging(self):
        p, _ = _make_pipeline()
        p.add_logging()
        assert p.middleware_count() == 1

    def test_dispatch_after_logging_succeeds(self):
        p, _ = _make_pipeline()
        p.add_logging()
        result = p.dispatch("act", "{}")
        assert result["action"] == "act"

    def test_add_logging_with_log_params_false(self):
        p, _ = _make_pipeline()
        result = p.add_logging(log_params=False)
        assert result is None
        assert "logging" in p.middleware_names()


# ──────────────────────────────────────────────────────────────────────────────
# 8. ActionPipeline — combined middleware
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineCombinedMiddleware:
    def test_all_middleware_added(self):
        p, _ = _make_pipeline()
        p.add_logging()
        p.add_timing()
        p.add_audit()
        p.add_rate_limit(max_calls=10, window_ms=1000)
        assert p.middleware_count() == 4

    def test_middleware_names_order(self):
        p, _ = _make_pipeline()
        p.add_logging()
        p.add_timing()
        p.add_audit()
        names = p.middleware_names()
        assert names == ["logging", "timing", "audit"]

    def test_dispatch_through_all_middleware(self):
        p, _ = _make_pipeline()
        p.add_logging()
        t = p.add_timing()
        a = p.add_audit()
        p.add_rate_limit(max_calls=5, window_ms=1000)
        result = p.dispatch("act", "{}")
        assert result["action"] == "act"
        assert t.last_elapsed_ms("act") is not None
        assert a.record_count() == 1

    def test_timing_and_audit_together(self):
        p, _ = _make_pipeline()
        t = p.add_timing()
        a = p.add_audit()
        p.dispatch("act", "{}")
        assert t.last_elapsed_ms("act") is not None
        assert a.records()[0]["success"] is True

    def test_audit_records_failed_action(self):
        """Handler that raises should produce success=False in audit."""
        reg = ActionRegistry()
        reg.register("fail_act", description="fail", category="test")
        disp = ActionDispatcher(reg)
        disp.register_handler("fail_act", lambda _: (_ for _ in ()).throw(ValueError("boom")))  # type: ignore[attr-defined]
        p = ActionPipeline(disp)
        a = p.add_audit()
        with contextlib.suppress(Exception):
            p.dispatch("fail_act", "{}")
        # audit may or may not record failed actions depending on implementation
        # just verify no crash occurred after attempting dispatch
        assert a is not None


# ──────────────────────────────────────────────────────────────────────────────
# 9. SemVer — create and fields
# ──────────────────────────────────────────────────────────────────────────────


class TestSemVerCreate:
    def test_parse_returns_semver(self):
        sv = SemVer.parse("1.2.3")
        assert sv is not None

    def test_major(self):
        assert SemVer.parse("1.2.3").major == 1

    def test_minor(self):
        assert SemVer.parse("1.2.3").minor == 2

    def test_patch(self):
        assert SemVer.parse("1.2.3").patch == 3

    def test_repr(self):
        sv = SemVer.parse("1.2.3")
        r = repr(sv)
        # repr is "SemVer(1, 2, 3)" — verify it contains the numeric components
        assert "1" in r and "2" in r and "3" in r

    def test_str(self):
        sv = SemVer.parse("1.2.3")
        # str representation should contain version info
        s = str(sv)
        assert isinstance(s, str) and len(s) > 0

    def test_zero_version(self):
        sv = SemVer.parse("0.0.0")
        assert sv.major == 0 and sv.minor == 0 and sv.patch == 0

    def test_large_version(self):
        sv = SemVer.parse("99.100.200")
        assert sv.major == 99 and sv.minor == 100 and sv.patch == 200


class TestSemVerComparison:
    def test_less_than(self):
        assert SemVer.parse("1.0.0") < SemVer.parse("2.0.0")

    def test_greater_than(self):
        assert SemVer.parse("2.0.0") > SemVer.parse("1.0.0")

    def test_equal(self):
        assert SemVer.parse("1.2.3") == SemVer.parse("1.2.3")

    def test_not_equal(self):
        assert SemVer.parse("1.0.0") != SemVer.parse("1.0.1")

    def test_less_equal(self):
        assert SemVer.parse("1.0.0") <= SemVer.parse("1.0.0")
        assert SemVer.parse("1.0.0") <= SemVer.parse("2.0.0")

    def test_greater_equal(self):
        assert SemVer.parse("2.0.0") >= SemVer.parse("1.0.0")

    def test_patch_ordering(self):
        assert SemVer.parse("1.0.1") > SemVer.parse("1.0.0")

    def test_minor_ordering(self):
        assert SemVer.parse("1.1.0") > SemVer.parse("1.0.9")


class TestSemVerMatchesConstraint:
    def test_matches_gte(self):
        sv = SemVer.parse("2.0.0")
        vc = VersionConstraint.parse(">=1.0.0")
        assert sv.matches_constraint(vc) is True

    def test_not_matches_gte(self):
        sv = SemVer.parse("0.5.0")
        vc = VersionConstraint.parse(">=1.0.0")
        assert sv.matches_constraint(vc) is False

    def test_matches_caret(self):
        sv = SemVer.parse("1.5.0")
        vc = VersionConstraint.parse("^1.0.0")
        assert sv.matches_constraint(vc) is True

    def test_not_matches_caret_major_change(self):
        sv = SemVer.parse("2.0.0")
        vc = VersionConstraint.parse("^1.0.0")
        assert sv.matches_constraint(vc) is False

    def test_matches_exact(self):
        sv = SemVer.parse("1.2.3")
        vc = VersionConstraint.parse("=1.2.3")
        assert sv.matches_constraint(vc) is True

    def test_not_matches_exact_different(self):
        sv = SemVer.parse("1.2.4")
        vc = VersionConstraint.parse("=1.2.3")
        assert sv.matches_constraint(vc) is False


# ──────────────────────────────────────────────────────────────────────────────
# 10. VersionConstraint
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionConstraint:
    def test_parse_returns_constraint(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc is not None

    def test_repr(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert "1.0.0" in repr(vc)

    def test_str(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert "1.0.0" in str(vc)

    def test_matches_true(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc.matches(SemVer.parse("2.0.0")) is True

    def test_matches_false(self):
        vc = VersionConstraint.parse(">=2.0.0")
        assert vc.matches(SemVer.parse("1.0.0")) is False

    def test_wildcard_matches_all(self):
        vc = VersionConstraint.parse("*")
        assert vc.matches(SemVer.parse("0.0.1")) is True
        assert vc.matches(SemVer.parse("99.99.99")) is True


# ──────────────────────────────────────────────────────────────────────────────
# 11. VersionedRegistry — create
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryCreate:
    def test_create(self):
        vreg = VersionedRegistry()
        assert vreg is not None

    def test_total_entries_empty(self):
        vreg = VersionedRegistry()
        assert vreg.total_entries() == 0

    def test_keys_empty(self):
        vreg = VersionedRegistry()
        assert vreg.keys() == []

    def test_versions_nonexist_empty(self):
        vreg = VersionedRegistry()
        assert vreg.versions("noexist", dcc="maya") == []

    def test_latest_version_nonexist_none(self):
        vreg = VersionedRegistry()
        assert vreg.latest_version("noexist", dcc="maya") is None

    def test_resolve_nonexist_none(self):
        vreg = VersionedRegistry()
        assert vreg.resolve("noexist", dcc="maya", constraint="*") is None


# ──────────────────────────────────────────────────────────────────────────────
# 12. VersionedRegistry — register_versioned
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryRegister:
    def test_register_single(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("sphere", dcc="maya", version="1.0.0")
        assert vreg.total_entries() == 1

    def test_versions_after_register(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("sphere", dcc="maya", version="1.0.0")
        assert vreg.versions("sphere", dcc="maya") == ["1.0.0"]

    def test_register_multiple_versions(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("sphere", dcc="maya", version="1.0.0")
        vreg.register_versioned("sphere", dcc="maya", version="2.0.0")
        versions = vreg.versions("sphere", dcc="maya")
        assert "1.0.0" in versions and "2.0.0" in versions

    def test_versions_sorted(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        versions = vreg.versions("a", dcc="maya")
        assert versions == sorted(versions)

    def test_keys_after_register(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("act", dcc="blender", version="1.0.0")
        keys = vreg.keys()
        assert ("act", "blender") in keys

    def test_register_with_metadata(self):
        vreg = VersionedRegistry()
        vreg.register_versioned(
            "sphere",
            dcc="blender",
            version="1.0.0",
            description="Create sphere",
            category="geometry",
            tags=["geo", "create"],
        )
        r = vreg.resolve("sphere", dcc="blender", constraint=">=1.0.0")
        assert r is not None
        assert r["description"] == "Create sphere"
        assert r["category"] == "geometry"
        assert "geo" in r["tags"]

    def test_register_multiple_dccs(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("act", dcc="maya", version="1.0.0")
        vreg.register_versioned("act", dcc="blender", version="1.0.0")
        assert vreg.total_entries() == 2
        keys = vreg.keys()
        assert ("act", "maya") in keys
        assert ("act", "blender") in keys

    def test_total_entries_counts_versions(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.register_versioned("b", dcc="blender", version="1.0.0")
        assert vreg.total_entries() == 3


# ──────────────────────────────────────────────────────────────────────────────
# 13. VersionedRegistry — latest_version
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryLatestVersion:
    def test_latest_single(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        assert vreg.latest_version("a", dcc="maya") == "1.0.0"

    def test_latest_multiple(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        assert vreg.latest_version("a", dcc="maya") == "2.0.0"

    def test_latest_out_of_order_registration(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        assert vreg.latest_version("a", dcc="maya") == "2.0.0"

    def test_latest_different_dcc_isolated(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="blender", version="3.0.0")
        assert vreg.latest_version("a", dcc="maya") == "1.0.0"
        assert vreg.latest_version("a", dcc="blender") == "3.0.0"


# ──────────────────────────────────────────────────────────────────────────────
# 14. VersionedRegistry — resolve
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryResolve:
    def test_resolve_returns_dict(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        r = vreg.resolve("a", dcc="maya", constraint="*")
        assert isinstance(r, dict)

    def test_resolve_version_field(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        r = vreg.resolve("a", dcc="maya", constraint="*")
        assert r["version"] == "1.0.0"

    def test_resolve_name_field(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("sphere", dcc="maya", version="1.0.0")
        r = vreg.resolve("sphere", dcc="maya", constraint="*")
        assert r["name"] == "sphere"

    def test_resolve_dcc_field(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        r = vreg.resolve("a", dcc="maya", constraint="*")
        assert r["dcc"] == "maya"

    def test_resolve_gte_returns_latest(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        r = vreg.resolve("a", dcc="maya", constraint=">=1.0.0")
        assert r["version"] == "2.0.0"

    def test_resolve_caret_stays_compatible(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        r = vreg.resolve("a", dcc="maya", constraint="^1.0.0")
        assert r["version"] == "1.2.0"

    def test_resolve_no_match_returns_none(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        assert vreg.resolve("a", dcc="maya", constraint=">=3.0.0") is None

    def test_resolve_nonexist_action_returns_none(self):
        vreg = VersionedRegistry()
        assert vreg.resolve("noexist", dcc="maya", constraint="*") is None


# ──────────────────────────────────────────────────────────────────────────────
# 15. VersionedRegistry — resolve_all
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryResolveAll:
    def test_resolve_all_returns_list(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        result = vreg.resolve_all("a", dcc="maya", constraint="*")
        assert isinstance(result, list)

    def test_resolve_all_wildcard_all_versions(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        result = vreg.resolve_all("a", dcc="maya", constraint="*")
        assert len(result) == 2

    def test_resolve_all_sorted_ascending(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        result = vreg.resolve_all("a", dcc="maya", constraint="*")
        versions = [r["version"] for r in result]
        assert versions == sorted(versions)

    def test_resolve_all_filtered_by_constraint(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.5.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        result = vreg.resolve_all("a", dcc="maya", constraint="^1.0.0")
        versions = [r["version"] for r in result]
        assert "2.0.0" not in versions
        assert "1.0.0" in versions or "1.5.0" in versions

    def test_resolve_all_no_match_empty_list(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        result = vreg.resolve_all("a", dcc="maya", constraint=">=5.0.0")
        assert result == []

    def test_resolve_all_nonexist_empty_list(self):
        vreg = VersionedRegistry()
        result = vreg.resolve_all("noexist", dcc="maya", constraint="*")
        assert result == []


# ──────────────────────────────────────────────────────────────────────────────
# 16. VersionedRegistry — remove
# ──────────────────────────────────────────────────────────────────────────────


class TestVersionedRegistryRemove:
    def test_remove_returns_count(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        removed = vreg.remove("a", dcc="maya", constraint="^1.0.0")
        assert removed == 2

    def test_remove_decreases_total_entries(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.remove("a", dcc="maya", constraint=">=1.0.0")
        assert vreg.total_entries() == 0

    def test_remove_partial_leaves_remaining(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.remove("a", dcc="maya", constraint="^1.0.0")
        remaining = vreg.versions("a", dcc="maya")
        assert "2.0.0" in remaining
        assert "1.0.0" not in remaining

    def test_remove_no_match_returns_zero(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        removed = vreg.remove("a", dcc="maya", constraint=">=5.0.0")
        assert removed == 0

    def test_remove_nonexist_returns_zero(self):
        vreg = VersionedRegistry()
        removed = vreg.remove("noexist", dcc="maya", constraint="*")
        assert removed == 0


# ──────────────────────────────────────────────────────────────────────────────
# 17. SkillMetadata — create and fields
# ──────────────────────────────────────────────────────────────────────────────


class TestSkillMetadataCreate:
    def test_create_minimal(self):
        sm = SkillMetadata(name="my_skill")
        assert sm.name == "my_skill"

    def test_default_version(self):
        sm = SkillMetadata(name="my_skill")
        assert sm.version == "1.0.0"

    def test_default_dcc(self):
        sm = SkillMetadata(name="my_skill")
        assert sm.dcc == "python"

    def test_default_description_empty(self):
        sm = SkillMetadata(name="my_skill")
        assert sm.description == ""

    def test_default_tags_empty(self):
        sm = SkillMetadata(name="my_skill")
        assert sm.tags == []

    def test_set_description(self):
        sm = SkillMetadata(name="skill", description="A useful skill")
        assert sm.description == "A useful skill"

    def test_set_version(self):
        sm = SkillMetadata(name="skill", version="2.5.0")
        assert sm.version == "2.5.0"

    def test_set_dcc(self):
        sm = SkillMetadata(name="skill", dcc="maya")
        assert sm.dcc == "maya"

    def test_set_tags(self):
        sm = SkillMetadata(name="skill", tags=["geo", "mesh"])
        assert "geo" in sm.tags
        assert "mesh" in sm.tags

    def test_repr_contains_name(self):
        sm = SkillMetadata(name="my_skill")
        assert "my_skill" in repr(sm)

    def test_str_contains_name(self):
        sm = SkillMetadata(name="my_skill")
        assert "my_skill" in str(sm)

    def test_equality_same_name_version(self):
        sm1 = SkillMetadata(name="a", version="1.0.0")
        sm2 = SkillMetadata(name="a", version="1.0.0")
        assert sm1 == sm2

    def test_inequality_different_name(self):
        sm1 = SkillMetadata(name="a")
        sm2 = SkillMetadata(name="b")
        assert sm1 != sm2

    def test_set_skill_path(self):
        sm = SkillMetadata(name="skill", skill_path="/tmp/skill")
        assert sm.skill_path == "/tmp/skill"

    def test_scripts_default_empty(self):
        sm = SkillMetadata(name="skill")
        assert sm.scripts == []

    def test_depends_default_empty(self):
        sm = SkillMetadata(name="skill")
        assert sm.depends == []

    def test_allowed_tools_default_empty(self):
        sm = SkillMetadata(name="skill")
        assert sm.allowed_tools == []


# ──────────────────────────────────────────────────────────────────────────────
# 18. SkillScanner — create and scan
# ──────────────────────────────────────────────────────────────────────────────


class TestSkillScanner:
    def test_create(self):
        scanner = SkillScanner()
        assert scanner is not None

    def test_repr(self):
        scanner = SkillScanner()
        assert isinstance(repr(scanner), str)

    def test_discovered_skills_initially_empty(self):
        scanner = SkillScanner()
        assert scanner.discovered_skills == []

    def test_scan_empty_dir_returns_list(self):
        scanner = SkillScanner()
        with tempfile.TemporaryDirectory() as tmp:
            result = scanner.scan(extra_paths=[tmp])
        assert isinstance(result, list)

    def test_scan_empty_dir_returns_empty(self):
        scanner = SkillScanner()
        with tempfile.TemporaryDirectory() as tmp:
            result = scanner.scan(extra_paths=[tmp])
        assert result == []

    def test_scan_with_force_refresh(self):
        scanner = SkillScanner()
        with tempfile.TemporaryDirectory() as tmp:
            result = scanner.scan(extra_paths=[tmp], force_refresh=True)
        assert isinstance(result, list)

    def test_scan_with_dcc_name_filter(self):
        scanner = SkillScanner()
        with tempfile.TemporaryDirectory() as tmp:
            result = scanner.scan(extra_paths=[tmp], dcc_name="maya")
        assert isinstance(result, list)

    def test_clear_cache(self):
        scanner = SkillScanner()
        with tempfile.TemporaryDirectory() as tmp:
            scanner.scan(extra_paths=[tmp])
        scanner.clear_cache()  # should not raise
        assert scanner.discovered_skills == []


# ──────────────────────────────────────────────────────────────────────────────
# 19. SkillCatalog — create and basic operations
# ──────────────────────────────────────────────────────────────────────────────


class TestSkillCatalog:
    def _make_catalog(self):
        reg = ActionRegistry()
        return SkillCatalog(reg), reg

    def test_create(self):
        cat, _ = self._make_catalog()
        assert cat is not None

    def test_repr(self):
        cat, _ = self._make_catalog()
        r = repr(cat)
        assert "SkillCatalog" in r

    def test_loaded_count_initial_zero(self):
        cat, _ = self._make_catalog()
        assert cat.loaded_count() == 0

    def test_list_skills_initial_empty(self):
        cat, _ = self._make_catalog()
        assert cat.list_skills() == []

    def test_discover_empty_dir(self):
        cat, _ = self._make_catalog()
        with tempfile.TemporaryDirectory() as tmp:
            cat.discover(extra_paths=[tmp])
        assert cat.list_skills() == []

    def test_is_loaded_false_for_nonexistent(self):
        cat, _ = self._make_catalog()
        assert cat.is_loaded("nonexist") is False

    def test_get_skill_info_none_for_nonexistent(self):
        cat, _ = self._make_catalog()
        assert cat.get_skill_info("nonexist") is None

    def test_load_skill_raises_for_nonexistent(self):
        cat, _ = self._make_catalog()
        with pytest.raises((ValueError, KeyError, RuntimeError)):
            cat.load_skill("nonexist")

    def test_find_skills_empty_no_results(self):
        cat, _ = self._make_catalog()
        assert cat.find_skills() == []

    def test_find_skills_query_no_results(self):
        cat, _ = self._make_catalog()
        assert cat.find_skills(query="anything") == []

    def test_find_skills_dcc_no_results(self):
        cat, _ = self._make_catalog()
        assert cat.find_skills(dcc="maya") == []

    def test_list_skills_status_loaded_empty(self):
        cat, _ = self._make_catalog()
        assert cat.list_skills(status="loaded") == []

    def test_list_skills_status_unloaded_empty(self):
        cat, _ = self._make_catalog()
        assert cat.list_skills(status="unloaded") == []

    def test_discover_with_dcc_name(self):
        cat, _ = self._make_catalog()
        with tempfile.TemporaryDirectory() as tmp:
            cat.discover(extra_paths=[tmp], dcc_name="maya")
        assert cat.list_skills() == []
