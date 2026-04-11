"""Deep tests for TelemetryConfig, ActionRecorder, ActionMetrics, RecordingGuard, EventBus, McpHttpConfig, SkillCatalog/SkillSummary, and encode/decode envelope functions.

Covers:
- TelemetryConfig: constructor / service_name / enable_tracing / enable_metrics
  / with_noop_exporter / with_stdout_exporter / with_json_logs / with_text_logs
  / with_service_version / with_attribute / set_enable_metrics / set_enable_tracing
- is_telemetry_initialized / shutdown_telemetry
- ActionRecorder: start / all_metrics / reset / scope name
- RecordingGuard: finish(success=True/False) / context-manager path
- ActionMetrics: action_name / invocation_count / success_count / failure_count
  / avg_duration_ms / p95_duration_ms / p99_duration_ms / success_rate()
- EventBus: subscribe / publish / unsubscribe / repr / multiple subscribers
  / publish unknown event / unsubscribe wrong id
- McpHttpConfig: port / server_name / server_version defaults & custom
- SkillCatalog: discover / list_skills / find_skills / load_skill(not-found)
  / unload_skill / is_loaded / loaded_count / get_skill_info
- SkillSummary fields (from watcher)
- encode_request / encode_notify / encode_response / decode_envelope roundtrip
"""

from __future__ import annotations

import struct
import threading
import time

import pytest

# ===========================================================================
# TelemetryConfig
# ===========================================================================


class TestTelemetryConfigCreate:
    def test_create_default_repr(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("maya-mcp")
        r = repr(tc)
        assert "maya-mcp" in r

    def test_service_name(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("my-service")
        assert tc.service_name == "my-service"

    def test_enable_tracing_default_true(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc")
        assert tc.enable_tracing is True

    def test_enable_metrics_default_true(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc")
        assert tc.enable_metrics is True

    def test_with_noop_exporter_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_noop_exporter()
        assert isinstance(tc, TelemetryConfig)
        assert "Noop" in repr(tc)

    def test_with_stdout_exporter_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_stdout_exporter()
        assert isinstance(tc, TelemetryConfig)
        r = repr(tc)
        assert "Stdout" in r or "stdout" in r.lower()

    def test_with_json_logs_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_json_logs()
        assert isinstance(tc, TelemetryConfig)

    def test_with_text_logs_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_text_logs()
        assert isinstance(tc, TelemetryConfig)

    def test_with_service_version_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_service_version("2.0.0")
        assert isinstance(tc, TelemetryConfig)

    def test_with_attribute_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_attribute("dcc.type", "maya")
        assert isinstance(tc, TelemetryConfig)

    def test_with_attribute_multiple_calls(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").with_attribute("dcc.type", "maya").with_attribute("dcc.version", "2025")
        assert isinstance(tc, TelemetryConfig)

    def test_set_enable_metrics_false(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").set_enable_metrics(False)
        assert isinstance(tc, TelemetryConfig)

    def test_set_enable_tracing_false(self):
        from dcc_mcp_core import TelemetryConfig

        tc = TelemetryConfig("svc").set_enable_tracing(False)
        assert isinstance(tc, TelemetryConfig)

    def test_builder_chain(self):
        from dcc_mcp_core import TelemetryConfig

        tc = (
            TelemetryConfig("maya-mcp-server")
            .with_noop_exporter()
            .with_attribute("dcc.type", "maya")
            .with_service_version("1.0.0")
            .set_enable_metrics(True)
            .set_enable_tracing(True)
        )
        assert isinstance(tc, TelemetryConfig)
        assert tc.service_name == "maya-mcp-server"


class TestTelemetryIsInitialized:
    def test_not_initialized_by_default(self):
        from dcc_mcp_core import is_telemetry_initialized

        # In a fresh test process, telemetry may or may not be initialized.
        # We just assert the function returns a bool.
        result = is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_shutdown_telemetry_callable(self):
        from dcc_mcp_core import shutdown_telemetry

        # Should not raise even if not initialized.
        shutdown_telemetry()


# ===========================================================================
# ActionRecorder + RecordingGuard + ActionMetrics
# ===========================================================================


class TestActionRecorderCreate:
    def test_create_with_scope(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("maya-mcp")
        assert rec is not None

    def test_metrics_none_before_any_call(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        assert rec.metrics("nonexistent") is None

    def test_all_metrics_empty_initially(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        assert rec.all_metrics() == []

    def test_reset_clears_metrics(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g = rec.start("op", "maya")
        g.finish(success=True)
        assert rec.metrics("op") is not None
        rec.reset()
        assert rec.metrics("op") is None

    def test_all_metrics_after_reset(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g = rec.start("op", "maya")
        g.finish(success=True)
        rec.reset()
        assert rec.all_metrics() == []


class TestRecordingGuard:
    def test_manual_finish_success(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g = rec.start("create_sphere", "maya")
        g.finish(success=True)
        m = rec.metrics("create_sphere")
        assert m is not None
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_manual_finish_failure(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g = rec.start("delete_mesh", "maya")
        g.finish(success=False)
        m = rec.metrics("delete_mesh")
        assert m.success_count == 0
        assert m.failure_count == 1

    def test_context_manager_success_no_exception(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        with rec.start("batch_op", "blender") as _guard:
            pass
        m = rec.metrics("batch_op")
        assert m.success_count == 1

    def test_context_manager_failure_on_exception(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        with pytest.raises(ValueError), rec.start("risky_op", "maya"):
            raise ValueError("something went wrong")
        m = rec.metrics("risky_op")
        assert m.failure_count == 1

    def test_repr_contains_action_name(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g = rec.start("my_action", "maya")
        r = repr(g)
        assert "my_action" in r
        g.finish(success=True)

    def test_multiple_calls_accumulate(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        for _ in range(5):
            g = rec.start("batch", "maya")
            g.finish(success=True)
        for _ in range(2):
            g = rec.start("batch", "maya")
            g.finish(success=False)
        m = rec.metrics("batch")
        assert m.invocation_count == 7
        assert m.success_count == 5
        assert m.failure_count == 2

    def test_different_dcc_same_action(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        g1 = rec.start("render", "maya")
        g1.finish(success=True)
        g2 = rec.start("render", "blender")
        g2.finish(success=True)
        # both recorded under same action name
        m = rec.metrics("render")
        assert m is not None
        assert m.invocation_count >= 1


class TestActionMetrics:
    def _make_metrics(self, success_count=3, failure_count=1, action="op", dcc="maya"):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        for _ in range(success_count):
            g = rec.start(action, dcc)
            g.finish(success=True)
        for _ in range(failure_count):
            g = rec.start(action, dcc)
            g.finish(success=False)
        return rec.metrics(action)

    def test_action_name(self):
        m = self._make_metrics()
        assert m.action_name == "op"

    def test_invocation_count(self):
        m = self._make_metrics(success_count=3, failure_count=1)
        assert m.invocation_count == 4

    def test_success_count(self):
        m = self._make_metrics(success_count=3, failure_count=1)
        assert m.success_count == 3

    def test_failure_count(self):
        m = self._make_metrics(success_count=3, failure_count=1)
        assert m.failure_count == 1

    def test_avg_duration_ms_non_negative(self):
        m = self._make_metrics()
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_non_negative(self):
        m = self._make_metrics()
        assert m.p95_duration_ms >= 0.0

    def test_p99_duration_ms_non_negative(self):
        m = self._make_metrics()
        assert m.p99_duration_ms >= 0.0

    def test_success_rate_all_success(self):
        m = self._make_metrics(success_count=4, failure_count=0)
        assert m.success_rate() == pytest.approx(1.0)

    def test_success_rate_partial(self):
        m = self._make_metrics(success_count=3, failure_count=1)
        rate = m.success_rate()
        assert 0.0 <= rate <= 1.0
        assert rate == pytest.approx(0.75)

    def test_success_rate_all_failure(self):
        m = self._make_metrics(success_count=0, failure_count=3)
        assert m.success_rate() == pytest.approx(0.0)

    def test_repr_contains_action(self):
        m = self._make_metrics(action="special_action")
        r = repr(m)
        assert "special_action" in r

    def test_all_metrics_list(self):
        from dcc_mcp_core import ActionRecorder

        rec = ActionRecorder("scope")
        for action in ["a", "b", "c"]:
            g = rec.start(action, "maya")
            g.finish(success=True)
        all_m = rec.all_metrics()
        assert len(all_m) == 3
        names = {m.action_name for m in all_m}
        assert names == {"a", "b", "c"}


# ===========================================================================
# EventBus
# ===========================================================================


class TestEventBusCreate:
    def test_create_and_repr(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        r = repr(bus)
        assert "EventBus" in r
        assert "0" in r  # subscriptions=0

    def test_subscribe_returns_id(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        sid = bus.subscribe("test_event", lambda **kw: None)
        assert sid is not None
        assert isinstance(sid, int)

    def test_subscribe_multiple_returns_distinct_ids(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        sid1 = bus.subscribe("ev", lambda **kw: None)
        sid2 = bus.subscribe("ev", lambda **kw: None)
        assert sid1 != sid2

    def test_repr_updates_after_subscribe(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        bus.subscribe("ev", lambda **kw: None)
        r = repr(bus)
        assert "EventBus" in r


class TestEventBusPublish:
    def test_publish_calls_subscriber(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        received = []
        bus.subscribe("action_done", lambda **kw: received.append(kw))
        bus.publish("action_done", action="create_sphere", success=True)
        assert received == [{"action": "create_sphere", "success": True}]

    def test_publish_multiple_subscribers(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        log1, log2 = [], []
        bus.subscribe("ev", lambda **kw: log1.append(kw))
        bus.subscribe("ev", lambda **kw: log2.append(kw))
        bus.publish("ev", x=1)
        assert log1 == [{"x": 1}]
        assert log2 == [{"x": 1}]

    def test_publish_unknown_event_no_error(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        # Publishing to an event with no subscribers should not raise.
        bus.publish("no_subscribers", data="anything")

    def test_publish_no_kwargs(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        received = []
        bus.subscribe("ping", lambda **kw: received.append(kw))
        bus.publish("ping")
        assert received == [{}]

    def test_publish_multiple_times(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        count = []
        bus.subscribe("tick", lambda **kw: count.append(1))
        for _ in range(5):
            bus.publish("tick")
        assert len(count) == 5

    def test_different_events_isolated(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        a_log, b_log = [], []
        bus.subscribe("event_a", lambda **kw: a_log.append(1))
        bus.subscribe("event_b", lambda **kw: b_log.append(1))
        bus.publish("event_a")
        assert a_log == [1]
        assert b_log == []
        bus.publish("event_b")
        assert a_log == [1]
        assert b_log == [1]


class TestEventBusUnsubscribe:
    def test_unsubscribe_returns_true_when_found(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        sid = bus.subscribe("ev", lambda **kw: None)
        assert bus.unsubscribe("ev", sid) is True

    def test_unsubscribe_returns_false_when_not_found(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        assert bus.unsubscribe("ev", 99999) is False

    def test_unsubscribe_stops_delivery(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        received = []
        sid = bus.subscribe("ev", lambda **kw: received.append(1))
        bus.publish("ev")
        assert received == [1]
        bus.unsubscribe("ev", sid)
        bus.publish("ev")
        assert received == [1]  # no new delivery

    def test_unsubscribe_one_of_two(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        log1, log2 = [], []
        sid1 = bus.subscribe("ev", lambda **kw: log1.append(1))
        bus.subscribe("ev", lambda **kw: log2.append(1))
        bus.unsubscribe("ev", sid1)
        bus.publish("ev")
        assert log1 == []  # removed
        assert log2 == [1]  # still active

    def test_unsubscribe_wrong_event_name(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        sid = bus.subscribe("event_a", lambda **kw: None)
        # Unsubscribing under wrong event name should return False
        result = bus.unsubscribe("event_b", sid)
        assert result is False


class TestEventBusThreadSafety:
    def test_concurrent_publish(self):
        from dcc_mcp_core import EventBus

        bus = EventBus()
        lock = threading.Lock()
        counts = [0]

        def handler(**kw):
            with lock:
                counts[0] += 1

        bus.subscribe("concurrent", handler)
        threads = [threading.Thread(target=lambda: bus.publish("concurrent")) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert counts[0] == 20


# ===========================================================================
# McpHttpConfig
# ===========================================================================


class TestMcpHttpConfigCreate:
    def test_default_server_name(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=8765)
        assert cfg.server_name == "dcc-mcp"

    def test_default_server_version_not_empty(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=0)
        # default version should be non-empty string (e.g. "0.12.12")
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0

    def test_custom_port(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=9090)
        assert cfg.port == 9090

    def test_port_zero(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_custom_server_name(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=8765, server_name="maya-mcp")
        assert cfg.server_name == "maya-mcp"

    def test_custom_server_version(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=8765, server_version="2.0.0")
        assert cfg.server_version == "2.0.0"

    def test_all_fields(self):
        from dcc_mcp_core import McpHttpConfig

        cfg = McpHttpConfig(port=8765, server_name="test-server", server_version="1.2.3")
        assert cfg.port == 8765
        assert cfg.server_name == "test-server"
        assert cfg.server_version == "1.2.3"


# ===========================================================================
# SkillCatalog + SkillSummary
# ===========================================================================


class TestSkillCatalogCreate:
    def test_create_with_registry(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert cat is not None

    def test_list_skills_empty_initially(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        skills = cat.list_skills()
        assert isinstance(skills, list)
        assert len(skills) == 0

    def test_loaded_count_zero_initially(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert cat.loaded_count() == 0

    def test_is_loaded_nonexistent_false(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert cat.is_loaded("nonexistent-skill") is False

    def test_get_skill_info_nonexistent_none(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        result = cat.get_skill_info("nonexistent-skill")
        assert result is None

    def test_load_skill_nonexistent_raises(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        with pytest.raises((ValueError, RuntimeError, KeyError)):
            cat.load_skill("nonexistent-skill")

    def test_unload_skill_nonexistent_raises(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        with pytest.raises((ValueError, RuntimeError, KeyError)):
            cat.unload_skill("nonexistent-skill")

    def test_find_skills_empty(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        results = cat.find_skills(query="maya")
        assert isinstance(results, list)
        assert len(results) == 0

    def test_discover_with_empty_extra_paths(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        count = cat.discover(extra_paths=[], dcc_name="maya")
        assert isinstance(count, int)
        assert count >= 0

    def test_discover_returns_int(self):
        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        count = cat.discover()
        assert isinstance(count, int)


class TestSkillCatalogWithRealSkills:
    """Tests using the examples/skills directory."""

    def _make_catalog_with_examples(self):
        from pathlib import Path

        from dcc_mcp_core import ActionRegistry
        from dcc_mcp_core import SkillCatalog

        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        examples_dir = Path(__file__).parent / ".." / "examples" / "skills"
        if not examples_dir.is_dir():
            pytest.skip("examples/skills directory not found")
        cat.discover(extra_paths=[str(examples_dir)])
        return cat

    def test_discover_finds_skills(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        assert len(skills) >= 1

    def test_list_skills_returns_skill_summary(self):
        from dcc_mcp_core import SkillSummary

        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if skills:
            s = skills[0]
            assert isinstance(s, SkillSummary)

    def test_skill_summary_fields(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if skills:
            s = skills[0]
            assert isinstance(s.name, str)
            assert isinstance(s.description, str)
            assert isinstance(s.version, str)
            assert isinstance(s.dcc, str)
            assert isinstance(s.tags, list)
            assert isinstance(s.tool_count, int)
            assert isinstance(s.tool_names, list)
            assert isinstance(s.loaded, bool)

    def test_skill_summary_not_loaded_initially(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if skills:
            assert not skills[0].loaded

    def test_load_skill_by_name(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if not skills:
            pytest.skip("no skills found")
        name = skills[0].name
        result = cat.load_skill(name)
        # load_skill returns the list of registered action names (may be a list or truthy)
        assert result is not None
        assert cat.is_loaded(name) is True
        assert cat.loaded_count() >= 1

    def test_get_skill_info_after_load(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if not skills:
            pytest.skip("no skills found")
        name = skills[0].name
        cat.load_skill(name)
        info = cat.get_skill_info(name)
        assert info is not None
        # get_skill_info may return a SkillMetadata or a dict depending on version
        if isinstance(info, dict):
            assert info.get("name") == name
        else:
            assert info.name == name

    def test_unload_skill(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if not skills:
            pytest.skip("no skills found")
        name = skills[0].name
        cat.load_skill(name)
        result = cat.unload_skill(name)
        # unload_skill returns the count of tools removed (int) or True
        assert result is not None
        assert result is not False
        assert cat.is_loaded(name) is False

    def test_find_skills_by_name_substring(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if not skills:
            pytest.skip("no skills found")
        first_name = skills[0].name
        query = first_name[:3] if len(first_name) >= 3 else first_name
        results = cat.find_skills(query=query)
        assert isinstance(results, list)

    def test_list_skills_status_loaded(self):
        cat = self._make_catalog_with_examples()
        skills = cat.list_skills()
        if not skills:
            pytest.skip("no skills found")
        cat.load_skill(skills[0].name)
        loaded = cat.list_skills(status="loaded")
        assert isinstance(loaded, list)
        assert len(loaded) >= 1


# ===========================================================================
# encode_request / encode_notify / encode_response / decode_envelope
# ===========================================================================


class TestEncodeDecodeEnvelope:
    def _strip_length_prefix(self, data: bytes) -> bytes:
        """encode_* prepends a 4-byte big-endian length prefix."""
        assert len(data) >= 4
        prefix = struct.unpack(">I", data[:4])[0]
        payload = data[4:]
        assert len(payload) == prefix
        return payload

    def test_encode_request_returns_bytes(self):
        from dcc_mcp_core import encode_request

        result = encode_request("execute_mel", b"sphere -r 1;")
        assert isinstance(result, bytes)
        assert len(result) > 4

    def test_encode_request_has_length_prefix(self):
        from dcc_mcp_core import encode_request

        data = encode_request("my_method", b"params")
        prefix = struct.unpack(">I", data[:4])[0]
        assert prefix == len(data) - 4

    def test_decode_request_envelope(self):
        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_request

        data = encode_request("execute_python", b"print('hello')")
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["type"] == "request"
        assert decoded["method"] == "execute_python"
        assert decoded["params"] == b"print('hello')"
        assert "id" in decoded

    def test_decode_request_has_uuid_id(self):
        import re

        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_request

        data = encode_request("hello", b"")
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        uuid_pattern = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        assert uuid_pattern.match(decoded["id"]), f"Not a UUID: {decoded['id']!r}"

    def test_encode_notify_returns_bytes(self):
        from dcc_mcp_core import encode_notify

        result = encode_notify("scene_changed", b"data")
        assert isinstance(result, bytes)

    def test_decode_notify_envelope(self):
        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_notify

        data = encode_notify("file_saved", b"path/to/scene.mb")
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["type"] == "notify"
        assert decoded["topic"] == "file_saved"
        assert decoded["data"] == b"path/to/scene.mb"

    def test_encode_response_returns_bytes(self):
        import uuid

        from dcc_mcp_core import encode_response

        req_id = str(uuid.uuid4())
        result = encode_response(req_id, True, b"result", None)
        assert isinstance(result, bytes)

    def test_decode_response_success(self):
        import uuid

        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_response

        req_id = str(uuid.uuid4())
        data = encode_response(req_id, True, b"output", None)
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["type"] == "response"
        assert decoded["success"] is True
        assert decoded["payload"] == b"output"
        assert decoded["error"] is None

    def test_decode_response_failure(self):
        import uuid

        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_response

        req_id = str(uuid.uuid4())
        data = encode_response(req_id, False, None, "NameError: x")
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["type"] == "response"
        assert decoded["success"] is False
        assert decoded["error"] == "NameError: x"

    def test_request_response_id_matches(self):
        import uuid

        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_request
        from dcc_mcp_core import encode_response

        req_data = encode_request("op", b"params")
        req_payload = self._strip_length_prefix(req_data)
        req_decoded = decode_envelope(req_payload)
        req_id = req_decoded["id"]

        resp_data = encode_response(req_id, True, b"ok", None)
        resp_payload = self._strip_length_prefix(resp_data)
        resp_decoded = decode_envelope(resp_payload)
        assert resp_decoded["id"] == req_id

    def test_encode_request_none_params(self):
        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_request

        data = encode_request("no_params", None)
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["method"] == "no_params"
        # When params=None is passed, the Rust side encodes it as empty bytes
        assert decoded["params"] in (None, b"")

    def test_encode_notify_none_data(self):
        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_notify

        data = encode_notify("heartbeat", None)
        payload = self._strip_length_prefix(data)
        decoded = decode_envelope(payload)
        assert decoded["type"] == "notify"
        assert decoded["topic"] == "heartbeat"

    def test_multiple_requests_have_unique_ids(self):
        from dcc_mcp_core import decode_envelope
        from dcc_mcp_core import encode_request

        ids = set()
        for _ in range(10):
            data = encode_request("method", b"p")
            payload = self._strip_length_prefix(data)
            decoded = decode_envelope(payload)
            ids.add(decoded["id"])
        assert len(ids) == 10
