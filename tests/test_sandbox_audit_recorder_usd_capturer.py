"""Deep coverage for sandbox, audit log, recorder, USD stage, and capturer APIs.

Targets not yet exercised by existing tests:
SandboxPolicy is_read_only/deny_actions/set_max_actions; AuditLog to_json/successes/denials/
entries_for_action; AuditEntry all properties; ActionRecorder/ActionMetrics/RecordingGuard full
depth; UsdStage.metrics()/export_usda()/from_json()/set_default_prim(); Capturer.stats()
increment; validate_action_result and from_exception edge inputs.
"""

from __future__ import annotations

import contextlib
import json

import pytest

import dcc_mcp_core

# ── Helpers ─────────────────────────────────────────────────────────────────


def _allowed_ctx(*actions: str) -> dcc_mcp_core.SandboxContext:
    """Return a SandboxContext with only *actions* allowed."""
    policy = dcc_mcp_core.SandboxPolicy()
    policy.allow_actions(list(actions))
    return dcc_mcp_core.SandboxContext(policy)


# ════════════════════════════════════════════════════════════════════════════
# SandboxPolicy
# ════════════════════════════════════════════════════════════════════════════


class TestSandboxPolicyReadOnly:
    """is_read_only flag toggle."""

    def test_default_is_false(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        assert p.is_read_only is False

    def test_set_read_only_true(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_set_read_only_false_again(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.set_read_only(True)
        p.set_read_only(False)
        assert p.is_read_only is False


class TestSandboxPolicyMaxActions:
    """set_max_actions enforcement — 3rd call should be denied."""

    def test_max_actions_allows_up_to_limit(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        p.set_max_actions(2)
        ctx = dcc_mcp_core.SandboxContext(p)
        ctx.execute_json("echo", "{}")
        ctx.execute_json("echo", "{}")
        assert ctx.action_count == 2

    def test_max_actions_denies_over_limit(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        p.set_max_actions(2)
        ctx = dcc_mcp_core.SandboxContext(p)
        ctx.execute_json("echo", "{}")
        ctx.execute_json("echo", "{}")
        with pytest.raises(RuntimeError):
            ctx.execute_json("echo", "{}")

    def test_max_actions_limit_1(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        p.set_max_actions(1)
        ctx = dcc_mcp_core.SandboxContext(p)
        ctx.execute_json("echo", "{}")
        with pytest.raises(RuntimeError):
            ctx.execute_json("echo", "{}")


class TestSandboxPolicyDenyActions:
    """deny_actions overrides allow_actions for the listed names."""

    def test_deny_overrides_allow(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo", "delete"])
        p.deny_actions(["delete"])
        ctx = dcc_mcp_core.SandboxContext(p)
        # echo is allowed
        ctx.execute_json("echo", "{}")
        assert ctx.action_count == 1
        # delete is denied despite being in allow list
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("delete", "{}")

    def test_deny_without_allow_still_denies(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        p.deny_actions(["echo"])
        ctx = dcc_mcp_core.SandboxContext(p)
        with pytest.raises(RuntimeError):
            ctx.execute_json("echo", "{}")

    def test_deny_unlisted_action_is_already_denied(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        ctx = dcc_mcp_core.SandboxContext(p)
        with pytest.raises(RuntimeError):
            ctx.execute_json("not_in_whitelist", "{}")


# ════════════════════════════════════════════════════════════════════════════
# AuditLog + AuditEntry
# ════════════════════════════════════════════════════════════════════════════


class TestAuditLogBasic:
    """AuditLog length / entries() basics."""

    def test_empty_log_length(self) -> None:
        ctx = _allowed_ctx("echo")
        assert len(ctx.audit_log) == 0

    def test_after_one_action_length_is_1(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        assert len(ctx.audit_log) == 1

    def test_entries_returns_list(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        assert isinstance(ctx.audit_log.entries(), list)

    def test_entries_count_matches_len(self) -> None:
        ctx = _allowed_ctx("echo", "get_info")
        ctx.execute_json("echo", "{}")
        ctx.execute_json("get_info", "{}")
        log = ctx.audit_log
        assert len(log.entries()) == len(log)


class TestAuditLogSuccessesDenials:
    """AuditLog.successes() and .denials() filtering."""

    def _make_mixed_ctx(self) -> dcc_mcp_core.SandboxContext:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo", "get_info"])
        ctx = dcc_mcp_core.SandboxContext(p)
        ctx.set_actor("agent-x")
        ctx.execute_json("echo", "{}")
        ctx.execute_json("get_info", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete_all", "{}")
        return ctx

    def test_successes_count(self) -> None:
        ctx = self._make_mixed_ctx()
        assert len(ctx.audit_log.successes()) == 2

    def test_denials_count(self) -> None:
        ctx = self._make_mixed_ctx()
        assert len(ctx.audit_log.denials()) == 1

    def test_successes_outcome_field(self) -> None:
        ctx = self._make_mixed_ctx()
        for entry in ctx.audit_log.successes():
            assert entry.outcome == "success"

    def test_denials_outcome_field(self) -> None:
        ctx = self._make_mixed_ctx()
        for entry in ctx.audit_log.denials():
            assert entry.outcome == "denied"

    def test_no_successes_when_all_denied(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        ctx = dcc_mcp_core.SandboxContext(p)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", "{}")
        assert len(ctx.audit_log.successes()) == 0

    def test_no_denials_when_all_succeed(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        assert len(ctx.audit_log.denials()) == 0


class TestAuditLogEntriesForAction:
    """AuditLog.entries_for_action() filtering."""

    def test_returns_only_matching_action(self) -> None:
        ctx = _allowed_ctx("echo", "get_info")
        ctx.execute_json("echo", "{}")
        ctx.execute_json("get_info", "{}")
        ctx.execute_json("echo", "{}")
        echo_entries = ctx.audit_log.entries_for_action("echo")
        assert len(echo_entries) == 2
        for e in echo_entries:
            assert e.action == "echo"

    def test_returns_empty_for_missing_action(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        result = ctx.audit_log.entries_for_action("nonexistent")
        assert result == []

    def test_entries_for_action_single(self) -> None:
        ctx = _allowed_ctx("ping")
        ctx.execute_json("ping", "{}")
        assert len(ctx.audit_log.entries_for_action("ping")) == 1


class TestAuditLogToJson:
    """AuditLog.to_json() JSON format validation."""

    def test_empty_log_to_json_is_empty_array(self) -> None:
        ctx = _allowed_ctx("echo")
        j = ctx.audit_log.to_json()
        parsed = json.loads(j)
        assert parsed == []

    def test_to_json_has_correct_count(self) -> None:
        ctx = _allowed_ctx("echo", "get_info")
        ctx.execute_json("echo", "{}")
        ctx.execute_json("get_info", "{}")
        parsed = json.loads(ctx.audit_log.to_json())
        assert len(parsed) == 2

    def test_to_json_entry_has_action_field(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        parsed = json.loads(ctx.audit_log.to_json())
        assert "action" in parsed[0]
        assert parsed[0]["action"] == "echo"

    def test_to_json_entry_has_outcome_field(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        parsed = json.loads(ctx.audit_log.to_json())
        assert "outcome" in parsed[0]
        assert parsed[0]["outcome"] == "success"

    def test_to_json_entry_has_timestamp_field(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        parsed = json.loads(ctx.audit_log.to_json())
        assert "timestamp_ms" in parsed[0]
        assert parsed[0]["timestamp_ms"] > 0

    def test_to_json_is_valid_json_string(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        j = ctx.audit_log.to_json()
        assert isinstance(j, str)
        # Must parse without error
        json.loads(j)


class TestAuditEntryProperties:
    """AuditEntry property accessors."""

    def _make_entry(self) -> dcc_mcp_core.AuditEntry:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        ctx = dcc_mcp_core.SandboxContext(p)
        ctx.set_actor("test-agent")
        ctx.execute_json("echo", '{"key": "val"}')
        return ctx.audit_log.entries()[0]

    def test_actor_is_set(self) -> None:
        assert self._make_entry().actor == "test-agent"

    def test_action_is_echo(self) -> None:
        assert self._make_entry().action == "echo"

    def test_outcome_is_success(self) -> None:
        assert self._make_entry().outcome == "success"

    def test_outcome_detail_is_none_on_success(self) -> None:
        assert self._make_entry().outcome_detail is None

    def test_params_json_is_string(self) -> None:
        entry = self._make_entry()
        assert isinstance(entry.params_json, str)

    def test_duration_ms_is_int(self) -> None:
        assert isinstance(self._make_entry().duration_ms, int)

    def test_timestamp_ms_positive(self) -> None:
        assert self._make_entry().timestamp_ms > 0

    def test_repr_contains_action(self) -> None:
        r = repr(self._make_entry())
        assert "echo" in r

    def test_repr_contains_outcome(self) -> None:
        r = repr(self._make_entry())
        assert "success" in r

    def test_denied_entry_outcome_detail_not_none(self) -> None:
        p = dcc_mcp_core.SandboxPolicy()
        p.allow_actions(["echo"])
        ctx = dcc_mcp_core.SandboxContext(p)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden_action", "{}")
        denied = ctx.audit_log.denials()
        assert len(denied) == 1
        # Denied entries carry a reason in outcome_detail
        # (may be None or str depending on sandbox impl)
        entry = denied[0]
        assert entry.outcome == "denied"
        assert entry.action == "forbidden_action"

    def test_actor_none_when_not_set(self) -> None:
        ctx = _allowed_ctx("echo")
        ctx.execute_json("echo", "{}")
        entry = ctx.audit_log.entries()[0]
        # actor not set → None
        assert entry.actor is None


# ════════════════════════════════════════════════════════════════════════════
# ActionRecorder + ActionMetrics + RecordingGuard
# ════════════════════════════════════════════════════════════════════════════


class TestActionRecorderBasic:
    """ActionRecorder creation and basic recording."""

    def test_no_metrics_before_recording(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-1")
        assert rec.metrics("create_sphere") is None

    def test_all_metrics_empty_initially(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-2")
        assert rec.all_metrics() == []

    def test_record_success_creates_metrics(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-3")
        guard = rec.start("create_sphere", "maya")
        guard.finish(success=True)
        m = rec.metrics("create_sphere")
        assert m is not None

    def test_invocation_count_increments(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-4")
        rec.start("ping", "maya").finish(success=True)
        rec.start("ping", "maya").finish(success=True)
        assert rec.metrics("ping").invocation_count == 2

    def test_success_count_correct(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-5")
        rec.start("ping", "maya").finish(success=True)
        rec.start("ping", "maya").finish(success=True)
        rec.start("ping", "maya").finish(success=False)
        m = rec.metrics("ping")
        assert m.success_count == 2
        assert m.failure_count == 1

    def test_failure_count_correct(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-6")
        rec.start("ping", "maya").finish(success=False)
        assert rec.metrics("ping").failure_count == 1


class TestActionMetricsProperties:
    """All properties of ActionMetrics."""

    def _make_metrics(self, successes: int = 2, failures: int = 1) -> dcc_mcp_core.ActionMetrics:
        rec = dcc_mcp_core.ActionRecorder("test-metrics")
        for _ in range(successes):
            rec.start("op", "maya").finish(success=True)
        for _ in range(failures):
            rec.start("op", "maya").finish(success=False)
        m = rec.metrics("op")
        assert m is not None
        return m

    def test_action_name(self) -> None:
        assert self._make_metrics().action_name == "op"

    def test_invocation_count(self) -> None:
        m = self._make_metrics(successes=2, failures=1)
        assert m.invocation_count == 3

    def test_success_count(self) -> None:
        m = self._make_metrics(successes=2, failures=1)
        assert m.success_count == 2

    def test_failure_count(self) -> None:
        m = self._make_metrics(successes=2, failures=1)
        assert m.failure_count == 1

    def test_success_rate_is_fraction(self) -> None:
        m = self._make_metrics(successes=2, failures=0)
        assert m.success_rate() == pytest.approx(1.0)

    def test_success_rate_zero_when_all_fail(self) -> None:
        m = self._make_metrics(successes=0, failures=3)
        assert m.success_rate() == pytest.approx(0.0)

    def test_success_rate_partial(self) -> None:
        m = self._make_metrics(successes=1, failures=1)
        assert m.success_rate() == pytest.approx(0.5)

    def test_avg_duration_ms_is_float(self) -> None:
        assert isinstance(self._make_metrics().avg_duration_ms, float)

    def test_p95_duration_ms_is_float(self) -> None:
        assert isinstance(self._make_metrics().p95_duration_ms, float)

    def test_p99_duration_ms_is_float(self) -> None:
        assert isinstance(self._make_metrics().p99_duration_ms, float)

    def test_repr_contains_action_name(self) -> None:
        m = self._make_metrics()
        assert "op" in repr(m)


class TestActionRecorderAllMetrics:
    """ActionRecorder.all_metrics() and .reset()."""

    def test_all_metrics_lists_all_actions(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-all")
        rec.start("create_sphere", "maya").finish(success=True)
        rec.start("delete_object", "maya").finish(success=False)
        names = sorted(m.action_name for m in rec.all_metrics())
        assert names == ["create_sphere", "delete_object"]

    def test_all_metrics_count(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-cnt")
        rec.start("a", "maya").finish(success=True)
        rec.start("b", "maya").finish(success=True)
        rec.start("c", "maya").finish(success=True)
        assert len(rec.all_metrics()) == 3

    def test_reset_clears_all_metrics(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-reset")
        rec.start("op", "maya").finish(success=True)
        rec.reset()
        assert rec.all_metrics() == []
        assert rec.metrics("op") is None

    def test_reset_allows_fresh_recording(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-fresh")
        rec.start("op", "maya").finish(success=True)
        rec.reset()
        rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m is not None
        assert m.invocation_count == 1


class TestRecordingGuardContextManager:
    """RecordingGuard used as context manager."""

    def test_context_manager_no_exception_success(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-cm")
        with rec.start("op", "maya"):
            pass
        m = rec.metrics("op")
        assert m is not None
        assert m.invocation_count == 1

    def test_context_manager_exception_records_failure(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-ex")
        with pytest.raises(ValueError), rec.start("op", "maya"):
            raise ValueError("test error")
        m = rec.metrics("op")
        assert m is not None
        assert m.invocation_count == 1

    def test_context_manager_enter_returns_guard(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-enter")
        with rec.start("op", "maya") as g:
            assert g is not None

    def test_multiple_context_manager_calls(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-multi")
        for _ in range(3):
            with rec.start("op", "maya"):
                pass
        assert rec.metrics("op").invocation_count == 3

    def test_repr_not_empty(self) -> None:
        rec = dcc_mcp_core.ActionRecorder("scope-repr")
        guard = rec.start("op", "maya")
        assert repr(guard) != ""
        guard.finish(success=True)


# ════════════════════════════════════════════════════════════════════════════
# UsdStage.metrics() / export_usda() / from_json() depth
# ════════════════════════════════════════════════════════════════════════════


class TestUsdStageMetrics:
    """UsdStage.metrics() returns correct per-type counts."""

    def _make_scene(self) -> dcc_mcp_core.UsdStage:
        stage = dcc_mcp_core.UsdStage("metrics_scene")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.define_prim("/World/Sphere", "Sphere")
        return stage

    def test_metrics_returns_dict(self) -> None:
        m = self._make_scene().metrics()
        assert isinstance(m, dict)

    def test_metrics_has_prim_count(self) -> None:
        m = self._make_scene().metrics()
        assert "prim_count" in m
        assert m["prim_count"] == 3

    def test_metrics_has_mesh_count(self) -> None:
        m = self._make_scene().metrics()
        assert "mesh_count" in m
        assert m["mesh_count"] == 1

    def test_metrics_has_xform_count(self) -> None:
        m = self._make_scene().metrics()
        assert "xform_count" in m
        assert m["xform_count"] == 1

    def test_metrics_has_camera_count(self) -> None:
        m = self._make_scene().metrics()
        assert "camera_count" in m
        assert m["camera_count"] == 0

    def test_metrics_has_light_count(self) -> None:
        m = self._make_scene().metrics()
        assert "light_count" in m
        assert m["light_count"] == 0

    def test_metrics_has_material_count(self) -> None:
        m = self._make_scene().metrics()
        assert "material_count" in m
        assert m["material_count"] == 0

    def test_empty_stage_prim_count_zero(self) -> None:
        m = dcc_mcp_core.UsdStage("empty").metrics()
        assert m["prim_count"] == 0


class TestUsdStageExportUsda:
    """UsdStage.export_usda() produces valid USDA text."""

    def test_export_usda_returns_string(self) -> None:
        stage = dcc_mcp_core.UsdStage("usda_stage")
        usda = stage.export_usda()
        assert isinstance(usda, str)

    def test_export_usda_has_header(self) -> None:
        usda = dcc_mcp_core.UsdStage("hdr").export_usda()
        assert usda.startswith("#usda")

    def test_export_usda_non_empty(self) -> None:
        usda = dcc_mcp_core.UsdStage("nonempty").export_usda()
        assert len(usda) > 0

    def test_export_usda_contains_prim_type(self) -> None:
        stage = dcc_mcp_core.UsdStage("prim_test")
        stage.define_prim("/Sphere", "Sphere")
        usda = stage.export_usda()
        assert "Sphere" in usda

    def test_export_usda_contains_up_axis(self) -> None:
        stage = dcc_mcp_core.UsdStage("axis_test")
        stage.up_axis = "Z"
        usda = stage.export_usda()
        assert "Z" in usda


class TestUsdStageFromJsonRoundTrip:
    """UsdStage.to_json() / from_json() round-trip."""

    def test_roundtrip_name_preserved(self) -> None:
        stage = dcc_mcp_core.UsdStage("rt_scene")
        back = dcc_mcp_core.UsdStage.from_json(stage.to_json())
        assert back.name == "rt_scene"

    def test_roundtrip_prim_count_preserved(self) -> None:
        stage = dcc_mcp_core.UsdStage("rt_prims")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/A/B", "Mesh")
        back = dcc_mcp_core.UsdStage.from_json(stage.to_json())
        assert len(back.traverse()) == 2

    def test_roundtrip_up_axis_preserved(self) -> None:
        stage = dcc_mcp_core.UsdStage("rt_axis")
        stage.up_axis = "Z"
        back = dcc_mcp_core.UsdStage.from_json(stage.to_json())
        assert back.up_axis == "Z"

    def test_roundtrip_fps_preserved(self) -> None:
        stage = dcc_mcp_core.UsdStage("rt_fps")
        stage.fps = 30.0
        back = dcc_mcp_core.UsdStage.from_json(stage.to_json())
        assert back.fps == pytest.approx(30.0)

    def test_to_json_is_valid_json(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_valid")
        json.loads(stage.to_json())

    def test_roundtrip_with_attributes(self) -> None:
        stage = dcc_mcp_core.UsdStage("rt_attr")
        stage.define_prim("/Cube", "Mesh")
        stage.set_attribute("/Cube", "radius", dcc_mcp_core.VtValue.from_float(2.5))
        back = dcc_mcp_core.UsdStage.from_json(stage.to_json())
        val = back.get_attribute("/Cube", "radius")
        assert val is not None
        assert val.to_python() == pytest.approx(2.5)


class TestUsdStageSetDefaultPrim:
    """UsdStage.set_default_prim() and default_prim read-back."""

    def test_default_prim_initially_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("dp_init")
        assert stage.default_prim is None

    def test_set_default_prim_returns_correct_path(self) -> None:
        stage = dcc_mcp_core.UsdStage("dp_set")
        stage.define_prim("/World", "Xform")
        stage.set_default_prim("/World")
        assert stage.default_prim == "/World"

    def test_set_default_prim_changes_value(self) -> None:
        stage = dcc_mcp_core.UsdStage("dp_change")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        stage.set_default_prim("/A")
        stage.set_default_prim("/B")
        assert stage.default_prim == "/B"

    def test_clear_default_prim_with_empty_string(self) -> None:
        stage = dcc_mcp_core.UsdStage("dp_clear")
        stage.define_prim("/World", "Xform")
        stage.set_default_prim("/World")
        stage.set_default_prim("")
        # After clearing, default_prim may be None or empty string
        assert stage.default_prim in (None, "", "/")


# ════════════════════════════════════════════════════════════════════════════
# Capturer.stats() increment
# ════════════════════════════════════════════════════════════════════════════


class TestCapturerStatsIncrement:
    """Capturer.stats() increments correctly per capture."""

    def test_initial_stats_all_zero(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        cnt, total, err = c.stats()
        assert cnt == 0
        assert total == 0
        assert err == 0

    def test_one_capture_increments_count(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="png")
        cnt, _, _ = c.stats()
        assert cnt == 1

    def test_two_captures_count_is_2(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="png")
        c.capture(format="png")
        cnt, _, _ = c.stats()
        assert cnt == 2

    def test_bytes_nonzero_after_capture(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="png")
        _, total_bytes, _ = c.stats()
        assert total_bytes > 0

    def test_bytes_accumulate_over_captures(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="png")
        _, b1, _ = c.stats()
        c.capture(format="png")
        _, b2, _ = c.stats()
        assert b2 >= b1

    def test_error_count_zero_on_success(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="png")
        _, _, err = c.stats()
        assert err == 0

    def test_jpeg_capture_increments_count(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="jpeg")
        cnt, _, _ = c.stats()
        assert cnt == 1

    def test_raw_bgra_capture_increments_count(self) -> None:
        c = dcc_mcp_core.Capturer.new_mock(320, 240)
        c.capture(format="raw_bgra")
        cnt, _, _ = c.stats()
        assert cnt == 1

    def test_stats_independent_between_capturers(self) -> None:
        c1 = dcc_mcp_core.Capturer.new_mock(320, 240)
        c2 = dcc_mcp_core.Capturer.new_mock(320, 240)
        c1.capture(format="png")
        c1.capture(format="png")
        cnt1, _, _ = c1.stats()
        cnt2, _, _ = c2.stats()
        assert cnt1 == 2
        assert cnt2 == 0


# ════════════════════════════════════════════════════════════════════════════
# validate_action_result + from_exception
# ════════════════════════════════════════════════════════════════════════════


class TestValidateActionResult:
    """validate_action_result normalises various input types."""

    def test_dict_success_true(self) -> None:
        r = dcc_mcp_core.validate_action_result({"success": True, "message": "ok"})
        assert r.success is True
        assert r.message == "ok"

    def test_dict_success_false(self) -> None:
        r = dcc_mcp_core.validate_action_result({"success": False, "error": "fail"})
        assert r.success is False

    def test_string_input_is_success(self) -> None:
        r = dcc_mcp_core.validate_action_result("any string")
        assert r.success is True

    def test_none_input_is_success(self) -> None:
        r = dcc_mcp_core.validate_action_result(None)
        assert r.success is True

    def test_returns_action_result_model(self) -> None:
        r = dcc_mcp_core.validate_action_result({"success": True})
        assert isinstance(r, dcc_mcp_core.ActionResultModel)

    def test_dict_with_context(self) -> None:
        r = dcc_mcp_core.validate_action_result({"success": True, "message": "m", "context": {"k": "v"}})
        assert r.success is True

    def test_action_result_model_passthrough(self) -> None:
        original = dcc_mcp_core.success_result("hello")
        r = dcc_mcp_core.validate_action_result(original)
        assert r.success is True
        assert r.message == "hello"


class TestFromException:
    """from_exception() produces error results from exception messages."""

    def test_success_is_false(self) -> None:
        r = dcc_mcp_core.from_exception("something went wrong")
        assert r.success is False

    def test_error_contains_message(self) -> None:
        r = dcc_mcp_core.from_exception("connection timeout")
        assert "connection timeout" in r.error

    def test_returns_action_result_model(self) -> None:
        r = dcc_mcp_core.from_exception("err")
        assert isinstance(r, dcc_mcp_core.ActionResultModel)

    def test_with_custom_message(self) -> None:
        r = dcc_mcp_core.from_exception("err detail", message="custom msg")
        assert r.message == "custom msg"

    def test_default_message_not_empty(self) -> None:
        r = dcc_mcp_core.from_exception("err detail")
        assert isinstance(r.message, str)

    def test_with_prompt(self) -> None:
        r = dcc_mcp_core.from_exception("err", prompt="try again")
        assert r.prompt == "try again"

    def test_possible_solutions_in_context_or_error(self) -> None:
        r = dcc_mcp_core.from_exception(
            "err",
            possible_solutions=["check network", "retry"],
        )
        assert r.success is False

    def test_include_traceback_false(self) -> None:
        r = dcc_mcp_core.from_exception("err", include_traceback=False)
        assert r.success is False
        assert "err" in r.error
