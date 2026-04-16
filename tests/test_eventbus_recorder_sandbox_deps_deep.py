"""Deep tests for EventBus, ToolRecorder, SandboxPolicy/Context, and skill dependency functions.

Coverage targets:
- EventBus: subscribe/publish/unsubscribe, concurrent multi-thread, isolation
- ToolRecorder: scope, start/finish, metrics attrs, reset, multi-action
- TimingMiddleware / AuditMiddleware: from pipeline.add_timing / add_audit
- resolve_dependencies / expand_transitive_dependencies / validate_dependencies
- SandboxPolicy: allow/deny/path/read-only/timeout/max-actions
- SandboxContext: is_allowed/is_path_allowed/execute_json, AuditLog entries
- Concurrent SandboxContext execution safety
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed
import contextlib
import threading

import pytest

from dcc_mcp_core import EventBus
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRecorder
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import expand_transitive_dependencies
from dcc_mcp_core import resolve_dependencies
from dcc_mcp_core import scan_and_load
from dcc_mcp_core import validate_dependencies

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def make_skill(name: str, depends: list[str] | None = None, dcc: str = "python") -> SkillMetadata:
    return SkillMetadata(
        name=name,
        dcc=dcc,
        version="1.0.0",
        description=f"test skill {name}",
        scripts=[],
        tags=[],
        metadata_files=[],
        skill_path="",
        depends=depends or [],
    )


def make_pipeline_with_action(action_name: str = "act"):
    reg = ToolRegistry()
    reg.register(action_name, description="", category="test")
    disp = ToolDispatcher(reg)
    disp.register_handler(action_name, lambda p: {"result": 42})
    return ToolPipeline(disp), action_name


# ===========================================================================
# TestEventBus
# ===========================================================================


class TestEventBus:
    """Tests for EventBus subscribe/publish/unsubscribe."""

    class TestSubscribePublish:
        def test_subscribe_returns_int_id(self) -> None:
            eb = EventBus()
            sid = eb.subscribe("ev", lambda **kw: None)
            assert isinstance(sid, int)
            assert sid > 0

        def test_publish_delivers_kwargs_to_subscriber(self) -> None:
            eb = EventBus()
            received: list[dict] = []
            eb.subscribe("topic", lambda **kw: received.append(kw))
            eb.publish("topic", x=1, y="hello")
            assert received == [{"x": 1, "y": "hello"}]

        def test_publish_no_kwargs(self) -> None:
            eb = EventBus()
            calls: list[dict] = []
            eb.subscribe("no_args_ev", lambda **kw: calls.append(kw))
            eb.publish("no_args_ev")
            assert calls == [{}]

        def test_multiple_subscribers_same_event(self) -> None:
            eb = EventBus()
            a: list = []
            b: list = []
            eb.subscribe("ev", lambda **kw: a.append(kw))
            eb.subscribe("ev", lambda **kw: b.append(kw))
            eb.publish("ev", val=99)
            assert a == [{"val": 99}]
            assert b == [{"val": 99}]

        def test_different_events_are_isolated(self) -> None:
            eb = EventBus()
            ev1: list = []
            ev2: list = []
            eb.subscribe("ev1", lambda **kw: ev1.append(kw))
            eb.subscribe("ev2", lambda **kw: ev2.append(kw))
            eb.publish("ev1", x=1)
            assert ev1 == [{"x": 1}]
            assert ev2 == []

        def test_publish_unknown_event_no_error(self) -> None:
            eb = EventBus()
            eb.publish("no_subscribers_here", key="val")

        def test_publish_multiple_times(self) -> None:
            eb = EventBus()
            counts: list = []
            eb.subscribe("repeated", lambda **kw: counts.append(kw.get("n")))
            for i in range(5):
                eb.publish("repeated", n=i)
            assert counts == [0, 1, 2, 3, 4]

        def test_subscriber_ids_are_unique(self) -> None:
            eb = EventBus()
            ids = [eb.subscribe("ev", lambda **kw: None) for _ in range(10)]
            assert len(set(ids)) == 10

    class TestUnsubscribe:
        def test_unsubscribe_returns_true(self) -> None:
            eb = EventBus()
            sid = eb.subscribe("ev", lambda **kw: None)
            assert eb.unsubscribe("ev", sid) is True

        def test_unsubscribe_stops_delivery(self) -> None:
            eb = EventBus()
            received: list = []
            sid = eb.subscribe("ev", lambda **kw: received.append(kw))
            eb.publish("ev", x=1)
            eb.unsubscribe("ev", sid)
            eb.publish("ev", x=2)
            assert len(received) == 1
            assert received[0] == {"x": 1}

        def test_unsubscribe_unknown_id_returns_false(self) -> None:
            eb = EventBus()
            assert eb.unsubscribe("ev", 99999) is False

        def test_unsubscribe_only_removes_one_subscriber(self) -> None:
            eb = EventBus()
            calls_a: list = []
            calls_b: list = []
            sid_a = eb.subscribe("ev", lambda **kw: calls_a.append(1))
            eb.subscribe("ev", lambda **kw: calls_b.append(1))
            eb.unsubscribe("ev", sid_a)
            eb.publish("ev")
            assert calls_a == []
            assert calls_b == [1]

        def test_double_unsubscribe_second_returns_false(self) -> None:
            eb = EventBus()
            sid = eb.subscribe("ev", lambda **kw: None)
            assert eb.unsubscribe("ev", sid) is True
            assert eb.unsubscribe("ev", sid) is False

    class TestConcurrent:
        def test_concurrent_subscribe_and_publish(self) -> None:
            eb = EventBus()
            collected: list[int] = []
            lock = threading.Lock()
            n_threads = 10

            def worker(i: int) -> None:
                def cb(**kw: object) -> None:
                    with lock:
                        collected.append(kw.get("v", -1))  # type: ignore[arg-type]

                eb.subscribe(f"ch_{i}", cb)
                eb.publish(f"ch_{i}", v=i)

            with ThreadPoolExecutor(max_workers=n_threads) as pool:
                list(pool.map(worker, range(n_threads)))

            assert sorted(collected) == list(range(n_threads))

        def test_concurrent_publish_to_shared_event(self) -> None:
            eb = EventBus()
            results: list[int] = []
            lock = threading.Lock()

            def cb(**kw: object) -> None:
                with lock:
                    results.append(kw.get("n"))  # type: ignore[arg-type]

            eb.subscribe("shared", cb)
            n = 20
            with ThreadPoolExecutor(max_workers=n) as pool:
                futs = [pool.submit(eb.publish, "shared", n=i) for i in range(n)]
                for f in as_completed(futs):
                    f.result()

            assert sorted(results) == list(range(n))

        def test_concurrent_subscribe_unsubscribe_no_crash(self) -> None:
            eb = EventBus()
            sids: list[int] = []
            lock = threading.Lock()

            def subscribe_worker() -> None:
                sid = eb.subscribe("chaos", lambda **kw: None)
                with lock:
                    sids.append(sid)

            with ThreadPoolExecutor(max_workers=10) as pool:
                list(pool.map(lambda _: subscribe_worker(), range(20)))

            for sid in sids:
                eb.unsubscribe("chaos", sid)


# ===========================================================================
# TestActionRecorder
# ===========================================================================


class TestActionRecorder:
    """Tests for ToolRecorder + RecordingGuard + ToolMetrics."""

    class TestBasicRecording:
        def test_create_recorder_with_scope(self) -> None:
            ar = ToolRecorder("scope1")
            assert ar is not None

        def test_start_returns_recording_guard(self) -> None:
            ar = ToolRecorder("scope1")
            guard = ar.start("my_action", "maya")
            assert guard is not None
            guard.finish(True)

        def test_finish_success_reflected_in_metrics(self) -> None:
            ar = ToolRecorder("scope_success")
            guard = ar.start("act_a", "maya")
            guard.finish(True)
            m = ar.metrics("act_a")
            assert m.invocation_count == 1
            assert m.success_count == 1
            assert m.failure_count == 0

        def test_finish_failure_reflected_in_metrics(self) -> None:
            ar = ToolRecorder("scope_fail")
            guard = ar.start("act_b", "maya")
            guard.finish(False)
            m = ar.metrics("act_b")
            assert m.invocation_count == 1
            assert m.success_count == 0
            assert m.failure_count == 1

        def test_success_rate_1_when_all_success(self) -> None:
            ar = ToolRecorder("scope_rate")
            for _ in range(3):
                ar.start("act_rate", "maya").finish(True)
            m = ar.metrics("act_rate")
            assert m.success_rate() == pytest.approx(1.0)

        def test_success_rate_0_when_all_failure(self) -> None:
            ar = ToolRecorder("scope_rate2")
            for _ in range(3):
                ar.start("act_fail", "maya").finish(False)
            m = ar.metrics("act_fail")
            assert m.success_rate() == pytest.approx(0.0)

        def test_mixed_success_failure_rate(self) -> None:
            ar = ToolRecorder("scope_mixed")
            ar.start("mix", "maya").finish(True)
            ar.start("mix", "maya").finish(False)
            m = ar.metrics("mix")
            assert m.invocation_count == 2
            assert m.success_count == 1
            assert m.failure_count == 1
            assert m.success_rate() == pytest.approx(0.5)

        def test_action_name_field(self) -> None:
            ar = ToolRecorder("scope_name")
            ar.start("named_action", "maya").finish(True)
            m = ar.metrics("named_action")
            assert m.action_name == "named_action"

        def test_avg_duration_ms_is_float(self) -> None:
            ar = ToolRecorder("scope_dur")
            ar.start("dur_act", "maya").finish(True)
            m = ar.metrics("dur_act")
            assert isinstance(m.avg_duration_ms, float)
            assert m.avg_duration_ms >= 0.0

        def test_p95_p99_duration_populated(self) -> None:
            ar = ToolRecorder("scope_pct")
            for _ in range(5):
                ar.start("pct_act", "maya").finish(True)
            m = ar.metrics("pct_act")
            assert m.p95_duration_ms >= 0.0
            assert m.p99_duration_ms >= 0.0

    class TestAllMetricsAndReset:
        def test_all_metrics_returns_list(self) -> None:
            ar = ToolRecorder("scope_all")
            ar.start("a1", "maya").finish(True)
            ar.start("a2", "maya").finish(True)
            all_m = ar.all_metrics()
            assert isinstance(all_m, list)
            assert len(all_m) >= 2
            names = [m.action_name for m in all_m]
            assert "a1" in names
            assert "a2" in names

        def test_reset_clears_metrics(self) -> None:
            ar = ToolRecorder("scope_reset")
            ar.start("reset_act", "maya").finish(True)
            ar.reset()
            all_m = ar.all_metrics()
            assert all_m == []

        def test_multiple_dccs_tracked_separately(self) -> None:
            """Different DCC names should result in separate metric entries."""
            ar = ToolRecorder("scope_multi_dcc")
            ar.start("act_x", "maya").finish(True)
            ar.start("act_x", "blender").finish(False)
            all_m = ar.all_metrics()
            assert len(all_m) >= 1

    class TestConcurrent:
        def test_concurrent_recording_no_crash(self) -> None:
            ar = ToolRecorder("scope_concurrent")
            n = 20

            def work(i: int) -> None:
                guard = ar.start(f"action_{i % 5}", "maya")
                guard.finish(i % 2 == 0)

            with ThreadPoolExecutor(max_workers=n) as pool:
                list(pool.map(work, range(n)))

            all_m = ar.all_metrics()
            total = sum(m.invocation_count for m in all_m)
            assert total == n


# ===========================================================================
# TestTimingAndAuditMiddleware
# ===========================================================================


class TestTimingAndAuditMiddleware:
    """Tests for TimingMiddleware and AuditMiddleware from ToolPipeline."""

    class TestTimingMiddleware:
        def test_last_elapsed_ms_none_before_dispatch(self) -> None:
            pipeline, act = make_pipeline_with_action("timing_act")
            timing = pipeline.add_timing()
            assert timing.last_elapsed_ms(act) is None

        def test_last_elapsed_ms_int_after_dispatch(self) -> None:
            pipeline, act = make_pipeline_with_action("timing_act2")
            timing = pipeline.add_timing()
            pipeline.dispatch(act, "{}")
            elapsed = timing.last_elapsed_ms(act)
            assert isinstance(elapsed, int)
            assert elapsed >= 0

        def test_timing_updates_on_redispatch(self) -> None:
            pipeline, act = make_pipeline_with_action("timing_redispatch")
            timing = pipeline.add_timing()
            pipeline.dispatch(act, "{}")
            first = timing.last_elapsed_ms(act)
            pipeline.dispatch(act, "{}")
            second = timing.last_elapsed_ms(act)
            assert first is not None
            assert second is not None

        def test_timing_unknown_action_returns_none(self) -> None:
            pipeline, _act = make_pipeline_with_action("timing_known")
            timing = pipeline.add_timing()
            assert timing.last_elapsed_ms("unknown_action") is None

    class TestAuditMiddleware:
        def test_record_count_zero_initially(self) -> None:
            pipeline, _act = make_pipeline_with_action("audit_init")
            audit = pipeline.add_audit()
            assert audit.record_count() == 0

        def test_record_count_increments_on_dispatch(self) -> None:
            pipeline, act = make_pipeline_with_action("audit_inc")
            audit = pipeline.add_audit()
            pipeline.dispatch(act, "{}")
            assert audit.record_count() == 1
            pipeline.dispatch(act, "{}")
            assert audit.record_count() == 2

        def test_records_has_required_keys(self) -> None:
            pipeline, act = make_pipeline_with_action("audit_keys")
            audit = pipeline.add_audit()
            pipeline.dispatch(act, "{}")
            recs = audit.records()
            assert len(recs) == 1
            rec = recs[0]
            assert "action" in rec
            assert "success" in rec
            assert "timestamp_ms" in rec

        def test_records_for_action_filters_correctly(self) -> None:
            reg = ToolRegistry()
            reg.register("aa", description="", category="test")
            reg.register("bb", description="", category="test")
            disp = ToolDispatcher(reg)
            disp.register_handler("aa", lambda p: {"r": 1})
            disp.register_handler("bb", lambda p: {"r": 2})
            pipeline = ToolPipeline(disp)
            audit = pipeline.add_audit()
            pipeline.dispatch("aa", "{}")
            pipeline.dispatch("bb", "{}")
            pipeline.dispatch("aa", "{}")
            aa_recs = audit.records_for_action("aa")
            bb_recs = audit.records_for_action("bb")
            assert len(aa_recs) == 2
            assert len(bb_recs) == 1

        def test_clear_resets_records(self) -> None:
            pipeline, act = make_pipeline_with_action("audit_clear")
            audit = pipeline.add_audit()
            pipeline.dispatch(act, "{}")
            audit.clear()
            assert audit.record_count() == 0
            assert audit.records() == []

        def test_record_success_field(self) -> None:
            pipeline, act = make_pipeline_with_action("audit_success_field")
            audit = pipeline.add_audit()
            pipeline.dispatch(act, "{}")
            recs = audit.records()
            assert recs[0]["success"] is True

        def test_concurrent_audit_records_count(self) -> None:
            pipeline, act = make_pipeline_with_action("audit_concurrent")
            audit = pipeline.add_audit()
            n = 20
            with ThreadPoolExecutor(max_workers=n) as pool:
                futs = [pool.submit(pipeline.dispatch, act, "{}") for _ in range(n)]
                for f in as_completed(futs):
                    f.result()
            assert audit.record_count() == n


# ===========================================================================
# TestSkillDependencies
# ===========================================================================


class TestSkillDependencies:
    """Tests for resolve_dependencies, expand_transitive_dependencies, validate_dependencies."""

    class TestResolveDependencies:
        def test_resolve_empty_list(self) -> None:
            result = resolve_dependencies([])
            assert result == []

        def test_resolve_no_deps(self) -> None:
            a = make_skill("a")
            b = make_skill("b")
            result = resolve_dependencies([a, b])
            names = [s.name for s in result]
            assert "a" in names
            assert "b" in names

        def test_resolve_single_dependency(self) -> None:
            a = make_skill("a")
            b = make_skill("b", ["a"])
            result = resolve_dependencies([a, b])
            names = [s.name for s in result]
            # a must come before b
            assert names.index("a") < names.index("b")

        def test_resolve_chain_dependency(self) -> None:
            a = make_skill("a")
            b = make_skill("b", ["a"])
            c = make_skill("c", ["b"])
            result = resolve_dependencies([a, b, c])
            names = [s.name for s in result]
            assert names.index("a") < names.index("b")
            assert names.index("b") < names.index("c")

        def test_resolve_returns_skill_metadata_objects(self) -> None:
            a = make_skill("a")
            result = resolve_dependencies([a])
            assert all(isinstance(s, SkillMetadata) for s in result)

        def test_resolve_with_real_skills(self) -> None:
            skills, _ = scan_and_load(["examples/skills"])
            result = resolve_dependencies(skills)
            assert len(result) == len(skills)
            names = [s.name for s in result]
            # maya-pipeline depends on maya-geometry and usd-tools
            if "maya-pipeline" in names and "maya-geometry" in names:
                assert names.index("maya-geometry") < names.index("maya-pipeline")
            if "maya-pipeline" in names and "usd-tools" in names:
                assert names.index("usd-tools") < names.index("maya-pipeline")

    class TestExpandTransitiveDependencies:
        def test_no_deps_returns_empty(self) -> None:
            a = make_skill("a")
            result = expand_transitive_dependencies([a], "a")
            assert result == []

        def test_direct_dep_only(self) -> None:
            a = make_skill("a")
            b = make_skill("b", ["a"])
            result = expand_transitive_dependencies([a, b], "b")
            assert "a" in result

        def test_transitive_chain(self) -> None:
            a = make_skill("a")
            b = make_skill("b", ["a"])
            c = make_skill("c", ["b"])
            result = expand_transitive_dependencies([a, b, c], "c")
            assert "a" in result
            assert "b" in result
            assert "c" not in result

        def test_diamond_dependency(self) -> None:
            """A <- B, A <- C, D <- [B, C]."""
            a = make_skill("a")
            b = make_skill("b", ["a"])
            c = make_skill("c", ["a"])
            d = make_skill("d", ["b", "c"])
            result = expand_transitive_dependencies([a, b, c, d], "d")
            assert "a" in result
            assert "b" in result
            assert "c" in result

        def test_expand_unknown_skill_returns_empty(self) -> None:
            a = make_skill("a")
            result = expand_transitive_dependencies([a], "nonexistent_skill")
            assert result == []

        def test_expand_real_maya_pipeline(self) -> None:
            skills, _ = scan_and_load(["examples/skills"])
            result = expand_transitive_dependencies(skills, "maya-pipeline")
            assert "maya-geometry" in result
            assert "usd-tools" in result

    class TestValidateDependencies:
        def test_empty_list_no_errors(self) -> None:
            errors = validate_dependencies([])
            assert isinstance(errors, list)
            assert errors == []

        def test_valid_skills_no_errors(self) -> None:
            a = make_skill("a")
            b = make_skill("b", ["a"])
            c = make_skill("c", ["b"])
            errors = validate_dependencies([a, b, c])
            assert errors == []

        def test_real_skills_no_errors(self) -> None:
            skills, _ = scan_and_load(["examples/skills"])
            errors = validate_dependencies(skills)
            assert errors == []

        def test_returns_list_type(self) -> None:
            a = make_skill("a")
            result = validate_dependencies([a])
            assert isinstance(result, list)


# ===========================================================================
# TestSandboxPolicy
# ===========================================================================


class TestSandboxPolicy:
    """Tests for SandboxPolicy configuration methods."""

    class TestDefaults:
        def test_policy_default_not_readonly(self) -> None:
            policy = SandboxPolicy()
            assert policy.is_read_only is False

        def test_policy_set_read_only(self) -> None:
            policy = SandboxPolicy()
            policy.set_read_only(True)
            assert policy.is_read_only is True

        def test_policy_set_read_only_false(self) -> None:
            policy = SandboxPolicy()
            policy.set_read_only(True)
            policy.set_read_only(False)
            assert policy.is_read_only is False

    class TestAllowDenyActions:
        def test_allow_actions_makes_them_available(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["a1", "a2"])
            ctx = SandboxContext(policy)
            assert ctx.is_allowed("a1") is True
            assert ctx.is_allowed("a2") is True
            assert ctx.is_allowed("unknown") is False

        def test_deny_actions_blocks_them(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["a1", "a2"])
            policy.deny_actions(["a2"])
            ctx = SandboxContext(policy)
            assert ctx.is_allowed("a1") is True
            assert ctx.is_allowed("a2") is False

        def test_empty_allow_list_denies_all(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions([])
            ctx = SandboxContext(policy)
            assert ctx.is_allowed("anything") is False

    class TestAllowPaths:
        def test_allow_paths_permits_matching(self) -> None:
            # allow_paths uses prefix matching; exact path works
            policy = SandboxPolicy()
            policy.allow_paths(["/tmp"])
            ctx = SandboxContext(policy)
            assert ctx.is_path_allowed("/tmp") is True

        def test_allow_paths_denies_non_listed(self) -> None:
            policy = SandboxPolicy()
            policy.allow_paths(["/tmp"])
            ctx = SandboxContext(policy)
            assert ctx.is_path_allowed("/etc") is False
            assert ctx.is_path_allowed("/home") is False

        def test_no_paths_configured_allows_all(self) -> None:
            # When no paths are configured, sandbox is open (allow-all)
            policy = SandboxPolicy()
            ctx = SandboxContext(policy)
            assert ctx.is_path_allowed("/tmp") is True
            assert ctx.is_path_allowed("/etc") is True

    class TestTimeoutAndMaxActions:
        def test_set_timeout_ms_no_error(self) -> None:
            policy = SandboxPolicy()
            policy.set_timeout_ms(5000)

        def test_set_max_actions_no_error(self) -> None:
            policy = SandboxPolicy()
            policy.set_max_actions(50)

        def test_independent_policies_dont_interfere(self) -> None:
            p1 = SandboxPolicy()
            p1.allow_actions(["a"])
            p2 = SandboxPolicy()
            p2.allow_actions(["b"])
            ctx1 = SandboxContext(p1)
            ctx2 = SandboxContext(p2)
            assert ctx1.is_allowed("a") is True
            assert ctx1.is_allowed("b") is False
            assert ctx2.is_allowed("b") is True
            assert ctx2.is_allowed("a") is False


# ===========================================================================
# TestSandboxContext
# ===========================================================================


class TestSandboxContext:
    """Tests for SandboxContext execution and AuditLog."""

    class TestExecuteJson:
        def test_allowed_action_executes_returns_str(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["do_work"])
            ctx = SandboxContext(policy)
            result = ctx.execute_json("do_work", "{}")
            assert isinstance(result, str)

        def test_denied_action_raises_runtime_error(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["safe_action"])
            ctx = SandboxContext(policy)
            with pytest.raises(RuntimeError):
                ctx.execute_json("exec_arbitrary", "{}")

        def test_action_count_increments_on_success(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["count_me"])
            ctx = SandboxContext(policy)
            assert ctx.action_count == 0
            ctx.execute_json("count_me", "{}")
            assert ctx.action_count == 1
            ctx.execute_json("count_me", "{}")
            assert ctx.action_count == 2

        def test_action_count_not_incremented_on_denial(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["safe"])
            ctx = SandboxContext(policy)
            ctx.execute_json("safe", "{}")
            with contextlib.suppress(RuntimeError):
                ctx.execute_json("denied", "{}")
            assert ctx.action_count == 1

        def test_set_actor_no_error(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["act"])
            ctx = SandboxContext(policy)
            ctx.set_actor("agent_007")
            ctx.execute_json("act", "{}")

    class TestAuditLog:
        def test_audit_log_entries_after_execute(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["work"])
            ctx = SandboxContext(policy)
            ctx.execute_json("work", "{}")
            al = ctx.audit_log
            entries = al.entries()
            assert len(entries) >= 1

        def test_audit_entry_fields(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["work2"])
            ctx = SandboxContext(policy)
            ctx.execute_json("work2", "{}")
            entry = ctx.audit_log.entries()[0]
            assert entry.action == "work2"
            assert isinstance(entry.timestamp_ms, int)
            assert entry.timestamp_ms > 0
            assert entry.duration_ms >= 0

        def test_audit_log_contains_denied_entry(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["safe2"])
            ctx = SandboxContext(policy)
            with contextlib.suppress(RuntimeError):
                ctx.execute_json("denied_action", "{}")
            al = ctx.audit_log
            all_entries = al.entries()
            assert len(all_entries) >= 1

        def test_denials_method(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["ok"])
            ctx = SandboxContext(policy)
            ctx.execute_json("ok", "{}")
            with contextlib.suppress(RuntimeError):
                ctx.execute_json("bad", "{}")
            denials = ctx.audit_log.denials()
            assert isinstance(denials, list)

        def test_successes_method(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["good"])
            ctx = SandboxContext(policy)
            ctx.execute_json("good", "{}")
            successes = ctx.audit_log.successes()
            assert isinstance(successes, list)
            assert len(successes) >= 1

        def test_to_json_returns_string(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["j_act"])
            ctx = SandboxContext(policy)
            ctx.execute_json("j_act", "{}")
            j = ctx.audit_log.to_json()
            assert isinstance(j, str)
            assert len(j) > 0

        def test_entries_for_action_filter(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["x_act", "y_act"])
            ctx = SandboxContext(policy)
            ctx.execute_json("x_act", "{}")
            ctx.execute_json("y_act", "{}")
            ctx.execute_json("x_act", "{}")
            x_entries = ctx.audit_log.entries_for_action("x_act")
            y_entries = ctx.audit_log.entries_for_action("y_act")
            assert len(x_entries) == 2
            assert len(y_entries) == 1

        def test_actor_stored_in_entries(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["actor_act"])
            ctx = SandboxContext(policy)
            ctx.set_actor("my_agent")
            ctx.execute_json("actor_act", "{}")
            entry = ctx.audit_log.entries()[0]
            assert entry.actor == "my_agent"

    class TestConcurrent:
        def test_concurrent_execute_no_crash(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions([f"action_{i}" for i in range(10)])
            ctx = SandboxContext(policy)
            n = 20

            def work(i: int) -> None:
                ctx.execute_json(f"action_{i % 10}", "{}")

            with ThreadPoolExecutor(max_workers=n) as pool:
                list(pool.map(work, range(n)))

            assert ctx.action_count == n

        def test_concurrent_denied_and_allowed(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["allowed_conc"])
            ctx = SandboxContext(policy)

            allowed_count = 0
            denied_count = 0
            lock = threading.Lock()

            def work(i: int) -> None:
                nonlocal allowed_count, denied_count
                if i % 2 == 0:
                    ctx.execute_json("allowed_conc", "{}")
                    with lock:
                        allowed_count += 1
                else:
                    try:
                        ctx.execute_json("denied_conc", "{}")
                    except RuntimeError:
                        with lock:
                            denied_count += 1

            with ThreadPoolExecutor(max_workers=20) as pool:
                list(pool.map(work, range(20)))

            assert allowed_count == 10
            assert denied_count == 10
            assert ctx.action_count == 10

        def test_independent_contexts_dont_share_state(self) -> None:
            policy = SandboxPolicy()
            policy.allow_actions(["act"])
            ctx_a = SandboxContext(policy)
            ctx_b = SandboxContext(policy)

            ctx_a.execute_json("act", "{}")
            ctx_a.execute_json("act", "{}")

            assert ctx_a.action_count == 2
            assert ctx_b.action_count == 0
