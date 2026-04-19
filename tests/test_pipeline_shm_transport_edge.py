"""Tests for ToolPipeline middleware edge cases, PySharedSceneBuffer, PyBufferPool, TransportManager.

New test classes (target: ~120 tests):

- TestRateLimitMiddleware          (20)
- TestPythonCallableMiddleware     (22)
- TestAuditMiddlewareDetails       (18)
- TestTimingMiddlewareDetails      (12)
- TestPySharedSceneBufferDescriptor (20)
- TestPyBufferPoolLifecycle        (18)
- TestTransportManagerDeregister   (18)
"""

from __future__ import annotations

# Import built-in modules
import json
import tempfile
import time

# Import third-party modules
import pytest

from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PySharedSceneBuffer

# Import local modules
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager

# ---------------------------------------------------------------------------
# Helper: build a fresh pipeline with N registered actions
# ---------------------------------------------------------------------------


def _make_pipeline(action_names: list[str]) -> tuple[ToolPipeline, ToolRegistry]:
    """Return (pipeline, registry) with each action registered and handled."""
    reg = ToolRegistry()
    for name in action_names:
        reg.register(name, description=f"action {name}", category="test")
    dispatcher = ToolDispatcher(reg)
    for name in action_names:
        dispatcher.register_handler(name, lambda params, n=name: {"action": n, "ok": True})
    pipeline = ToolPipeline(dispatcher)
    return pipeline, reg


# ===========================================================================
# TestRateLimitMiddleware
# ===========================================================================


class TestRateLimitMiddleware:
    """Verify RateLimitMiddleware behaviour: counting, limits, per-action isolation."""

    def test_rate_limit_allows_calls_within_limit(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=5, window_ms=1000)
        for _ in range(5):
            pipeline.dispatch("act", "{}")
        assert rl.call_count("act") == 5

    def test_rate_limit_blocks_on_exceed(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=2, window_ms=5000)
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        with pytest.raises(RuntimeError, match="rate limit exceeded"):
            pipeline.dispatch("act", "{}")

    def test_rate_limit_error_message_contains_action_name(self):
        pipeline, _ = _make_pipeline(["my_special_act"])
        pipeline.add_rate_limit(max_calls=1, window_ms=5000)
        pipeline.dispatch("my_special_act", "{}")
        with pytest.raises(RuntimeError, match="my_special_act"):
            pipeline.dispatch("my_special_act", "{}")

    def test_rate_limit_error_message_contains_max_calls(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=3, window_ms=5000)
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        with pytest.raises(RuntimeError, match="3"):
            pipeline.dispatch("act", "{}")

    def test_rate_limit_max_calls_property(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=7, window_ms=2000)
        assert rl.max_calls == 7

    def test_rate_limit_window_ms_property(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=5, window_ms=3500)
        assert rl.window_ms == 3500

    def test_rate_limit_call_count_starts_at_zero(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        assert rl.call_count("act") == 0

    def test_rate_limit_call_count_unknown_action_is_zero(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        pipeline.dispatch("act", "{}")
        assert rl.call_count("nonexistent") == 0

    def test_rate_limit_call_count_increments_per_dispatch(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        for i in range(1, 6):
            pipeline.dispatch("act", "{}")
            assert rl.call_count("act") == i

    def test_rate_limit_per_action_isolation(self):
        pipeline, _ = _make_pipeline(["a1", "a2"])
        rl = pipeline.add_rate_limit(max_calls=2, window_ms=5000)
        pipeline.dispatch("a1", "{}")
        pipeline.dispatch("a1", "{}")
        assert rl.call_count("a1") == 2
        assert rl.call_count("a2") == 0
        # a2 should still work
        pipeline.dispatch("a2", "{}")
        assert rl.call_count("a2") == 1

    def test_rate_limit_a1_exhausted_a2_still_works(self):
        pipeline, _ = _make_pipeline(["a1", "a2"])
        pipeline.add_rate_limit(max_calls=1, window_ms=5000)
        pipeline.dispatch("a1", "{}")
        with pytest.raises(RuntimeError):
            pipeline.dispatch("a1", "{}")
        result = pipeline.dispatch("a2", "{}")
        assert result["output"]["ok"] is True

    def test_rate_limit_window_reset_after_wait(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=1, window_ms=100)
        pipeline.dispatch("act", "{}")
        time.sleep(0.15)
        # After window expires, should be allowed again
        result = pipeline.dispatch("act", "{}")
        assert result["output"]["ok"] is True

    def test_rate_limit_in_middleware_names(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=5, window_ms=1000)
        assert "rate_limit" in pipeline.middleware_names()

    def test_rate_limit_counted_in_middleware_count(self):
        pipeline, _ = _make_pipeline(["act"])
        before = pipeline.middleware_count()
        pipeline.add_rate_limit(max_calls=5, window_ms=1000)
        assert pipeline.middleware_count() == before + 1

    def test_rate_limit_max_calls_one(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=1, window_ms=5000)
        pipeline.dispatch("act", "{}")
        assert rl.call_count("act") == 1
        with pytest.raises(RuntimeError):
            pipeline.dispatch("act", "{}")

    def test_rate_limit_dispatch_result_has_correct_keys(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        result = pipeline.dispatch("act", "{}")
        assert set(result.keys()) == {"action", "output", "validation_skipped"}

    def test_rate_limit_dispatch_action_key_correct(self):
        pipeline, _ = _make_pipeline(["named_act"])
        pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        result = pipeline.dispatch("named_act", "{}")
        assert result["action"] == "named_act"

    def test_rate_limit_high_max_calls(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=1000, window_ms=1000)
        for _ in range(100):
            pipeline.dispatch("act", "{}")
        assert rl.call_count("act") == 100

    def test_rate_limit_multiple_rate_limits_stacked(self):
        pipeline, _ = _make_pipeline(["act"])
        rl1 = pipeline.add_rate_limit(max_calls=3, window_ms=5000)
        rl2 = pipeline.add_rate_limit(max_calls=10, window_ms=5000)
        pipeline.dispatch("act", "{}")
        assert rl1.call_count("act") == 1
        assert rl2.call_count("act") == 1

    def test_rate_limit_with_json_params(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_rate_limit(max_calls=5, window_ms=1000)
        result = pipeline.dispatch("act", '{"key": "value", "num": 42}')
        assert result["output"]["ok"] is True

    def test_rate_limit_exactly_at_max_succeeds(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=4, window_ms=5000)
        for _ in range(4):
            pipeline.dispatch("act", "{}")
        assert rl.call_count("act") == 4


# ===========================================================================
# TestPythonCallableMiddleware
# ===========================================================================


class TestPythonCallableMiddleware:
    """Verify Python callable (before_fn / after_fn) middleware."""

    def test_before_fn_called_on_dispatch(self):
        pipeline, _ = _make_pipeline(["act"])
        calls = []
        pipeline.add_callable(
            before_fn=lambda action: calls.append(("before", action)),
            after_fn=None,
        )
        pipeline.dispatch("act", "{}")
        assert calls == [("before", "act")]

    def test_after_fn_called_on_dispatch(self):
        pipeline, _ = _make_pipeline(["act"])
        calls = []
        pipeline.add_callable(
            before_fn=None,
            after_fn=lambda action, success: calls.append(("after", action, success)),
        )
        pipeline.dispatch("act", "{}")
        assert calls == [("after", "act", True)]

    def test_both_fn_called(self):
        pipeline, _ = _make_pipeline(["act"])
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append(f"before:{action}"),
            after_fn=lambda action, success: log.append(f"after:{action}:{success}"),
        )
        pipeline.dispatch("act", "{}")
        assert "before:act" in log
        assert "after:act:True" in log

    def test_after_fn_success_is_true_on_success(self):
        pipeline, _ = _make_pipeline(["act"])
        successes = []
        pipeline.add_callable(
            before_fn=None,
            after_fn=lambda action, success: successes.append(success),
        )
        pipeline.dispatch("act", "{}")
        assert successes == [True]

    def test_multiple_callable_middleware_executed(self):
        pipeline, _ = _make_pipeline(["act"])
        log1, log2 = [], []
        pipeline.add_callable(
            before_fn=lambda action: log1.append("1_before"),
            after_fn=lambda action, success: log1.append("1_after"),
        )
        pipeline.add_callable(
            before_fn=lambda action: log2.append("2_before"),
            after_fn=lambda action, success: log2.append("2_after"),
        )
        pipeline.dispatch("act", "{}")
        assert log1 == ["1_before", "1_after"]
        assert log2 == ["2_before", "2_after"]

    def test_callable_middleware_counted_in_middleware_count(self):
        pipeline, _ = _make_pipeline(["act"])
        before = pipeline.middleware_count()
        pipeline.add_callable(
            before_fn=lambda action: None,
            after_fn=None,
        )
        assert pipeline.middleware_count() == before + 1

    def test_callable_middleware_appears_in_middleware_names(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_callable(
            before_fn=lambda action: None,
            after_fn=None,
        )
        assert "python_callable" in pipeline.middleware_names()

    def test_before_fn_receives_action_name(self):
        pipeline, _ = _make_pipeline(["my_action"])
        received = []
        pipeline.add_callable(
            before_fn=lambda action: received.append(action),
            after_fn=None,
        )
        pipeline.dispatch("my_action", "{}")
        assert received == ["my_action"]

    def test_after_fn_receives_action_name_and_success(self):
        pipeline, _ = _make_pipeline(["my_action"])
        received = []
        pipeline.add_callable(
            before_fn=None,
            after_fn=lambda action, success: received.append((action, success)),
        )
        pipeline.dispatch("my_action", "{}")
        assert received == [("my_action", True)]

    def test_none_before_fn_ok(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_callable(before_fn=None, after_fn=None)
        # Should dispatch without error
        result = pipeline.dispatch("act", "{}")
        assert result["output"]["ok"] is True

    def test_none_after_fn_ok(self):
        pipeline, _ = _make_pipeline(["act"])
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append(action),
            after_fn=None,
        )
        result = pipeline.dispatch("act", "{}")
        assert result["output"]["ok"] is True
        assert log == ["act"]

    def test_callable_middleware_chained_with_audit(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append("before"),
            after_fn=lambda action, success: log.append("after"),
        )
        pipeline.dispatch("act", "{}")
        assert log == ["before", "after"]
        assert audit.record_count() == 1

    def test_callable_middleware_with_rate_limit(self):
        pipeline, _ = _make_pipeline(["act"])
        rl = pipeline.add_rate_limit(max_calls=2, window_ms=5000)
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append("before"),
            after_fn=None,
        )
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        log.clear()
        with pytest.raises(RuntimeError):
            pipeline.dispatch("act", "{}")
        # Rate limit blocked the 3rd call; count may reflect 3 (counted before check)
        assert rl.call_count("act") >= 2

    def test_multiple_dispatches_accumulate_before_calls(self):
        pipeline, _ = _make_pipeline(["act"])
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append("before"),
            after_fn=None,
        )
        for _ in range(5):
            pipeline.dispatch("act", "{}")
        assert log.count("before") == 5

    def test_callable_order_before_then_after(self):
        pipeline, _ = _make_pipeline(["act"])
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append("before"),
            after_fn=lambda action, success: log.append("after"),
        )
        pipeline.dispatch("act", "{}")
        assert log.index("before") < log.index("after")

    def test_callable_middleware_two_actions(self):
        pipeline, _ = _make_pipeline(["a1", "a2"])
        log = []
        pipeline.add_callable(
            before_fn=lambda action: log.append(f"before:{action}"),
            after_fn=None,
        )
        pipeline.dispatch("a1", "{}")
        pipeline.dispatch("a2", "{}")
        assert "before:a1" in log
        assert "before:a2" in log

    def test_callable_middleware_side_effect_counter(self):
        pipeline, _ = _make_pipeline(["act"])
        counter = [0]
        pipeline.add_callable(
            before_fn=lambda action: counter.__setitem__(0, counter[0] + 1),
            after_fn=None,
        )
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        assert counter[0] == 3

    def test_callable_middleware_collects_results(self):
        pipeline, _ = _make_pipeline(["act"])
        results = []
        pipeline.add_callable(
            before_fn=None,
            after_fn=lambda action, success: results.append(success),
        )
        for _ in range(4):
            pipeline.dispatch("act", "{}")
        assert results == [True, True, True, True]

    def test_callable_handler_registered_directly_on_pipeline(self):
        _pipeline, _ = _make_pipeline([])
        reg = ToolRegistry()
        reg.register("direct", description="direct", category="test")
        dispatcher = ToolDispatcher(reg)
        p2 = ToolPipeline(dispatcher)
        p2.register_handler("direct", lambda params: {"direct": True})
        result = p2.dispatch("direct", "{}")
        assert result["output"]["direct"] is True

    def test_handler_count_reflects_registered_handlers(self):
        pipeline, _ = _make_pipeline(["a", "b", "c"])
        assert pipeline.handler_count() == 3

    def test_pipeline_register_handler_increments_count(self):
        pipeline, _ = _make_pipeline(["a"])
        before = pipeline.handler_count()
        pipeline.register_handler("new_handler", lambda params: {})
        assert pipeline.handler_count() == before + 1


# ===========================================================================
# TestAuditMiddlewareDetails
# ===========================================================================


class TestAuditMiddlewareDetails:
    """Verify AuditMiddleware record structure and behavior."""

    def test_audit_record_has_expected_keys(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert len(records) == 1
        record = records[0]
        assert "action" in record
        assert "success" in record
        assert "error" in record
        assert "timestamp_ms" in record

    def test_audit_record_action_value(self):
        pipeline, _ = _make_pipeline(["my_action"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("my_action", "{}")
        records = audit.records()
        assert records[0]["action"] == "my_action"

    def test_audit_record_success_true_on_success(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert records[0]["success"] is True

    def test_audit_record_error_none_on_success(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert records[0]["error"] is None

    def test_audit_record_timestamp_ms_is_int(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert isinstance(records[0]["timestamp_ms"], int)
        assert records[0]["timestamp_ms"] > 0

    def test_audit_multiple_records_ordered(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert len(records) == 3
        # Timestamps should be non-decreasing
        ts = [r["timestamp_ms"] for r in records]
        assert ts == sorted(ts)

    def test_audit_record_count_increments(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        for i in range(1, 6):
            pipeline.dispatch("act", "{}")
            assert audit.record_count() == i

    def test_audit_clear_resets_records(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        pipeline.dispatch("act", "{}")
        assert audit.record_count() == 2
        audit.clear()
        assert audit.record_count() == 0
        assert audit.records() == []

    def test_audit_records_for_action_filtered(self):
        pipeline, _ = _make_pipeline(["a1", "a2"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("a1", "{}")
        pipeline.dispatch("a2", "{}")
        pipeline.dispatch("a1", "{}")
        r1 = audit.records_for_action("a1")
        r2 = audit.records_for_action("a2")
        assert len(r1) == 2
        assert len(r2) == 1
        assert all(r["action"] == "a1" for r in r1)
        assert all(r["action"] == "a2" for r in r2)

    def test_audit_records_for_unknown_action(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        result = audit.records_for_action("nonexistent")
        assert result == []

    def test_audit_appears_in_middleware_names(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_audit(record_params=False)
        assert "audit" in pipeline.middleware_names()

    def test_audit_counted_in_middleware_count(self):
        pipeline, _ = _make_pipeline(["act"])
        before = pipeline.middleware_count()
        pipeline.add_audit(record_params=False)
        assert pipeline.middleware_count() == before + 1

    def test_audit_output_preview_field_exists(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=True)
        pipeline.dispatch("act", "{}")
        records = audit.records()
        assert "output_preview" in records[0]

    def test_audit_record_after_clear_is_fresh(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        pipeline.dispatch("act", "{}")
        audit.clear()
        pipeline.dispatch("act", "{}")
        assert audit.record_count() == 1

    def test_audit_records_returns_list(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        assert isinstance(audit.records(), list)

    def test_audit_initial_record_count_zero(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        assert audit.record_count() == 0

    def test_audit_initial_records_empty(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        assert audit.records() == []

    def test_audit_records_for_action_returns_list(self):
        pipeline, _ = _make_pipeline(["act"])
        audit = pipeline.add_audit(record_params=False)
        assert isinstance(audit.records_for_action("act"), list)


# ===========================================================================
# TestTimingMiddlewareDetails
# ===========================================================================


class TestTimingMiddlewareDetails:
    """Verify TimingMiddleware last_elapsed_ms behavior."""

    def test_timing_last_elapsed_ms_is_int(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        pipeline.dispatch("act", "{}")
        val = timing.last_elapsed_ms("act")
        assert isinstance(val, int)

    def test_timing_last_elapsed_ms_nonnegative(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        pipeline.dispatch("act", "{}")
        assert timing.last_elapsed_ms("act") >= 0

    def test_timing_unknown_action_returns_none(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        assert timing.last_elapsed_ms("unknown") is None

    def test_timing_before_any_dispatch_returns_none(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        assert timing.last_elapsed_ms("act") is None

    def test_timing_updates_after_second_dispatch(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        pipeline.dispatch("act", "{}")
        _ = timing.last_elapsed_ms("act")
        pipeline.dispatch("act", "{}")
        val2 = timing.last_elapsed_ms("act")
        assert val2 is not None
        assert val2 >= 0

    def test_timing_per_action_independence(self):
        pipeline, _ = _make_pipeline(["a1", "a2"])
        timing = pipeline.add_timing()
        pipeline.dispatch("a1", "{}")
        assert timing.last_elapsed_ms("a1") is not None
        assert timing.last_elapsed_ms("a2") is None
        pipeline.dispatch("a2", "{}")
        assert timing.last_elapsed_ms("a2") is not None

    def test_timing_appears_in_middleware_names(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_timing()
        assert "timing" in pipeline.middleware_names()

    def test_timing_counted_in_middleware_count(self):
        pipeline, _ = _make_pipeline(["act"])
        before = pipeline.middleware_count()
        pipeline.add_timing()
        assert pipeline.middleware_count() == before + 1

    def test_timing_large_number_of_dispatches_records_last(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        for _ in range(20):
            pipeline.dispatch("act", "{}")
        assert timing.last_elapsed_ms("act") is not None
        assert timing.last_elapsed_ms("act") >= 0

    def test_timing_stacked_with_audit_and_rate_limit(self):
        pipeline, _ = _make_pipeline(["act"])
        timing = pipeline.add_timing()
        audit = pipeline.add_audit(record_params=False)
        pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        pipeline.dispatch("act", "{}")
        assert timing.last_elapsed_ms("act") is not None
        assert audit.record_count() == 1
        assert pipeline.middleware_count() == 3

    def test_timing_logging_combination(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_logging(log_params=True)
        timing = pipeline.add_timing()
        pipeline.dispatch("act", "{}")
        assert timing.last_elapsed_ms("act") is not None

    def test_dispatch_result_validation_skipped_field(self):
        pipeline, _ = _make_pipeline(["act"])
        pipeline.add_timing()
        result = pipeline.dispatch("act", "{}")
        assert "validation_skipped" in result


# ===========================================================================
# TestPySharedSceneBufferDescriptor
# ===========================================================================


class TestPySharedSceneBufferDescriptor:
    """Verify PySharedSceneBuffer descriptor JSON and metadata."""

    def test_write_returns_scene_buffer(self):
        buf = PySharedSceneBuffer.write(b"hello")
        assert buf is not None

    def test_id_is_string(self):
        buf = PySharedSceneBuffer.write(b"data")
        assert isinstance(buf.id, str)

    def test_id_is_nonempty_uuid(self):
        buf = PySharedSceneBuffer.write(b"data")
        parts = buf.id.split("-")
        assert len(parts) == 5  # UUID format: 8-4-4-4-12

    def test_total_bytes_matches_input(self):
        data = b"hello world"
        buf = PySharedSceneBuffer.write(data)
        assert buf.total_bytes == len(data)

    def test_is_inline_small_data(self):
        buf = PySharedSceneBuffer.write(b"small")
        assert buf.is_inline is True

    def test_is_chunked_small_data(self):
        buf = PySharedSceneBuffer.write(b"small")
        assert buf.is_chunked is False

    def test_read_roundtrip(self):
        data = b"round trip data 1234"
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_read_returns_bytes(self):
        buf = PySharedSceneBuffer.write(b"bytes")
        assert isinstance(buf.read(), bytes)

    def test_descriptor_json_returns_string(self):
        buf = PySharedSceneBuffer.write(b"desc")
        desc = buf.descriptor_json()
        assert isinstance(desc, str)

    def test_descriptor_json_is_valid_json(self):
        buf = PySharedSceneBuffer.write(b"json test")
        desc = buf.descriptor_json()
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_descriptor_json_has_meta_key(self):
        buf = PySharedSceneBuffer.write(b"meta")
        desc = json.loads(buf.descriptor_json())
        assert "meta" in desc

    def test_descriptor_json_has_storage_key(self):
        buf = PySharedSceneBuffer.write(b"storage")
        desc = json.loads(buf.descriptor_json())
        assert "storage" in desc

    def test_descriptor_json_meta_has_id(self):
        buf = PySharedSceneBuffer.write(b"id test")
        desc = json.loads(buf.descriptor_json())
        assert "id" in desc["meta"]
        assert desc["meta"]["id"] == buf.id

    def test_descriptor_json_meta_total_bytes(self):
        data = b"total bytes test"
        buf = PySharedSceneBuffer.write(data)
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["total_bytes"] == len(data)

    def test_descriptor_json_storage_has_path(self):
        buf = PySharedSceneBuffer.write(b"path test")
        desc = json.loads(buf.descriptor_json())
        assert "name" in desc["storage"]
        assert isinstance(desc["storage"]["name"], str)

    def test_large_data_read_roundtrip(self):
        large = bytes(range(256)) * 4000  # ~1MB
        buf = PySharedSceneBuffer.write(large)
        assert buf.read() == large

    def test_large_data_total_bytes(self):
        large = b"x" * 500_000
        buf = PySharedSceneBuffer.write(large)
        assert buf.total_bytes == 500_000

    def test_empty_data_write_and_read(self):
        buf = PySharedSceneBuffer.write(b"")
        assert buf.total_bytes == 0
        assert buf.read() == b""

    def test_binary_data_write_and_read(self):
        data = bytes(range(256))
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_two_buffers_have_different_ids(self):
        buf1 = PySharedSceneBuffer.write(b"first")
        buf2 = PySharedSceneBuffer.write(b"second")
        assert buf1.id != buf2.id


# ===========================================================================
# TestPyBufferPoolLifecycle
# ===========================================================================


class TestPyBufferPoolLifecycle:
    """Verify PyBufferPool construction, acquire, read/write, clear."""

    def test_pool_construction(self):
        pool = PyBufferPool(capacity=5, buffer_size=256)
        assert pool is not None

    def test_pool_capacity(self):
        pool = PyBufferPool(capacity=5, buffer_size=256)
        assert pool.capacity() == 5

    def test_pool_buffer_size(self):
        pool = PyBufferPool(capacity=5, buffer_size=512)
        assert pool.buffer_size() == 512

    def test_pool_initial_available_equals_capacity(self):
        pool = PyBufferPool(capacity=4, buffer_size=128)
        assert pool.available() == 4

    def test_pool_available_initial_equals_capacity(self):
        pool = PyBufferPool(capacity=3, buffer_size=128)
        assert pool.available() == pool.capacity()

    def test_pool_acquire_returns_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=128)
        buf = pool.acquire()
        assert buf is not None

    def test_pool_acquired_buffer_has_id(self):
        pool = PyBufferPool(capacity=2, buffer_size=128)
        buf = pool.acquire()
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_pool_acquired_buffer_has_capacity(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        assert buf.capacity() == 256

    def test_pool_write_and_read(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        buf.write(b"hello pool")
        assert buf.read() == b"hello pool"

    def test_pool_data_len_after_write(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        buf.write(b"12345")
        assert buf.data_len() == 5

    def test_pool_clear_resets_data_len(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        buf.write(b"clear me")
        buf.clear()
        assert buf.data_len() == 0

    def test_pool_clear_allows_rewrite(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_pool_name_returns_string(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        p = buf.name()
        assert isinstance(p, str)
        assert len(p) > 0

    def test_pool_descriptor_json_returns_string(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        desc = buf.descriptor_json()
        assert isinstance(desc, str)

    def test_pool_descriptor_json_is_valid_json(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        desc = json.loads(buf.descriptor_json())
        assert isinstance(desc, dict)
        assert "id" in desc
        assert "capacity" in desc

    def test_pool_multiple_acquire_different_ids(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert b1.id != b2.id

    def test_pool_acquire_all_capacity(self):
        pool = PyBufferPool(capacity=3, buffer_size=64)
        bufs = [pool.acquire() for _ in range(3)]
        assert pool.available() == 0
        assert len(bufs) == 3

    def test_pool_capacity_fixed(self):
        pool = PyBufferPool(capacity=4, buffer_size=64)
        assert pool.capacity() == 4
        pool.acquire()
        assert pool.capacity() == 4  # capacity doesn't change


# ===========================================================================
# TestTransportManagerDeregister
# ===========================================================================


class TestTransportManagerDeregister:
    """Verify TransportManager.deregister_service removes instances correctly."""

    def _make_tm(self) -> TransportManager:
        tmpdir = tempfile.mkdtemp()
        return TransportManager(registry_dir=tmpdir)

    def test_deregister_returns_true_when_found(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        result = tm.deregister_service("maya", inst.instance_id)
        assert result is True

    def test_deregister_removes_instance(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        tm.deregister_service("maya", inst.instance_id)
        remaining = tm.list_instances("maya")
        assert len(remaining) == 0

    def test_deregister_nonexistent_uuid_returns_false(self):
        tm = self._make_tm()
        # Use a valid UUID format that doesn't exist in registry
        fake_uuid = "00000000-0000-0000-0000-000000000000"
        result = tm.deregister_service("maya", fake_uuid)
        assert result is False

    def test_deregister_does_not_remove_other_instances(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        instances = tm.list_instances("maya")
        assert len(instances) == 2
        tm.deregister_service("maya", instances[0].instance_id)
        remaining = tm.list_instances("maya")
        assert len(remaining) == 1

    def test_deregister_does_not_affect_other_dcc_types(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        maya_inst = tm.list_instances("maya")[0]
        tm.deregister_service("maya", maya_inst.instance_id)
        blender_remaining = tm.list_instances("blender")
        assert len(blender_remaining) == 1

    def test_deregister_with_multiple_instances(self):
        tm = self._make_tm()
        for port in range(9001, 9006):
            tm.register_service("houdini", "127.0.0.1", port)
        instances = tm.list_instances("houdini")
        assert len(instances) == 5
        tm.deregister_service("houdini", instances[2].instance_id)
        remaining = tm.list_instances("houdini")
        assert len(remaining) == 4

    def test_service_entry_has_instance_id_attribute(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        assert hasattr(inst, "instance_id")
        assert isinstance(inst.instance_id, str)

    def test_service_entry_has_dcc_type_attribute(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        assert inst.dcc_type == "maya"

    def test_service_entry_has_host_attribute(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        assert inst.host == "127.0.0.1"

    def test_service_entry_has_port_attribute(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        assert inst.port == 9001

    def test_list_all_instances_includes_all_dcc_types(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        tm.register_service("houdini", "127.0.0.1", 9003)
        all_instances = tm.list_all_instances()
        dcc_types = {inst.dcc_type for inst in all_instances}
        assert dcc_types == {"maya", "blender", "houdini"}

    def test_list_all_services_same_as_list_all_instances(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        services = tm.list_all_services()
        instances = tm.list_all_instances()
        assert len(services) == len(instances)

    def test_list_instances_empty_for_unknown_dcc(self):
        tm = self._make_tm()
        assert tm.list_instances("unknown_dcc") == []

    def test_deregister_all_instances_one_by_one(self):
        tm = self._make_tm()
        for port in range(9001, 9004):
            tm.register_service("maya", "127.0.0.1", port)
        instances = tm.list_instances("maya")
        for inst in instances:
            tm.deregister_service("maya", inst.instance_id)
        assert tm.list_instances("maya") == []

    def test_register_after_deregister(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        tm.deregister_service("maya", inst.instance_id)
        tm.register_service("maya", "127.0.0.1", 9001)
        assert len(tm.list_instances("maya")) == 1

    def test_service_entry_status_available(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        assert str(inst.status) == "AVAILABLE"

    def test_list_instances_returns_list(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        result = tm.list_instances("maya")
        assert isinstance(result, list)

    def test_deregister_second_call_returns_false(self):
        tm = self._make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        inst = tm.list_instances("maya")[0]
        tm.deregister_service("maya", inst.instance_id)
        result2 = tm.deregister_service("maya", inst.instance_id)
        assert result2 is False
