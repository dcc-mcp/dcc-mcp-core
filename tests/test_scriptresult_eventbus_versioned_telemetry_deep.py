"""Deep tests for ScriptResult, EventBus multi-subscriber, VersionedRegistry full methods.

TelemetryConfig builder, ToolRecorder / ToolMetrics, and RecordingGuard context manager.
"""

from __future__ import annotations

import threading
import time

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# ScriptResult
# ---------------------------------------------------------------------------


class TestScriptResultCreate:
    """ScriptResult construction and field access."""

    def test_success_true(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=10, output="ok", error=None, context={})
        assert r.success is True

    def test_success_false(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=False, execution_time_ms=0, output="", error="boom", context={})
        assert r.success is False

    def test_execution_time_ms(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=42, output="", error=None, context={})
        assert r.execution_time_ms == 42

    def test_execution_time_zero(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=0, output="", error=None, context={})
        assert r.execution_time_ms == 0

    def test_output_field(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=5, output="sphere1", error=None, context={})
        assert r.output == "sphere1"

    def test_output_empty(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        assert r.output == ""

    def test_error_none(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        assert r.error is None

    def test_error_message(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=False, execution_time_ms=0, output="", error="Script failed", context={})
        assert r.error == "Script failed"

    def test_context_empty(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        assert r.context == {}

    def test_context_with_data(self) -> None:
        r = dcc_mcp_core.ScriptResult(
            success=True, execution_time_ms=10, output="", error=None, context={"dcc": "maya", "version": "2025"}
        )
        assert r.context["dcc"] == "maya"
        assert r.context["version"] == "2025"

    def test_repr_contains_success(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=42, output="", error=None, context={})
        assert "true" in repr(r).lower() or "True" in repr(r)

    def test_repr_contains_time(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=99, output="", error=None, context={})
        assert "99" in repr(r)


class TestScriptResultToDict:
    """ScriptResult.to_dict() output."""

    def test_to_dict_has_success_key(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="x", error=None, context={})
        d = r.to_dict()
        assert "success" in d

    def test_to_dict_success_value(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="x", error=None, context={})
        assert r.to_dict()["success"] is True

    def test_to_dict_has_output(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="my_output", error=None, context={})
        assert r.to_dict()["output"] == "my_output"

    def test_to_dict_has_error(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=False, execution_time_ms=0, output="", error="err", context={})
        assert r.to_dict()["error"] == "err"

    def test_to_dict_error_none(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        assert r.to_dict()["error"] is None

    def test_to_dict_has_execution_time(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=55, output="", error=None, context={})
        assert r.to_dict()["execution_time_ms"] == 55

    def test_to_dict_has_context(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={"k": "v"})
        assert r.to_dict()["context"] == {"k": "v"}

    def test_to_dict_all_keys(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        keys = set(r.to_dict().keys())
        assert {"success", "output", "error", "execution_time_ms", "context"}.issubset(keys)

    def test_to_dict_new_dict_each_call(self) -> None:
        r = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        d1 = r.to_dict()
        d2 = r.to_dict()
        assert d1 == d2
        assert d1 is not d2


# ---------------------------------------------------------------------------
# EventBus
# ---------------------------------------------------------------------------


class TestEventBusBasic:
    """Basic EventBus operations."""

    def test_repr_format(self) -> None:
        bus = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(bus)

    def test_repr_subscriptions_zero(self) -> None:
        bus = dcc_mcp_core.EventBus()
        assert "0" in repr(bus)

    def test_subscribe_returns_id(self) -> None:
        bus = dcc_mcp_core.EventBus()
        sub_id = bus.subscribe("test_event", lambda **kw: None)
        assert sub_id is not None

    def test_subscribe_two_different_ids(self) -> None:
        bus = dcc_mcp_core.EventBus()
        id1 = bus.subscribe("evt", lambda **kw: None)
        id2 = bus.subscribe("evt", lambda **kw: None)
        assert id1 != id2

    def test_publish_calls_subscriber(self) -> None:
        bus = dcc_mcp_core.EventBus()
        received = []
        bus.subscribe("my_event", lambda **kw: received.append(kw))
        bus.publish("my_event", value=99)
        assert len(received) == 1
        assert received[0]["value"] == 99

    def test_publish_passes_kwargs(self) -> None:
        bus = dcc_mcp_core.EventBus()
        received = []
        bus.subscribe("evt", lambda **kw: received.append(kw))
        bus.publish("evt", a=1, b="x", c=True)
        assert received[0] == {"a": 1, "b": "x", "c": True}

    def test_publish_no_subscribers_no_error(self) -> None:
        bus = dcc_mcp_core.EventBus()
        bus.publish("nonexistent_event", x=1)

    def test_unsubscribe_returns_true(self) -> None:
        bus = dcc_mcp_core.EventBus()
        sub_id = bus.subscribe("evt", lambda **kw: None)
        removed = bus.unsubscribe("evt", sub_id)
        assert removed is True

    def test_unsubscribe_nonexistent_returns_false(self) -> None:
        bus = dcc_mcp_core.EventBus()
        removed = bus.unsubscribe("evt", 99999)
        assert removed is False


class TestEventBusMultiSubscriber:
    """EventBus with multiple subscribers."""

    def test_two_subscribers_both_called(self) -> None:
        bus = dcc_mcp_core.EventBus()
        calls = []
        bus.subscribe("evt", lambda **kw: calls.append(("s1", kw)))
        bus.subscribe("evt", lambda **kw: calls.append(("s2", kw)))
        bus.publish("evt", x=1)
        assert len(calls) == 2
        names = {c[0] for c in calls}
        assert names == {"s1", "s2"}

    def test_three_subscribers_all_called(self) -> None:
        bus = dcc_mcp_core.EventBus()
        calls = []

        def make_sub(n):
            return lambda **kw: calls.append(n)

        for i in range(3):
            bus.subscribe("evt", make_sub(i))
        bus.publish("evt")
        assert len(calls) == 3

    def test_unsubscribe_one_other_still_called(self) -> None:
        bus = dcc_mcp_core.EventBus()
        calls = []
        sub1 = bus.subscribe("evt", lambda **kw: calls.append("sub1"))
        bus.subscribe("evt", lambda **kw: calls.append("sub2"))
        bus.unsubscribe("evt", sub1)
        bus.publish("evt")
        assert "sub1" not in calls
        assert "sub2" in calls

    def test_different_events_isolated(self) -> None:
        bus = dcc_mcp_core.EventBus()
        calls_a = []
        calls_b = []
        bus.subscribe("event_a", lambda **kw: calls_a.append(kw))
        bus.subscribe("event_b", lambda **kw: calls_b.append(kw))
        bus.publish("event_a", x=1)
        assert len(calls_a) == 1
        assert len(calls_b) == 0
        bus.publish("event_b", y=2)
        assert len(calls_a) == 1
        assert len(calls_b) == 1

    def test_repr_reflects_subscription_count(self) -> None:
        bus = dcc_mcp_core.EventBus()
        bus.subscribe("evt", lambda **kw: None)
        bus.subscribe("evt", lambda **kw: None)
        r = repr(bus)
        assert "2" in r

    def test_multiple_publishes_accumulate(self) -> None:
        bus = dcc_mcp_core.EventBus()
        calls = []
        bus.subscribe("evt", lambda **kw: calls.append(kw.get("n")))
        for i in range(5):
            bus.publish("evt", n=i)
        assert calls == [0, 1, 2, 3, 4]

    def test_subscriber_receives_correct_data_after_partial_unsubscribe(self) -> None:
        bus = dcc_mcp_core.EventBus()
        data = []
        sub1 = bus.subscribe("evt", lambda **kw: data.append(("s1", kw.get("v"))))
        bus.subscribe("evt", lambda **kw: data.append(("s2", kw.get("v"))))
        bus.publish("evt", v=10)
        bus.unsubscribe("evt", sub1)
        bus.publish("evt", v=20)
        # s1 only sees v=10, s2 sees both
        s1_values = [x[1] for x in data if x[0] == "s1"]
        s2_values = [x[1] for x in data if x[0] == "s2"]
        assert s1_values == [10]
        assert s2_values == [10, 20]

    def test_thread_safe_subscribe_publish(self) -> None:
        bus = dcc_mcp_core.EventBus()
        received = []
        lock = threading.Lock()

        def handler(**kw):
            with lock:
                received.append(kw.get("n"))

        bus.subscribe("thread_evt", handler)

        def worker(n: int):
            bus.publish("thread_evt", n=n)

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(received) == 10
        assert sorted(received) == list(range(10))


# ---------------------------------------------------------------------------
# VersionedRegistry (full method coverage)
# ---------------------------------------------------------------------------


class TestVersionedRegistryCreate:
    """VersionedRegistry construction and basic registration."""

    def test_create_empty(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        assert vr.total_entries() == 0

    def test_register_versioned(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        assert vr.total_entries() == 1

    def test_register_multiple_versions(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")
        assert vr.total_entries() == 2

    def test_register_different_actions(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("delete_mesh", "maya", "1.0.0")
        assert vr.total_entries() == 2

    def test_register_different_dccs(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "blender", "1.0.0")
        assert vr.total_entries() == 2


class TestVersionedRegistryVersions:
    """.versions() and .latest_version()."""

    def test_versions_sorted(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "2.0.0")
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        v = vr.versions("act", "maya")
        assert v == ["1.0.0", "1.5.0", "2.0.0"]

    def test_versions_single(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "3.0.0")
        assert vr.versions("act", "maya") == ["3.0.0"]

    def test_versions_empty_if_not_registered(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        assert vr.versions("nonexistent", "maya") == []

    def test_latest_version(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        assert vr.latest_version("act", "maya") == "2.0.0"

    def test_latest_version_none_if_empty(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        assert vr.latest_version("nonexistent", "maya") is None


class TestVersionedRegistryKeys:
    """.keys() returns unique (name, dcc) pairs."""

    def test_keys_single(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        assert ("create_sphere", "maya") in vr.keys()

    def test_keys_multiple_versions_dedup(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")
        keys = vr.keys()
        assert len(keys) == 1
        assert ("create_sphere", "maya") in keys

    def test_keys_multiple_actions(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("delete_mesh", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "blender", "1.0.0")
        keys = vr.keys()
        assert len(keys) == 3


class TestVersionedRegistryResolve:
    """.resolve() returns best-matching version dict."""

    def test_resolve_caret_returns_highest(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        result = vr.resolve("act", "maya", "^1.0.0")
        assert result["version"] == "1.5.0"

    def test_resolve_wildcard_returns_latest(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "3.0.0")
        result = vr.resolve("act", "maya", "*")
        assert result["version"] == "3.0.0"

    def test_resolve_exact_ge(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        result = vr.resolve("act", "maya", ">=2.0.0")
        assert result["version"] == "2.0.0"

    def test_resolve_has_name_key(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("my_action", "blender", "1.0.0")
        result = vr.resolve("my_action", "blender", "*")
        assert result["name"] == "my_action"

    def test_resolve_has_dcc_key(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("my_action", "blender", "1.0.0")
        result = vr.resolve("my_action", "blender", "*")
        assert result["dcc"] == "blender"

    def test_resolve_none_if_no_match(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        result = vr.resolve("act", "maya", ">=9.0.0")
        assert result is None

    def test_resolve_description_preserved(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0", description="my desc")
        result = vr.resolve("act", "maya", "1.0.0")
        assert result["description"] == "my desc"


class TestVersionedRegistryResolveAll:
    """.resolve_all() returns list of all matching entries."""

    def test_resolve_all_caret_returns_matching(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        results = vr.resolve_all("act", "maya", "^1.0.0")
        versions = [r["version"] for r in results]
        assert "1.0.0" in versions
        assert "1.5.0" in versions
        assert "2.0.0" not in versions

    def test_resolve_all_wildcard_returns_all(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        vr.register_versioned("act", "maya", "3.0.0")
        results = vr.resolve_all("act", "maya", "*")
        assert len(results) == 3

    def test_resolve_all_empty_if_no_match(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        results = vr.resolve_all("act", "maya", ">=5.0.0")
        assert results == []

    def test_resolve_all_sorted_ascending(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "3.0.0")
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        results = vr.resolve_all("act", "maya", "*")
        versions = [r["version"] for r in results]
        assert versions == sorted(versions)


class TestVersionedRegistryRemove:
    """.remove() returns count of removed versions."""

    def test_remove_caret_returns_count(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        removed = vr.remove("act", "maya", "^1.0.0")
        assert removed == 2

    def test_remove_leaves_remaining(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        vr.remove("act", "maya", "^1.0.0")
        assert vr.versions("act", "maya") == ["2.0.0"]

    def test_remove_wildcard_removes_all(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        removed = vr.remove("act", "maya", "*")
        assert removed == 2
        assert vr.versions("act", "maya") == []

    def test_remove_zero_if_no_match(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        removed = vr.remove("act", "maya", ">=5.0.0")
        assert removed == 0

    def test_total_entries_decreases_after_remove(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        vr.remove("act", "maya", "1.0.0")
        assert vr.total_entries() == 1


# ---------------------------------------------------------------------------
# TelemetryConfig builder
# ---------------------------------------------------------------------------


class TestTelemetryConfigCreate:
    """TelemetryConfig construction and builder methods."""

    def test_service_name(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("my-service")
        assert cfg.service_name == "my-service"

    def test_enable_metrics_default_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_metrics is True

    def test_enable_tracing_default_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_tracing is True

    def test_with_noop_exporter_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_noop_exporter()
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_with_stdout_exporter_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_stdout_exporter()
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_with_attribute_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_attribute("dcc.type", "maya")
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_with_service_version_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_service_version("1.0.0")
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_set_enable_metrics_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.set_enable_metrics(False)
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_set_enable_tracing_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.set_enable_tracing(False)
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_with_json_logs_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_json_logs()
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_with_text_logs_returns_self_type(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg2 = cfg.with_text_logs()
        assert isinstance(cfg2, dcc_mcp_core.TelemetryConfig)

    def test_builder_chain(self) -> None:
        cfg = (
            dcc_mcp_core.TelemetryConfig("maya-server")
            .with_noop_exporter()
            .with_attribute("dcc.type", "maya")
            .with_service_version("0.1.0")
            .set_enable_metrics(True)
            .set_enable_tracing(True)
        )
        assert cfg.service_name == "maya-server"

    def test_is_telemetry_initialized_returns_bool(self) -> None:
        result = dcc_mcp_core.is_telemetry_initialized()
        assert isinstance(result, bool)


# ---------------------------------------------------------------------------
# ToolRecorder + ToolMetrics + RecordingGuard
# ---------------------------------------------------------------------------


class TestActionRecorderBasic:
    """ToolRecorder basic guard usage."""

    def test_start_returns_guard(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        guard = rec.start("my_action", "maya")
        assert guard is not None
        guard.finish(success=True)

    def test_metrics_none_before_first_call(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        assert rec.metrics("nonexistent") is None

    def test_metrics_available_after_finish(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        guard = rec.start("act", "maya")
        guard.finish(success=True)
        assert rec.metrics("act") is not None

    def test_invocation_count_increments(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(3):
            guard = rec.start("act", "maya")
            guard.finish(success=True)
        assert rec.metrics("act").invocation_count == 3

    def test_success_count(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(2):
            g = rec.start("act", "maya")
            g.finish(success=True)
        g = rec.start("act", "maya")
        g.finish(success=False)
        m = rec.metrics("act")
        assert m.success_count == 2
        assert m.failure_count == 1

    def test_failure_count(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(3):
            g = rec.start("act", "maya")
            g.finish(success=False)
        m = rec.metrics("act")
        assert m.failure_count == 3

    def test_reset_clears_metrics(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("act", "maya")
        g.finish(success=True)
        rec.reset()
        assert rec.metrics("act") is None

    def test_all_metrics_empty_initially(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("fresh")
        assert rec.all_metrics() == []

    def test_all_metrics_contains_recorded(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("action_a", "maya")
        g.finish(success=True)
        all_m = rec.all_metrics()
        assert len(all_m) == 1
        assert all_m[0].action_name == "action_a"

    def test_all_metrics_multiple_actions(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for name in ("a1", "a2", "a3"):
            g = rec.start(name, "maya")
            g.finish(success=True)
        all_m = rec.all_metrics()
        assert len(all_m) == 3


class TestActionMetrics:
    """ToolMetrics field types and values."""

    def test_action_name_str(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("sphere_op", "maya")
        g.finish(success=True)
        assert isinstance(rec.metrics("sphere_op").action_name, str)

    def test_invocation_count_int(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("act", "maya")
        g.finish(success=True)
        assert isinstance(rec.metrics("act").invocation_count, int)

    def test_success_count_int(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("act", "maya")
        g.finish(success=True)
        assert isinstance(rec.metrics("act").success_count, int)

    def test_failure_count_int(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("act", "maya")
        g.finish(success=False)
        assert isinstance(rec.metrics("act").failure_count, int)

    def test_avg_duration_ms_float(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        g = rec.start("act", "maya")
        time.sleep(0.001)
        g.finish(success=True)
        m = rec.metrics("act")
        assert isinstance(m.avg_duration_ms, float)
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_float(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(5):
            g = rec.start("act", "maya")
            g.finish(success=True)
        m = rec.metrics("act")
        assert isinstance(m.p95_duration_ms, float)

    def test_p99_duration_ms_float(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(5):
            g = rec.start("act", "maya")
            g.finish(success=True)
        m = rec.metrics("act")
        assert isinstance(m.p99_duration_ms, float)

    def test_success_rate_all_success(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(4):
            g = rec.start("act", "maya")
            g.finish(success=True)
        assert rec.metrics("act").success_rate() == 1.0

    def test_success_rate_all_failure(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for _ in range(3):
            g = rec.start("act", "maya")
            g.finish(success=False)
        assert rec.metrics("act").success_rate() == 0.0

    def test_success_rate_partial(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for i in range(4):
            g = rec.start("act", "maya")
            g.finish(success=(i % 2 == 0))
        m = rec.metrics("act")
        assert 0.0 < m.success_rate() < 1.0

    def test_success_rate_range(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        for i in range(10):
            g = rec.start("act", "maya")
            g.finish(success=(i < 7))
        sr = rec.metrics("act").success_rate()
        assert 0.0 <= sr <= 1.0


class TestRecordingGuardContextManager:
    """RecordingGuard as context manager."""

    def test_context_manager_success(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        with rec.start("act", "maya"):
            pass
        m = rec.metrics("act")
        assert m is not None
        assert m.invocation_count == 1

    def test_context_manager_exception_marks_failure(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        with pytest.raises(ValueError), rec.start("act", "maya"):
            raise ValueError("test error")
        m = rec.metrics("act")
        assert m is not None
        assert m.failure_count == 1

    def test_context_manager_no_exception_marks_success(self) -> None:
        rec = dcc_mcp_core.ToolRecorder("svc")
        with rec.start("act", "maya"):
            pass  # no exception
        assert rec.metrics("act").success_count == 1
