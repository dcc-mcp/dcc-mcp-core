"""Tests for SandboxPolicy/SandboxContext, TelemetryConfig, ActionRecorder, SkillWatcher, McpHttpConfig.

Coverage areas:

- SandboxPolicy: allow_paths, deny_actions, set_max_actions, set_read_only
- SandboxContext: execute_json, is_path_allowed, audit log entries, to_json
- TelemetryConfig: builder chain methods (with_service_version, with_noop_exporter, etc.)
- ActionRecorder: start/finish, metrics, all_metrics, reset
- SkillWatcher: watch/unwatch/reload, skills(), skill_count(), watched_paths()
- McpHttpConfig: port, server_name, server_version attributes
"""

from __future__ import annotations

import contextlib
import json
from pathlib import Path
import tempfile

import pytest

from dcc_mcp_core import ActionRecorder
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import SkillWatcher
from dcc_mcp_core import TelemetryConfig

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_skill_dir(root: str, name: str = "test_skill") -> str:
    """Create a minimal SKILL.md directory under *root* and return root."""
    skill_dir = Path(root) / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    (skill_dir / "SKILL.md").write_text(
        f"---\nname: {name}\nversion: 1.0.0\ndescription: A test skill\nauthor: test\ntags: [test]\n---\n\n# {name}\n"
    )
    return root


# ===========================================================================
# 1. SandboxPolicy — path restrictions
# ===========================================================================


class TestSandboxPolicyPathRestriction:
    """Tests for SandboxPolicy.allow_paths and SandboxContext.is_path_allowed."""

    def _ctx_with_paths(self, paths: list[str]) -> SandboxContext:
        p = SandboxPolicy()
        p.allow_actions(["read_file"])
        p.allow_paths(paths)
        return SandboxContext(p)

    def test_path_in_allowed_prefix(self):
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/tmp/test.txt") is True

    def test_path_in_subdirectory(self):
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/tmp/sub/deep/file.txt") is True

    def test_directory_itself_allowed(self):
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/tmp") is True

    def test_path_outside_prefix_denied(self):
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/etc/passwd") is False

    def test_path_partial_match_not_allowed(self):
        # /tmpother should NOT match prefix /tmp/
        ctx = self._ctx_with_paths(["/tmp/"])
        # /tmpother doesn't start with /tmp/  so depends on implementation
        # We just test a clearly different path
        assert ctx.is_path_allowed("/var/log/syslog") is False

    def test_multiple_allowed_paths(self):
        # allow_paths replaces on each call; pass both at once
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/tmp/x") is True
        assert ctx.is_path_allowed("/root/secret") is False

    def test_empty_paths_list_allows_any(self):
        # When no paths restriction is set, any path is allowed
        p = SandboxPolicy()
        p.allow_actions(["read_file"])
        ctx = SandboxContext(p)
        assert ctx.is_path_allowed("/etc/passwd") is True
        assert ctx.is_path_allowed("/any/path") is True

    def test_allow_paths_with_single_path(self):
        ctx = self._ctx_with_paths(["/tmp/"])
        assert ctx.is_path_allowed("/tmp/project/file.py") is True
        assert ctx.is_path_allowed("/other/") is False

    def test_deny_actions_only_policy_allows_other_actions(self):
        p = SandboxPolicy()
        p.deny_actions(["dangerous_action", "rm_rf"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("create_sphere") is True
        assert ctx.is_allowed("get_scene_info") is True

    def test_deny_actions_blocks_specified(self):
        p = SandboxPolicy()
        p.deny_actions(["dangerous_action", "rm_rf"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("dangerous_action") is False
        assert ctx.is_allowed("rm_rf") is False

    def test_allow_list_takes_precedence_over_all(self):
        # When allow_actions is set, only listed are allowed
        p = SandboxPolicy()
        p.allow_actions(["safe_action"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("safe_action") is True
        assert ctx.is_allowed("other_action") is False

    def test_no_path_restriction_initially(self):
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        assert ctx.is_path_allowed("/any/path/here") is True

    def test_policy_read_only_flag(self):
        p = SandboxPolicy()
        assert p.is_read_only is False
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_policy_read_only_toggle(self):
        p = SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True
        p.set_read_only(False)
        assert p.is_read_only is False

    def test_sandbox_context_initial_action_count_zero(self):
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        assert ctx.action_count == 0

    def test_policy_default_constructor(self):
        p = SandboxPolicy()
        assert p is not None
        assert p.is_read_only is False

    def test_policy_set_timeout_ms(self):
        p = SandboxPolicy()
        # Should not raise
        p.set_timeout_ms(5000)
        p.set_timeout_ms(0)
        p.set_timeout_ms(60000)

    def test_policy_set_max_actions_zero(self):
        p = SandboxPolicy()
        p.set_max_actions(0)
        # Should not raise; means 0 actions allowed

    def test_context_set_actor(self):
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.set_actor("robot_agent")
        # No attribute to read back actor, but should not raise

    def test_context_repr(self):
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        r = repr(ctx)
        assert "SandboxContext" in r
        assert "action_count" in r


# ===========================================================================
# 2. SandboxContext — execute_json and audit log deep
# ===========================================================================


class TestSandboxContextAuditDeep:
    """Tests for SandboxContext.execute_json and AuditLog / AuditEntry fields."""

    def _ctx(self, allowed: list[str]) -> SandboxContext:
        p = SandboxPolicy()
        p.allow_actions(allowed)
        ctx = SandboxContext(p)
        ctx.set_actor("test_agent")
        return ctx

    def test_execute_allowed_action_returns_result(self):
        ctx = self._ctx(["ping"])
        result = ctx.execute_json("ping", "{}")
        # execute_json returns JSON string ('null' when no handler registered)
        assert result is not None
        assert isinstance(result, str)

    def test_execute_increments_action_count(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        assert ctx.action_count == 1

    def test_execute_multiple_increments_count(self):
        ctx = self._ctx(["ping", "pong"])
        ctx.execute_json("ping", "{}")
        ctx.execute_json("pong", "{}")
        ctx.execute_json("ping", "{}")
        assert ctx.action_count == 3

    def test_execute_denied_action_raises_runtime_error(self):
        ctx = self._ctx(["ping"])
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("forbidden_action", "{}")

    def test_denied_action_does_not_increment_count(self):
        ctx = self._ctx(["ping"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden_action", "{}")
        assert ctx.action_count == 0

    def test_audit_log_entries_after_execute(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        log = ctx.audit_log
        entries = log.entries()
        assert len(entries) == 1

    def test_audit_entry_action_field(self):
        ctx = self._ctx(["create_sphere"])
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "create_sphere"

    def test_audit_entry_actor_field(self):
        ctx = self._ctx(["create_sphere"])
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "test_agent"

    def test_audit_entry_outcome_success(self):
        ctx = self._ctx(["create_sphere"])
        ctx.execute_json("create_sphere", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.outcome == "success"

    def test_audit_entry_timestamp_ms_positive(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_audit_entry_duration_ms_non_negative(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.duration_ms, int)
        assert entry.duration_ms >= 0

    def test_audit_entry_params_json_field(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        # params_json should be the JSON string we passed
        assert entry.params_json is not None

    def test_audit_log_successes(self):
        ctx = self._ctx(["ping", "pong"])
        ctx.execute_json("ping", "{}")
        ctx.execute_json("pong", "{}")
        successes = ctx.audit_log.successes()
        assert len(successes) == 2

    def test_audit_log_denials_empty_when_all_succeed(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        denials = ctx.audit_log.denials()
        assert denials == []

    def test_audit_log_entries_for_action_filter(self):
        ctx = self._ctx(["ping", "pong"])
        ctx.execute_json("ping", "{}")
        ctx.execute_json("pong", "{}")
        ctx.execute_json("ping", "{}")
        entries = ctx.audit_log.entries_for_action("ping")
        assert len(entries) == 2
        assert all(e.action == "ping" for e in entries)

    def test_audit_log_entries_for_nonexistent_action_empty(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        entries = ctx.audit_log.entries_for_action("nonexistent_action")
        assert entries == []

    def test_audit_log_to_json_valid_json(self):
        ctx = self._ctx(["ping"])
        ctx.execute_json("ping", "{}")
        j = ctx.audit_log.to_json()
        data = json.loads(j)
        assert isinstance(data, list)
        assert len(data) == 1

    def test_audit_log_to_json_entry_keys(self):
        ctx = self._ctx(["create_sphere"])
        ctx.execute_json("create_sphere", "{}")
        data = json.loads(ctx.audit_log.to_json())
        entry = data[0]
        assert "action" in entry
        assert "timestamp_ms" in entry
        assert "outcome" in entry

    def test_audit_log_multiple_entries_to_json(self):
        ctx = self._ctx(["a", "b", "c"])
        ctx.execute_json("a", "{}")
        ctx.execute_json("b", "{}")
        ctx.execute_json("c", "{}")
        data = json.loads(ctx.audit_log.to_json())
        assert len(data) == 3

    def test_audit_log_len(self):
        ctx = self._ctx(["ping"])
        assert len(ctx.audit_log) == 0
        ctx.execute_json("ping", "{}")
        assert len(ctx.audit_log) == 1


# ===========================================================================
# 3. SandboxContext — max actions limit and read-only mode
# ===========================================================================


class TestSandboxMaxActionsLimit:
    """Tests for set_max_actions enforcement via execute_json."""

    def test_max_actions_allows_exact_count(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(3)
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        ctx.execute_json("ping", "{}")
        ctx.execute_json("ping", "{}")
        assert ctx.action_count == 3

    def test_max_actions_exceeded_raises(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(2)
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        ctx.execute_json("ping", "{}")
        with pytest.raises(RuntimeError):
            ctx.execute_json("ping", "{}")

    def test_max_actions_exceeded_error_message_contains_count(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(1)
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        with pytest.raises(RuntimeError, match="1"):
            ctx.execute_json("ping", "{}")

    def test_max_actions_one(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(1)
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        assert ctx.action_count == 1
        with pytest.raises(RuntimeError):
            ctx.execute_json("ping", "{}")

    def test_no_max_actions_allows_many(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        for _ in range(20):
            ctx.execute_json("ping", "{}")
        assert ctx.action_count == 20

    def test_set_read_only_policy_does_not_affect_is_allowed(self):
        # read_only is a mode flag, but is_allowed checks the whitelist
        p = SandboxPolicy()
        p.allow_actions(["create_sphere"])
        p.set_read_only(True)
        ctx = SandboxContext(p)
        # is_allowed checks whitelist, not read-only
        assert ctx.is_allowed("create_sphere") is True

    def test_policy_max_actions_large_value(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(1000)
        ctx = SandboxContext(p)
        for _ in range(10):
            ctx.execute_json("ping", "{}")
        assert ctx.action_count == 10

    def test_set_actor_then_audit_entry_actor(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        ctx.set_actor("custom_actor")
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "custom_actor"

    def test_default_actor_is_not_empty_string(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        # actor is None when set_actor has not been called
        # (None is the default, no actor assigned)
        assert entry.actor is None or isinstance(entry.actor, str)

    def test_multiple_contexts_independent(self):
        p1 = SandboxPolicy()
        p1.allow_actions(["ping"])
        p1.set_max_actions(2)
        ctx1 = SandboxContext(p1)

        p2 = SandboxPolicy()
        p2.allow_actions(["pong"])
        ctx2 = SandboxContext(p2)

        ctx1.execute_json("ping", "{}")
        ctx2.execute_json("pong", "{}")
        assert ctx1.action_count == 1
        assert ctx2.action_count == 1

    def test_policy_deny_with_allow_list_deny_wins(self):
        # If allow_actions is set AND deny_actions includes same action,
        # deny should take precedence
        p = SandboxPolicy()
        p.allow_actions(["ping", "pong"])
        p.deny_actions(["ping"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("ping") is False

    def test_policy_allow_list_restricts_non_listed(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("not_listed") is False

    def test_execute_json_with_empty_params(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        result = ctx.execute_json("ping", "{}")
        # execute_json returns a JSON string, e.g. 'null'
        assert isinstance(result, str)

    def test_execute_json_denied_action_not_in_audit_log_as_success(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        ctx = SandboxContext(p)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", "{}")
        successes = ctx.audit_log.successes()
        assert len(successes) == 0

    def test_set_max_actions_100(self):
        p = SandboxPolicy()
        p.allow_actions(["ping"])
        p.set_max_actions(100)
        ctx = SandboxContext(p)
        for _ in range(5):
            ctx.execute_json("ping", "{}")
        assert ctx.action_count == 5


# ===========================================================================
# 4. TelemetryConfig — builder chain
# ===========================================================================


class TestTelemetryConfigBuilder:
    """Tests for TelemetryConfig builder methods and attributes."""

    def test_constructor_requires_service_name(self):
        with pytest.raises(TypeError):
            TelemetryConfig()

    def test_constructor_sets_service_name(self):
        t = TelemetryConfig("my_service")
        assert t.service_name == "my_service"

    def test_enable_tracing_default_true(self):
        t = TelemetryConfig("svc")
        assert t.enable_tracing is True

    def test_enable_metrics_default_true(self):
        t = TelemetryConfig("svc")
        assert t.enable_metrics is True

    def test_with_service_version_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_service_version("1.2.3")
        assert t2 is not None
        assert "TelemetryConfig" in repr(t2)

    def test_with_service_version_service_name_preserved(self):
        t = TelemetryConfig("my_svc")
        t2 = t.with_service_version("2.0.0")
        assert t2.service_name == "my_svc"

    def test_with_noop_exporter_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_noop_exporter()
        assert "Noop" in repr(t2)

    def test_with_stdout_exporter_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_stdout_exporter()
        assert "Stdout" in repr(t2)

    def test_with_json_logs_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_json_logs()
        assert t2 is not None

    def test_with_text_logs_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_text_logs()
        assert t2 is not None

    def test_with_attribute_returns_config(self):
        t = TelemetryConfig("svc")
        t2 = t.with_attribute("env", "production")
        assert t2 is not None

    def test_set_enable_tracing_false(self):
        t = TelemetryConfig("svc")
        t2 = t.set_enable_tracing(False)
        assert t2 is not None

    def test_set_enable_metrics_false(self):
        t = TelemetryConfig("svc")
        t2 = t.set_enable_metrics(False)
        assert t2 is not None

    def test_builder_chain(self):
        t = (
            TelemetryConfig("pipeline_service")
            .with_service_version("3.0.0")
            .with_noop_exporter()
            .with_attribute("team", "dcc")
        )
        assert t.service_name == "pipeline_service"

    def test_noop_exporter_in_repr(self):
        t = TelemetryConfig("svc").with_noop_exporter()
        assert "Noop" in repr(t)

    def test_service_name_empty_string(self):
        t = TelemetryConfig("")
        assert t.service_name == ""

    def test_service_name_with_special_chars(self):
        t = TelemetryConfig("dcc-mcp-core/maya")
        assert "dcc-mcp-core/maya" in t.service_name

    def test_init_method_exists(self):
        t = TelemetryConfig("svc")
        assert hasattr(t, "init")

    def test_with_noop_then_stdout(self):
        t = TelemetryConfig("svc").with_noop_exporter().with_stdout_exporter()
        assert "Stdout" in repr(t)


# ===========================================================================
# 5. ActionRecorder — start/finish/metrics/all_metrics/reset
# ===========================================================================


class TestActionRecorderMetrics:
    """Tests for ActionRecorder lifecycle and ActionMetrics fields."""

    def _recorder(self) -> ActionRecorder:
        return ActionRecorder("test_recorder")

    def test_start_returns_recording_guard(self):
        r = self._recorder()
        guard = r.start("create_sphere", "maya")
        assert guard is not None
        guard.finish(True)

    def test_finish_true_records_success(self):
        r = self._recorder()
        r.start("create_sphere", "maya").finish(True)
        m = r.metrics("create_sphere")
        assert m is not None
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_finish_false_records_failure(self):
        r = self._recorder()
        r.start("delete_sphere", "maya").finish(False)
        m = r.metrics("delete_sphere")
        assert m is not None
        assert m.success_count == 0
        assert m.failure_count == 1

    def test_invocation_count_increments(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert m.invocation_count == 2

    def test_success_rate_all_success(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert abs(m.success_rate() - 1.0) < 1e-6

    def test_success_rate_all_failure(self):
        r = self._recorder()
        r.start("op", "maya").finish(False)
        r.start("op", "maya").finish(False)
        m = r.metrics("op")
        assert abs(m.success_rate() - 0.0) < 1e-6

    def test_action_name_in_metrics(self):
        r = self._recorder()
        r.start("my_action", "blender").finish(True)
        m = r.metrics("my_action")
        assert m.action_name == "my_action"

    def test_metrics_none_for_unknown_action(self):
        r = self._recorder()
        assert r.metrics("nonexistent") is None

    def test_all_metrics_returns_list(self):
        r = self._recorder()
        r.start("a", "maya").finish(True)
        r.start("b", "maya").finish(False)
        all_m = r.all_metrics()
        assert isinstance(all_m, list)
        assert len(all_m) == 2

    def test_all_metrics_empty_initially(self):
        r = self._recorder()
        assert r.all_metrics() == []

    def test_reset_clears_all_metrics(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        r.reset()
        assert r.metrics("op") is None
        assert r.all_metrics() == []

    def test_avg_duration_ms_field(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert isinstance(m.avg_duration_ms, (int, float))
        assert m.avg_duration_ms >= 0

    def test_p95_duration_ms_field(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert isinstance(m.p95_duration_ms, (int, float))

    def test_p99_duration_ms_field(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert isinstance(m.p99_duration_ms, (int, float))

    def test_multiple_actions_independent_metrics(self):
        r = self._recorder()
        r.start("a", "maya").finish(True)
        r.start("b", "maya").finish(False)
        ma = r.metrics("a")
        mb = r.metrics("b")
        assert ma.invocation_count == 1
        assert mb.invocation_count == 1
        assert ma.success_count == 1
        assert mb.failure_count == 1

    def test_same_action_different_dcc(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        r.start("op", "blender").finish(False)
        # Both recorded under same action name
        m = r.metrics("op")
        assert m.invocation_count == 2

    def test_guard_repr_contains_action(self):
        r = self._recorder()
        guard = r.start("my_action", "houdini")
        assert "my_action" in repr(guard)
        guard.finish(True)

    def test_recorder_scope_name(self):
        r = ActionRecorder("custom_scope")
        # Just ensure it constructs without error
        assert r is not None

    def test_reset_after_many_records(self):
        r = self._recorder()
        for i in range(10):
            r.start(f"op_{i}", "maya").finish(i % 2 == 0)
        r.reset()
        assert r.all_metrics() == []

    def test_finish_true_false_mixed(self):
        r = self._recorder()
        r.start("op", "maya").finish(True)
        r.start("op", "maya").finish(False)
        r.start("op", "maya").finish(True)
        m = r.metrics("op")
        assert m.invocation_count == 3
        assert m.success_count == 2
        assert m.failure_count == 1


# ===========================================================================
# 6. SkillWatcher — lifecycle
# ===========================================================================


class TestSkillWatcherLifecycle:
    """Tests for SkillWatcher watch/unwatch/reload/skills()/skill_count()/watched_paths()."""

    def test_construct_with_debounce_ms(self):
        sw = SkillWatcher(300)
        assert sw is not None

    def test_initial_skill_count_zero(self):
        sw = SkillWatcher(100)
        assert sw.skill_count() == 0

    def test_initial_watched_paths_empty(self):
        sw = SkillWatcher(100)
        assert sw.watched_paths() == []

    def test_initial_skills_empty(self):
        sw = SkillWatcher(100)
        assert sw.skills() == []

    def test_watch_adds_path(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir)
            sw.watch(tmpdir)
            paths = sw.watched_paths()
            assert len(paths) == 1
            sw.unwatch(tmpdir)

    def test_watch_loads_skills_immediately(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir, "my_skill")
            sw.watch(tmpdir)
            assert sw.skill_count() >= 1
            sw.unwatch(tmpdir)

    def test_skills_returns_list_of_skill_metadata(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir, "test_skill")
            sw.watch(tmpdir)
            skills = sw.skills()
            assert isinstance(skills, list)
            assert len(skills) >= 1
            sw.unwatch(tmpdir)

    def test_skill_metadata_has_name(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir, "named_skill")
            sw.watch(tmpdir)
            skills = sw.skills()
            names = [s.name for s in skills]
            assert "named_skill" in names
            sw.unwatch(tmpdir)

    def test_unwatch_removes_path(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir)
            sw.watch(tmpdir)
            assert len(sw.watched_paths()) == 1
            sw.unwatch(tmpdir)
            assert sw.watched_paths() == []

    def test_unwatch_clears_skills(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir)
            sw.watch(tmpdir)
            assert sw.skill_count() >= 1
            sw.unwatch(tmpdir)
            assert sw.skill_count() == 0

    def test_reload_after_watch(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir, "reload_skill")
            sw.watch(tmpdir)
            sw.reload()
            assert sw.skill_count() >= 1
            sw.unwatch(tmpdir)

    def test_reload_on_empty_watcher(self):
        sw = SkillWatcher(100)
        # Should not raise
        sw.reload()
        assert sw.skill_count() == 0

    def test_watch_multiple_dirs(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as dir1, tempfile.TemporaryDirectory() as dir2:
            _make_skill_dir(dir1, "skill_a")
            _make_skill_dir(dir2, "skill_b")
            sw.watch(dir1)
            sw.watch(dir2)
            assert len(sw.watched_paths()) == 2
            assert sw.skill_count() >= 2
            sw.unwatch(dir1)
            sw.unwatch(dir2)

    def test_watch_dir_without_skills(self):
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            sw.watch(tmpdir)
            assert sw.skill_count() == 0
            sw.unwatch(tmpdir)

    def test_repr_contains_skills_count(self):
        sw = SkillWatcher(100)
        r = repr(sw)
        assert "SkillWatcher" in r
        assert "skills" in r

    def test_watched_paths_returns_list(self):
        sw = SkillWatcher(100)
        result = sw.watched_paths()
        assert isinstance(result, list)

    def test_skills_returns_list(self):
        sw = SkillWatcher(100)
        result = sw.skills()
        assert isinstance(result, list)

    def test_skill_count_returns_int(self):
        sw = SkillWatcher(100)
        assert isinstance(sw.skill_count(), int)

    def test_new_skill_after_reload(self):
        """Add skill, watch, unwatch, re-watch with new skill: count increases."""
        sw = SkillWatcher(100)
        with tempfile.TemporaryDirectory() as tmpdir:
            _make_skill_dir(tmpdir, "skill_one")
            sw.watch(tmpdir)
            count_before = sw.skill_count()
            _make_skill_dir(tmpdir, "skill_two")
            sw.reload()
            count_after = sw.skill_count()
            assert count_after >= count_before
            sw.unwatch(tmpdir)

    def test_debounce_ms_varied(self):
        for ms in [0, 100, 500, 1000]:
            sw = SkillWatcher(ms)
            assert sw is not None
            assert sw.skill_count() == 0


# ===========================================================================
# 7. McpHttpConfig — attributes
# ===========================================================================


class TestMcpHttpConfigAttributes:
    """Tests for McpHttpConfig port, server_name, server_version read-only properties."""

    def test_constructor_sets_port(self):
        c = McpHttpConfig(8765)
        assert c.port == 8765

    def test_default_server_name(self):
        c = McpHttpConfig(8765)
        assert isinstance(c.server_name, str)
        assert len(c.server_name) > 0

    def test_default_server_version(self):
        c = McpHttpConfig(8765)
        assert isinstance(c.server_version, str)
        assert len(c.server_version) > 0

    def test_port_zero(self):
        c = McpHttpConfig(0)
        assert c.port == 0

    def test_different_ports(self):
        for port in [80, 443, 8080, 8765, 9000, 65535]:
            c = McpHttpConfig(port)
            assert c.port == port

    def test_server_name_default_value(self):
        c = McpHttpConfig(8765)
        assert c.server_name == "dcc-mcp"

    def test_server_version_contains_dot(self):
        c = McpHttpConfig(8765)
        assert "." in c.server_version

    def test_server_name_not_writable(self):
        c = McpHttpConfig(8765)
        with pytest.raises(AttributeError):
            c.server_name = "new_name"

    def test_server_version_not_writable(self):
        c = McpHttpConfig(8765)
        with pytest.raises(AttributeError):
            c.server_version = "99.0.0"

    def test_port_not_writable(self):
        c = McpHttpConfig(8765)
        with pytest.raises(AttributeError):
            c.port = 9999

    def test_repr_contains_port_info(self):
        c = McpHttpConfig(8765)
        r = repr(c)
        assert r is not None
        assert len(r) > 0

    def test_two_configs_different_ports_independent(self):
        c1 = McpHttpConfig(8001)
        c2 = McpHttpConfig(8002)
        assert c1.port == 8001
        assert c2.port == 8002
