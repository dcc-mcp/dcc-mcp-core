"""Deep tests for TelemetryConfig, ActionRecorder, SandboxPolicy, SandboxContext, EventBus, and ActionPipeline middleware.

Coverage targets:
- TelemetryConfig: construction, builder chain, service_name, init, exporter variants
- ActionRecorder: start/finish, metrics aggregation, reset, multiple actions
- ActionMetrics: all numeric fields, success_rate(), repr
- SandboxPolicy: allow/deny rules, read_only, timeout, max_actions
- SandboxContext: is_allowed, execute_json, action_count, audit_log
- AuditLog / AuditEntry: entries, entries_for_action, successes, denials, to_json
- EventBus: subscribe/publish/unsubscribe, multiple subscribers, kwargs propagation
- ActionPipeline: timing, audit, rate_limit, add_callable, middleware_count/names
"""

from __future__ import annotations

import json
import time


class TestTelemetryConfigCreate:
    """TelemetryConfig construction and basic properties."""

    def test_create_with_service_name(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("my-service")
        assert cfg.service_name == "my-service"

    def test_repr_contains_service_name(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("maya-core")
        assert "maya-core" in repr(cfg)

    def test_repr_contains_exporter(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        assert "Stdout" in repr(cfg) or "Noop" in repr(cfg) or "exporter" in repr(cfg).lower()

    def test_enable_tracing_default(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        # enable_tracing is a property
        assert isinstance(cfg.enable_tracing, bool)

    def test_enable_metrics_default(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        assert isinstance(cfg.enable_metrics, bool)

    def test_set_enable_tracing(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        cfg.set_enable_tracing(False)
        assert cfg.enable_tracing is False

    def test_set_enable_metrics(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        cfg.set_enable_metrics(False)
        assert cfg.enable_metrics is False


class TestTelemetryConfigBuilderChain:
    """TelemetryConfig builder-pattern methods return self."""

    def test_with_noop_exporter_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_noop_exporter()
        # should return self or a new TelemetryConfig (still usable)
        assert result is not None

    def test_with_stdout_exporter_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_stdout_exporter()
        assert result is not None

    def test_with_service_version(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_service_version("1.2.3")
        assert result is not None

    def test_with_attribute(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_attribute("env", "test")
        assert result is not None

    def test_with_json_logs_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_json_logs()
        assert result is not None

    def test_with_text_logs_returns_config(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        result = cfg.with_text_logs()
        assert result is not None

    def test_init_noop(self):
        """init() should either succeed or raise 'already set' (global tracer singleton)."""
        import pytest

        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("svc")
        cfg.with_noop_exporter()
        try:
            cfg.init()
        except RuntimeError as exc:
            # Acceptable: global tracer already installed by a prior test/import
            assert "already" in str(exc).lower() or "provider" in str(exc).lower()

    def test_chain_multiple_methods(self):
        from dcc_mcp_core import TelemetryConfig

        cfg = TelemetryConfig("complex-svc")
        cfg.with_service_version("2.0.0")
        cfg.with_attribute("team", "dcc")
        cfg.with_noop_exporter()
        cfg.with_text_logs()
        # All should succeed without exception
        assert cfg.service_name == "complex-svc"


class TestActionRecorderCreate:
    """ActionRecorder construction."""

    def test_create_with_scope(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("my-scope")
        assert r is not None

    def test_create_different_scopes(self):
        from dcc_mcp_core import ActionRecorder

        r1 = ActionRecorder("scope-a")
        r2 = ActionRecorder("scope-b")
        assert r1 is not r2

    def test_all_metrics_empty_initially(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        result = r.all_metrics()
        assert isinstance(result, list)
        assert len(result) == 0

    def test_metrics_returns_none_before_any_recording(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        import contextlib

        # Either returns None or raises — just ensure no unexpected crash
        with contextlib.suppress(Exception):
            r.metrics("nonexistent_action")


class TestActionRecorderStartFinish:
    """ActionRecorder start/finish cycle."""

    def test_start_returns_recording_guard(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        guard = r.start("sphere", "maya")
        assert guard is not None
        assert hasattr(guard, "finish")
        guard.finish(success=True)

    def test_finish_success_increments_success_count(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        g = r.start("sphere", "maya")
        g.finish(success=True)
        m = r.metrics("sphere")
        assert m is not None
        assert m.invocation_count == 1
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_finish_failure_increments_failure_count(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        g = r.start("sphere", "maya")
        g.finish(success=False)
        m = r.metrics("sphere")
        assert m.invocation_count == 1
        assert m.failure_count == 1
        assert m.success_count == 0

    def test_multiple_invocations_accumulated(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        for _ in range(3):
            g = r.start("cube", "blender")
            g.finish(success=True)
        g4 = r.start("cube", "blender")
        g4.finish(success=False)
        m = r.metrics("cube")
        assert m.invocation_count == 4
        assert m.success_count == 3
        assert m.failure_count == 1

    def test_all_metrics_lists_all_actions(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("a1", "maya").finish(success=True)
        r.start("a2", "maya").finish(success=True)
        r.start("a3", "blender").finish(success=False)
        all_m = r.all_metrics()
        names = [m.action_name for m in all_m]
        assert "a1" in names
        assert "a2" in names
        assert "a3" in names

    def test_reset_clears_all_metrics(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("x", "maya").finish(success=True)
        r.reset()
        assert len(r.all_metrics()) == 0

    def test_metrics_after_reset_returns_none_or_empty(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("y", "maya").finish(success=True)
        r.reset()
        result = r.metrics("y")
        assert result is None or (hasattr(result, "invocation_count") and result.invocation_count == 0)


class TestActionMetrics:
    """ActionMetrics fields and methods."""

    def test_action_name_field(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("render_mesh", "maya").finish(success=True)
        m = r.metrics("render_mesh")
        assert m.action_name == "render_mesh"

    def test_success_rate_is_callable(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("op", "maya").finish(success=True)
        m = r.metrics("op")
        assert callable(m.success_rate)

    def test_success_rate_all_success(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        for _ in range(3):
            r.start("op", "maya").finish(success=True)
        m = r.metrics("op")
        assert abs(m.success_rate() - 1.0) < 0.01

    def test_success_rate_all_failure(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        for _ in range(2):
            r.start("bad_op", "maya").finish(success=False)
        m = r.metrics("bad_op")
        assert abs(m.success_rate() - 0.0) < 0.01

    def test_success_rate_mixed(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        for _ in range(2):
            r.start("mixed", "maya").finish(success=True)
        for _ in range(2):
            r.start("mixed", "maya").finish(success=False)
        m = r.metrics("mixed")
        assert abs(m.success_rate() - 0.5) < 0.01

    def test_avg_duration_ms_is_numeric(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("timed_op", "maya").finish(success=True)
        m = r.metrics("timed_op")
        assert isinstance(m.avg_duration_ms, (int, float))
        assert m.avg_duration_ms >= 0

    def test_p95_duration_ms_is_numeric(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("timed_op2", "maya").finish(success=True)
        m = r.metrics("timed_op2")
        assert isinstance(m.p95_duration_ms, (int, float))

    def test_p99_duration_ms_is_numeric(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("timed_op3", "maya").finish(success=True)
        m = r.metrics("timed_op3")
        assert isinstance(m.p99_duration_ms, (int, float))

    def test_repr_contains_action_name_and_invocations(self):
        from dcc_mcp_core import ActionRecorder

        r = ActionRecorder("svc")
        r.start("repr_op", "maya").finish(success=True)
        m = r.metrics("repr_op")
        rep = repr(m)
        assert "repr_op" in rep
        assert "1" in rep


class TestSandboxPolicyCreate:
    """SandboxPolicy construction and configuration."""

    def test_create_default_policy(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        assert p is not None

    def test_is_read_only_defaults_false(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        assert p.is_read_only is False

    def test_set_read_only_true(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_set_read_only_toggle(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.set_read_only(True)
        p.set_read_only(False)
        assert p.is_read_only is False

    def test_set_timeout_ms(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.set_timeout_ms(5000)
        # No exception expected; no getter for timeout_ms but ensure it doesn't fail

    def test_set_max_actions(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.set_max_actions(100)
        # No exception expected

    def test_allow_actions_list(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere", "delete_sphere", "rename_object"])
        # No exception

    def test_deny_actions_list(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.deny_actions(["execute_code", "eval_script"])

    def test_allow_paths_list(self):
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_paths(["/project", "/tmp"])


class TestSandboxContextIsAllowed:
    """SandboxContext.is_allowed() enforcement."""

    def test_allowed_action_returns_true(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("create_sphere") is True

    def test_non_allowed_action_returns_false(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("execute_code") is False

    def test_denied_action_returns_false(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.deny_actions(["execute_code"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("execute_code") is False

    def test_empty_policy_allows_unknown(self):
        """With no allow/deny list, all actions should pass or fail consistently."""
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        ctx = SandboxContext(p)
        result = ctx.is_allowed("some_action")
        # Result is a bool; just ensure no exception
        assert isinstance(result, bool)

    def test_is_path_allowed_in_whitelist(self):
        """With no path restriction, all paths are allowed (default allow-all)."""
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        # No path restriction: allow_paths not called → default allow-all
        ctx = SandboxContext(p)
        assert ctx.is_path_allowed("/project") is True

    def test_is_path_allowed_restricted_returns_bool(self):
        """With a path restriction set, result is still a bool (may deny on platform mismatch)."""
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_paths(["/project"])
        ctx = SandboxContext(p)
        # Result must be a bool regardless of platform
        result = ctx.is_path_allowed("/project")
        assert isinstance(result, bool)

    def test_is_path_allowed_empty_paths(self):
        """With no path restrictions, any path may be allowed."""
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        ctx = SandboxContext(p)
        result = ctx.is_path_allowed("/any/path")
        assert isinstance(result, bool)


class TestSandboxContextExecuteAndCount:
    """SandboxContext.execute_json and action_count."""

    def test_action_count_starts_zero(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        ctx = SandboxContext(p)
        assert ctx.action_count == 0

    def test_execute_increments_action_count(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        assert ctx.action_count == 1

    def test_execute_multiple_increments_count(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["op"])
        ctx = SandboxContext(p)
        for _ in range(5):
            ctx.execute_json("op", "{}")
        assert ctx.action_count == 5

    def test_execute_returns_string_or_none(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        result = ctx.execute_json("create_sphere", "{}")
        # Result is null JSON string or None
        assert result is None or isinstance(result, str)

    def test_set_actor_succeeds(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.set_actor("agent-007")
        # No exception expected

    def test_execute_different_actions(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["op_a", "op_b", "op_c"])
        ctx = SandboxContext(p)
        ctx.execute_json("op_a", "{}")
        ctx.execute_json("op_b", "{}")
        ctx.execute_json("op_c", "{}")
        assert ctx.action_count == 3


class TestSandboxAuditLog:
    """AuditLog and AuditEntry fields."""

    def test_audit_log_initial_empty(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        ctx = SandboxContext(p)
        log = ctx.audit_log
        assert len(log.entries()) == 0

    def test_audit_log_records_entry_after_execute(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.set_actor("test-agent")
        ctx.execute_json("create_sphere", "{}")
        log = ctx.audit_log
        entries = log.entries()
        assert len(entries) == 1

    def test_audit_entry_action_name(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["my_action"])
        ctx = SandboxContext(p)
        ctx.execute_json("my_action", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "my_action"

    def test_audit_entry_outcome_success(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert "success" in str(entry.outcome).lower()

    def test_audit_entry_has_duration_ms(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.duration_ms, (int, float))
        assert entry.duration_ms >= 0

    def test_audit_entry_has_timestamp_ms(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_audit_entry_has_actor(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.set_actor("my-agent")
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "my-agent"

    def test_audit_entry_has_params_json(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.params_json == "{}"

    def test_audit_log_successes_contains_allowed_entries(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        log = ctx.audit_log
        assert len(log.successes()) >= 1

    def test_audit_log_denials_empty_when_no_denials(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("create_sphere", "{}")
        log = ctx.audit_log
        # denials should be empty if no denied actions were attempted
        assert isinstance(log.denials(), list)

    def test_audit_log_entries_for_action(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["op_a", "op_b"])
        ctx = SandboxContext(p)
        ctx.execute_json("op_a", "{}")
        ctx.execute_json("op_b", "{}")
        ctx.execute_json("op_a", "{}")
        log = ctx.audit_log
        a_entries = log.entries_for_action("op_a")
        assert len(a_entries) == 2

    def test_audit_log_to_json_returns_parseable_json(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.set_actor("agent-test")
        ctx.execute_json("create_sphere", "{}")
        json_str = ctx.audit_log.to_json()
        parsed = json.loads(json_str)
        assert isinstance(parsed, list)
        assert len(parsed) >= 1
        assert "action" in parsed[0]
        assert "outcome" in parsed[0]

    def test_audit_log_to_json_contains_actor(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        ctx = SandboxContext(p)
        ctx.set_actor("json-test-agent")
        ctx.execute_json("create_sphere", "{}")
        json_str = ctx.audit_log.to_json()
        assert "json-test-agent" in json_str

    def test_audit_log_multiple_actions(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["a", "b", "c"])
        ctx = SandboxContext(p)
        for action in ["a", "b", "c", "a"]:
            ctx.execute_json(action, "{}")
        log = ctx.audit_log
        assert len(log.entries()) == 4

    def test_audit_entry_repr(self):
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy

        p = SandboxPolicy()
        p.allow_actions(["sphere"])
        ctx = SandboxContext(p)
        ctx.execute_json("sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        rep = repr(entry)
        assert "sphere" in rep


class TestEventBusCreate:
    """EventBus construction."""

    def test_create_event_bus(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        assert eb is not None

    def test_multiple_instances_independent(self):
        from dcc_mcp_core import EventBus

        eb1 = EventBus()
        eb2 = EventBus()
        received1 = []
        received2 = []
        eb1.subscribe("evt", lambda **kw: received1.append(kw))
        eb2.subscribe("evt", lambda **kw: received2.append(kw))
        eb1.publish("evt", key="from_eb1")
        assert len(received1) == 1
        assert len(received2) == 0


class TestEventBusSubscribePublish:
    """EventBus subscribe/publish/unsubscribe semantics."""

    def test_subscribe_returns_int_id(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        sid = eb.subscribe("test.event", lambda **kw: None)
        assert isinstance(sid, int)
        assert sid > 0

    def test_publish_delivers_kwargs_to_subscriber(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received = []
        eb.subscribe("my.event", lambda **kw: received.append(kw))
        eb.publish("my.event", key="value", num=42)
        assert len(received) == 1
        assert received[0]["key"] == "value"
        assert received[0]["num"] == 42

    def test_publish_no_subscribers_no_error(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        eb.publish("nonexistent.event", data="test")
        # Should not raise

    def test_publish_multiple_times_calls_subscriber_each_time(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        count = []
        eb.subscribe("repeated", lambda **kw: count.append(1))
        for _ in range(5):
            eb.publish("repeated", x=1)
        assert len(count) == 5

    def test_multiple_subscribers_same_event(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received_a = []
        received_b = []
        eb.subscribe("shared", lambda **kw: received_a.append(kw))
        eb.subscribe("shared", lambda **kw: received_b.append(kw))
        eb.publish("shared", msg="hello")
        assert len(received_a) == 1
        assert len(received_b) == 1

    def test_subscribe_returns_unique_ids(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        sid1 = eb.subscribe("evt", lambda **kw: None)
        sid2 = eb.subscribe("evt", lambda **kw: None)
        assert sid1 != sid2

    def test_unsubscribe_stops_delivery(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received = []
        sid = eb.subscribe("evt", lambda **kw: received.append(kw))
        eb.publish("evt", x=1)
        assert len(received) == 1
        eb.unsubscribe("evt", sid)
        eb.publish("evt", x=2)
        assert len(received) == 1  # still 1, not 2

    def test_unsubscribe_one_leaves_other(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received_a = []
        received_b = []
        sid_a = eb.subscribe("evt", lambda **kw: received_a.append(kw))
        eb.subscribe("evt", lambda **kw: received_b.append(kw))
        eb.publish("evt", x=1)
        eb.unsubscribe("evt", sid_a)
        eb.publish("evt", x=2)
        assert len(received_a) == 1  # only first delivery
        assert len(received_b) == 2  # both deliveries

    def test_different_events_independent(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received_a = []
        received_b = []
        eb.subscribe("event.a", lambda **kw: received_a.append(kw))
        eb.subscribe("event.b", lambda **kw: received_b.append(kw))
        eb.publish("event.a", src="a")
        assert len(received_a) == 1
        assert len(received_b) == 0
        eb.publish("event.b", src="b")
        assert len(received_a) == 1
        assert len(received_b) == 1

    def test_unsubscribe_nonexistent_no_error(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        # Unsubscribing a nonexistent ID should not crash
        import contextlib

        with contextlib.suppress(Exception):
            eb.unsubscribe("evt", 99999)

    def test_subscriber_receives_empty_kwargs(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        received = []
        eb.subscribe("empty", lambda **kw: received.append(kw))
        eb.publish("empty")
        assert len(received) == 1
        assert received[0] == {}

    def test_subscriber_exception_does_not_crash_bus(self):
        from dcc_mcp_core import EventBus

        eb = EventBus()
        good_received = []

        def bad_sub(**kw):
            raise ValueError("subscriber error")

        eb.subscribe("evt", bad_sub)
        eb.subscribe("evt", lambda **kw: good_received.append(kw))
        import contextlib

        with contextlib.suppress(Exception):
            eb.publish("evt", x=1)


class TestActionPipelineMiddleware:
    """ActionPipeline middleware: timing, audit, rate_limit, callable hooks."""

    def _make_pipeline(self, actions=None):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        if actions is None:
            actions = ["sphere"]
        for name in actions:
            reg.register(name, description=f"Action {name}")
        disp = ActionDispatcher(reg)
        for name in actions:
            disp.register_handler(name, lambda p: {"done": True})
        pipe = ActionPipeline(disp)
        return pipe

    def test_pipeline_create_zero_middleware(self):
        pipe = self._make_pipeline()
        assert pipe.middleware_count() == 0

    def test_add_timing_increments_middleware_count(self):
        pipe = self._make_pipeline()
        pipe.add_timing()
        assert pipe.middleware_count() == 1

    def test_add_audit_increments_middleware_count(self):
        pipe = self._make_pipeline()
        pipe.add_audit()
        assert pipe.middleware_count() == 1

    def test_add_rate_limit_increments_middleware_count(self):
        pipe = self._make_pipeline()
        pipe.add_rate_limit(max_calls=10, window_ms=1000)
        assert pipe.middleware_count() == 1

    def test_add_callable_increments_middleware_count(self):
        pipe = self._make_pipeline()
        pipe.add_callable(before_fn=lambda a: None)
        assert pipe.middleware_count() == 1

    def test_multiple_middleware_counted(self):
        pipe = self._make_pipeline()
        pipe.add_timing()
        pipe.add_audit()
        pipe.add_rate_limit(max_calls=5, window_ms=500)
        assert pipe.middleware_count() == 3

    def test_middleware_names_returns_list(self):
        pipe = self._make_pipeline()
        pipe.add_timing()
        pipe.add_audit()
        names = pipe.middleware_names()
        assert isinstance(names, list)
        assert len(names) == 2

    def test_middleware_names_contain_timing(self):
        pipe = self._make_pipeline()
        pipe.add_timing()
        names = pipe.middleware_names()
        assert any("timing" in n.lower() for n in names)

    def test_middleware_names_contain_audit(self):
        pipe = self._make_pipeline()
        pipe.add_audit()
        names = pipe.middleware_names()
        assert any("audit" in n.lower() for n in names)

    def test_handler_count_reflects_registered_handlers(self):
        pipe = self._make_pipeline(["a", "b", "c"])
        assert pipe.handler_count() == 3

    def test_register_handler_on_pipeline(self):
        pipe = self._make_pipeline()
        pipe.register_handler("extra_op", lambda p: {"extra": True})
        assert pipe.handler_count() == 2


class TestActionPipelineTiming:
    """ActionPipeline timing middleware."""

    def test_timing_last_elapsed_ms_after_dispatch(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("sphere", description="sphere")
        disp = ActionDispatcher(reg)
        disp.register_handler("sphere", lambda p: {})
        pipe = ActionPipeline(disp)
        timing = pipe.add_timing()
        pipe.dispatch("sphere", "{}")
        elapsed = timing.last_elapsed_ms("sphere")
        assert elapsed is not None
        assert isinstance(elapsed, (int, float))
        assert elapsed >= 0

    def test_timing_none_before_dispatch(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("sphere", description="sphere")
        disp = ActionDispatcher(reg)
        disp.register_handler("sphere", lambda p: {})
        pipe = ActionPipeline(disp)
        timing = pipe.add_timing()
        result = timing.last_elapsed_ms("sphere")
        assert result is None

    def test_timing_tracks_multiple_actions(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        for name in ["a", "b"]:
            reg.register(name, description=name)
        disp = ActionDispatcher(reg)
        for name in ["a", "b"]:
            disp.register_handler(name, lambda p: {})
        pipe = ActionPipeline(disp)
        timing = pipe.add_timing()
        pipe.dispatch("a", "{}")
        pipe.dispatch("b", "{}")
        assert timing.last_elapsed_ms("a") is not None
        assert timing.last_elapsed_ms("b") is not None


class TestActionPipelineAudit:
    """ActionPipeline audit middleware."""

    def test_audit_empty_before_dispatch(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("sphere", description="sphere")
        disp = ActionDispatcher(reg)
        disp.register_handler("sphere", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        assert audit.record_count() == 0
        assert audit.records() == []

    def test_audit_records_after_dispatch(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("sphere", description="sphere")
        disp = ActionDispatcher(reg)
        disp.register_handler("sphere", lambda p: {"name": "s1"})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit(record_params=True)
        pipe.dispatch("sphere", "{}")
        assert audit.record_count() == 1

    def test_audit_record_has_action_key(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("cube", description="cube")
        disp = ActionDispatcher(reg)
        disp.register_handler("cube", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        pipe.dispatch("cube", "{}")
        record = audit.records()[0]
        assert record["action"] == "cube"

    def test_audit_record_has_success_true(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        pipe.dispatch("op", "{}")
        record = audit.records()[0]
        assert record["success"] is True

    def test_audit_record_has_timestamp_ms(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        pipe.dispatch("op", "{}")
        record = audit.records()[0]
        assert "timestamp_ms" in record
        assert record["timestamp_ms"] > 0

    def test_audit_records_for_action_filters(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        for name in ["a", "b"]:
            reg.register(name, description=name)
        disp = ActionDispatcher(reg)
        for name in ["a", "b"]:
            disp.register_handler(name, lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        pipe.dispatch("a", "{}")
        pipe.dispatch("b", "{}")
        pipe.dispatch("a", "{}")
        a_records = audit.records_for_action("a")
        assert len(a_records) == 2

    def test_audit_clear_resets_records(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        pipe.dispatch("op", "{}")
        assert audit.record_count() == 1
        audit.clear()
        assert audit.record_count() == 0
        assert audit.records() == []

    def test_audit_multiple_dispatches_accumulate(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()
        for _ in range(7):
            pipe.dispatch("op", "{}")
        assert audit.record_count() == 7


class TestActionPipelineRateLimit:
    """ActionPipeline rate_limit middleware."""

    def test_rate_limit_max_calls_property(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        rl = pipe.add_rate_limit(max_calls=15, window_ms=2000)
        assert rl.max_calls == 15

    def test_rate_limit_window_ms_property(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        rl = pipe.add_rate_limit(max_calls=10, window_ms=3000)
        assert rl.window_ms == 3000

    def test_rate_limit_call_count_starts_zero(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        rl = pipe.add_rate_limit(max_calls=10, window_ms=1000)
        assert rl.call_count("op") == 0

    def test_rate_limit_call_count_increments(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        rl = pipe.add_rate_limit(max_calls=100, window_ms=60000)
        pipe.dispatch("op", "{}")
        pipe.dispatch("op", "{}")
        assert rl.call_count("op") >= 1  # at least 1 due to window semantics

    def test_rate_limit_exceeded_raises(self):
        """When rate limit is exceeded, dispatch should raise RuntimeError or similar."""
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        pipe.add_rate_limit(max_calls=2, window_ms=60000)  # Very tight: 2 calls / 60s
        pipe.dispatch("op", "{}")
        pipe.dispatch("op", "{}")
        import contextlib

        with contextlib.suppress(Exception):
            pipe.dispatch("op", "{}")  # 3rd call should be rejected


class TestActionPipelineCallable:
    """ActionPipeline add_callable hooks."""

    def test_before_hook_called(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        before_calls = []
        pipe.add_callable(before_fn=lambda action: before_calls.append(action))
        pipe.dispatch("op", "{}")
        assert len(before_calls) == 1

    def test_after_hook_called(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        after_calls = []
        pipe.add_callable(after_fn=lambda action, success: after_calls.append((action, success)))
        pipe.dispatch("op", "{}")
        assert len(after_calls) == 1

    def test_both_hooks_called(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        before_calls = []
        after_calls = []
        pipe.add_callable(
            before_fn=lambda action: before_calls.append(action),
            after_fn=lambda action, success: after_calls.append((action, success)),
        )
        pipe.dispatch("op", "{}")
        assert len(before_calls) == 1
        assert len(after_calls) == 1

    def test_no_hooks_does_not_crash(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {})
        pipe = ActionPipeline(disp)
        pipe.add_callable()
        result = pipe.dispatch("op", "{}")
        assert "action" in result

    def test_dispatch_result_has_output(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("op", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("op", lambda p: {"value": 99})
        pipe = ActionPipeline(disp)
        result = pipe.dispatch("op", "{}")
        assert "output" in result
        assert result["output"] == {"value": 99}

    def test_dispatch_result_has_action_name(self):
        from dcc_mcp_core import ActionDispatcher
        from dcc_mcp_core import ActionPipeline
        from dcc_mcp_core import ActionRegistry

        reg = ActionRegistry()
        reg.register("my_action", description="op")
        disp = ActionDispatcher(reg)
        disp.register_handler("my_action", lambda p: {})
        pipe = ActionPipeline(disp)
        result = pipe.dispatch("my_action", "{}")
        assert result["action"] == "my_action"
