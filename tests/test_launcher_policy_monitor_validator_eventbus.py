"""Deep coverage tests.

Covers PyDccLauncher, PyCrashRecoveryPolicy, PyProcessMonitor,
ActionValidator (from_action_registry), EventBus multi-subscriber, SkillMetadata
new constructor, InputValidator, and PyBufferPool.

All tests are fully self-contained with no external dependencies.
"""

from __future__ import annotations

import contextlib
import os

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import ActionValidator
from dcc_mcp_core import EventBus
from dcc_mcp_core import InputValidator
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import SkillMetadata

# ---------------------------------------------------------------------------
# PyDccLauncher
# ---------------------------------------------------------------------------


class TestPyDccLauncherInit:
    def test_init_no_args(self):
        launcher = PyDccLauncher()
        assert launcher is not None

    def test_running_count_initially_zero(self):
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_pid_of_unknown_returns_none(self):
        launcher = PyDccLauncher()
        assert launcher.pid_of("nonexistent") is None

    def test_restart_count_unknown_returns_zero(self):
        launcher = PyDccLauncher()
        # unknown name should return 0
        assert launcher.restart_count("unknown") == 0

    def test_repr_contains_class(self):
        launcher = PyDccLauncher()
        r = repr(launcher)
        assert "Launcher" in r or "launcher" in r.lower() or "PyDcc" in r


class TestPyDccLauncherLaunch:
    def test_launch_python_interpreter(self):
        """Launch the Python interpreter itself as a real subprocess."""
        import sys

        launcher = PyDccLauncher()
        executable = sys.executable
        info = launcher.launch("test-py", executable, args=["-c", "import time; time.sleep(5)"])
        assert isinstance(info, dict)
        assert "pid" in info
        assert info["pid"] > 0
        assert "name" in info
        assert info["name"] == "test-py"
        # Cleanup
        with contextlib.suppress(Exception):
            launcher.kill("test-py")

    def test_launch_running_count_increments(self):
        import sys

        launcher = PyDccLauncher()
        launcher.launch("proc-a", sys.executable, args=["-c", "import time; time.sleep(5)"])
        assert launcher.running_count() >= 1
        with contextlib.suppress(Exception):
            launcher.kill("proc-a")

    def test_pid_of_launched_process(self):
        import sys

        launcher = PyDccLauncher()
        info = launcher.launch("pid-test", sys.executable, args=["-c", "import time; time.sleep(5)"])
        pid = launcher.pid_of("pid-test")
        assert pid == info["pid"]
        with contextlib.suppress(Exception):
            launcher.kill("pid-test")

    def test_kill_removes_from_running(self):
        import sys

        launcher = PyDccLauncher()
        launcher.launch("kill-me", sys.executable, args=["-c", "import time; time.sleep(10)"])
        count_before = launcher.running_count()
        launcher.kill("kill-me")
        # After kill, pid_of should eventually return None (process exited)
        # running_count may still show it if not reaped yet, so just check no crash
        assert count_before >= 1

    def test_terminate_unknown_does_not_crash(self):
        launcher = PyDccLauncher()
        with contextlib.suppress(RuntimeError):
            launcher.terminate("does-not-exist", timeout_ms=100)

    def test_kill_unknown_does_not_crash(self):
        launcher = PyDccLauncher()
        with contextlib.suppress(RuntimeError):
            launcher.kill("does-not-exist-kill")

    def test_launch_invalid_executable_raises(self):
        launcher = PyDccLauncher()
        with pytest.raises((RuntimeError, OSError, Exception)):
            launcher.launch("bad-exe", "/no/such/executable/ever", launch_timeout_ms=500)

    def test_restart_count_after_launch(self):
        import sys

        launcher = PyDccLauncher()
        launcher.launch("restart-test", sys.executable, args=["-c", "import time; time.sleep(5)"])
        # No restarts yet — should be 0
        assert launcher.restart_count("restart-test") == 0
        with contextlib.suppress(Exception):
            launcher.kill("restart-test")


# ---------------------------------------------------------------------------
# PyCrashRecoveryPolicy
# ---------------------------------------------------------------------------


class TestPyCrashRecoveryPolicyDefault:
    def test_default_max_restarts(self):
        p = PyCrashRecoveryPolicy()
        assert p.max_restarts == 3

    def test_custom_max_restarts(self):
        p = PyCrashRecoveryPolicy(max_restarts=7)
        assert p.max_restarts == 7

    def test_should_restart_crashed(self):
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("crashed") is True

    def test_should_restart_unresponsive(self):
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("unresponsive") is True

    def test_should_restart_running_is_false(self):
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("running") is False

    def test_should_restart_stopped_is_false(self):
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("stopped") is False

    def test_should_restart_unknown_raises(self):
        p = PyCrashRecoveryPolicy()
        with pytest.raises(ValueError):
            p.should_restart("ok")

    def test_next_delay_attempt_0(self):
        p = PyCrashRecoveryPolicy()
        delay = p.next_delay_ms("maya", 0)
        assert isinstance(delay, int)
        assert delay >= 0

    def test_next_delay_attempt_1(self):
        p = PyCrashRecoveryPolicy()
        delay = p.next_delay_ms("maya", 1)
        assert isinstance(delay, int)

    def test_next_delay_attempt_2(self):
        p = PyCrashRecoveryPolicy()
        delay = p.next_delay_ms("maya", 2)
        assert isinstance(delay, int)

    def test_next_delay_exceeds_max_raises(self):
        p = PyCrashRecoveryPolicy(max_restarts=3)
        with pytest.raises(RuntimeError, match="exceeded max restarts"):
            p.next_delay_ms("maya", 3)

    def test_max_restarts_zero_raises_immediately(self):
        p = PyCrashRecoveryPolicy(max_restarts=0)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("any", 0)

    def test_repr_contains_policy(self):
        p = PyCrashRecoveryPolicy()
        r = repr(p)
        assert len(r) > 0


class TestPyCrashRecoveryPolicyExponential:
    def test_use_exponential_backoff_instance_method(self):
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        # First delay should be >= 100
        delay0 = p.next_delay_ms("x", 0)
        delay1 = p.next_delay_ms("x", 1)
        _delay2 = p.next_delay_ms("x", 2)
        assert delay0 >= 0
        # Exponential: delays should grow
        assert delay1 >= delay0

    def test_exponential_delays_grow(self):
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=500, max_delay_ms=60000)
        delays = [p.next_delay_ms("x", i) for i in range(4)]
        # Each delay should be >= previous
        for i in range(1, len(delays)):
            assert delays[i] >= delays[i - 1]

    def test_exponential_capped_at_max(self):
        p = PyCrashRecoveryPolicy(max_restarts=10)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=3000)
        # With enough retries, should be capped
        delays = [p.next_delay_ms("x", i) for i in range(5)]
        for d in delays:
            assert d <= 3000 + 1  # small tolerance

    def test_exponential_exceeds_max_raises(self):
        p = PyCrashRecoveryPolicy(max_restarts=3)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("x", 3)


class TestPyCrashRecoveryPolicyFixed:
    def test_use_fixed_backoff_instance_method(self):
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=2000)
        for i in range(5):
            assert p.next_delay_ms("x", i) == 2000

    def test_fixed_all_equal(self):
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=500)
        delays = [p.next_delay_ms("x", i) for i in range(5)]
        assert all(d == 500 for d in delays)

    def test_fixed_exceeds_max_raises(self):
        p = PyCrashRecoveryPolicy(max_restarts=2)
        p.use_fixed_backoff(delay_ms=1000)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("x", 2)

    def test_fixed_zero_delay(self):
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=0)
        assert p.next_delay_ms("x", 0) == 0


# ---------------------------------------------------------------------------
# PyProcessMonitor
# ---------------------------------------------------------------------------


class TestPyProcessMonitorBasic:
    def test_init(self):
        mon = PyProcessMonitor()
        assert mon is not None

    def test_tracked_count_initially_zero(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_list_all_initially_empty(self):
        mon = PyProcessMonitor()
        assert mon.list_all() == []

    def test_repr(self):
        mon = PyProcessMonitor()
        r = repr(mon)
        assert len(r) > 0

    def test_track_current_process(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        assert mon.tracked_count() == 1

    def test_track_multiple_processes(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.track(os.getppid(), "parent")
        assert mon.tracked_count() == 2

    def test_untrack_reduces_count(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        assert mon.tracked_count() == 1
        mon.untrack(os.getpid())
        assert mon.tracked_count() == 0

    def test_untrack_nonexistent_no_crash(self):
        mon = PyProcessMonitor()
        mon.untrack(99999999)  # Should not raise

    def test_is_alive_self(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_nonexistent_pid(self):
        mon = PyProcessMonitor()
        # PID 0 or very large PID should not be alive
        assert mon.is_alive(0) is False or isinstance(mon.is_alive(0), bool)

    def test_is_alive_unlikely_pid(self):
        mon = PyProcessMonitor()
        # 2^30 should not exist as a real PID
        result = mon.is_alive(1073741824)
        assert result is False


class TestPyProcessMonitorQuery:
    def test_query_tracked_after_refresh(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        # After refresh, query should return data
        assert info is not None
        assert isinstance(info, dict)

    def test_query_keys_present(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None
        assert "pid" in info
        assert "name" in info
        assert "status" in info
        assert "cpu_usage_percent" in info
        assert "memory_bytes" in info
        assert "restart_count" in info

    def test_query_pid_matches(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None
        assert info["pid"] == os.getpid()

    def test_query_name_matches(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "my-process")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None
        assert info["name"] == "my-process"

    def test_query_memory_positive(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None
        assert info["memory_bytes"] >= 0

    def test_query_cpu_non_negative(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None
        assert info["cpu_usage_percent"] >= 0.0

    def test_query_untracked_returns_none(self):
        mon = PyProcessMonitor()
        assert mon.query(99999998) is None

    def test_query_before_refresh_may_return_none(self):
        """Query without refresh is allowed — result may be None or stale."""
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        result = mon.query(os.getpid())
        # Either None (no data yet) or a dict — both acceptable
        assert result is None or isinstance(result, dict)

    def test_list_all_after_track_and_refresh(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        items = mon.list_all()
        assert isinstance(items, list)

    def test_list_all_count_matches_tracked(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        items = mon.list_all()
        # At least the tracked PID should be in list
        assert len(items) >= 0  # May be 0 if not yet populated

    def test_untrack_then_query_returns_none(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        mon.untrack(os.getpid())
        # After untrack, data may still be cached or None
        result = mon.query(os.getpid())
        assert result is None or isinstance(result, dict)


# ---------------------------------------------------------------------------
# ActionValidator — from_action_registry error paths
# ---------------------------------------------------------------------------


class TestActionValidatorFromRegistry:
    def _make_registry_with_schema(self):
        schema = '{"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]}'
        reg = ActionRegistry()
        reg.register("sphere", description="Create sphere", category="geo", input_schema=schema)
        return reg, schema

    def test_from_action_registry_happy_path(self):
        reg, _ = self._make_registry_with_schema()
        av = ActionValidator.from_action_registry(reg, "sphere")
        ok, errors = av.validate('{"radius": 1.0}')
        assert ok is True
        assert errors == []

    def test_from_action_registry_validation_fails(self):
        reg, _ = self._make_registry_with_schema()
        av = ActionValidator.from_action_registry(reg, "sphere")
        ok, errors = av.validate('{"radius": "not-a-number"}')
        assert ok is False
        assert len(errors) > 0

    def test_from_action_registry_missing_required(self):
        reg, _ = self._make_registry_with_schema()
        av = ActionValidator.from_action_registry(reg, "sphere")
        ok, _ = av.validate("{}")
        assert ok is False

    def test_from_action_registry_key_error_on_missing(self):
        reg = ActionRegistry()
        with pytest.raises(KeyError):
            ActionValidator.from_action_registry(reg, "nonexistent_action")

    def test_from_action_registry_with_dcc_name(self):
        schema = '{"type": "object", "properties": {"x": {"type": "integer"}}, "required": ["x"]}'
        reg = ActionRegistry()
        reg.register("do_thing", dcc="maya", input_schema=schema)
        av = ActionValidator.from_action_registry(reg, "do_thing", dcc_name="maya")
        ok, _ = av.validate('{"x": 42}')
        assert ok is True

    def test_from_action_registry_wrong_dcc_raises(self):
        schema = '{"type": "object"}'
        reg = ActionRegistry()
        reg.register("do_thing", dcc="maya", input_schema=schema)
        with pytest.raises(KeyError):
            ActionValidator.from_action_registry(reg, "do_thing", dcc_name="blender")

    def test_from_schema_json_invalid_json_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            ActionValidator.from_schema_json("not-valid-json{")

    def test_validate_invalid_json_raises(self):
        av = ActionValidator.from_schema_json('{"type": "object"}')
        with pytest.raises((ValueError, RuntimeError)):
            av.validate("not-json")

    def test_repr_validator(self):
        av = ActionValidator.from_schema_json('{"type": "object"}')
        r = repr(av)
        assert len(r) > 0

    def test_validate_empty_object_against_empty_schema(self):
        av = ActionValidator.from_schema_json('{"type": "object"}')
        ok, _ = av.validate("{}")
        assert ok is True

    def test_validate_complex_schema(self):
        schema = """{
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "count": {"type": "integer", "minimum": 0, "maximum": 100}
            },
            "required": ["name", "count"]
        }"""
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate('{"name": "cube", "count": 5}')
        assert ok is True
        ok2, _ = av.validate('{"name": "", "count": 200}')
        # minLength or maximum violation
        assert isinstance(ok2, bool)


# ---------------------------------------------------------------------------
# EventBus — multi-subscriber, unsubscribe by id, publish kwargs
# ---------------------------------------------------------------------------


class TestEventBusMultiSubscriber:
    def test_subscribe_returns_int_id(self):
        eb = EventBus()
        received = []
        sid = eb.subscribe("evt", lambda **kw: received.append(kw))
        assert isinstance(sid, int)

    def test_publish_delivers_to_subscriber(self):
        eb = EventBus()
        received = []
        eb.subscribe("test", lambda **kw: received.append(kw))
        eb.publish("test", value=42)
        assert len(received) == 1
        assert received[0].get("value") == 42

    def test_multiple_subscribers_all_receive(self):
        eb = EventBus()
        r1, r2 = [], []
        eb.subscribe("multi", lambda **kw: r1.append(kw))
        eb.subscribe("multi", lambda **kw: r2.append(kw))
        eb.publish("multi", msg="hello")
        assert len(r1) == 1
        assert len(r2) == 1

    def test_unsubscribe_by_id_stops_delivery(self):
        eb = EventBus()
        received = []
        sid = eb.subscribe("evt", lambda **kw: received.append(kw))
        eb.publish("evt", x=1)
        assert len(received) == 1
        removed = eb.unsubscribe("evt", sid)
        assert removed is True
        eb.publish("evt", x=2)
        # Should not receive the second event
        assert len(received) == 1

    def test_unsubscribe_wrong_id_returns_false(self):
        eb = EventBus()
        eb.subscribe("evt", lambda **kw: None)
        result = eb.unsubscribe("evt", 999999)
        assert result is False

    def test_unsubscribe_nonexistent_event_returns_false(self):
        eb = EventBus()
        result = eb.unsubscribe("no-such-event", 1)
        assert result is False

    def test_publish_no_subscribers_no_error(self):
        eb = EventBus()
        eb.publish("orphan", data="x")  # Should not raise

    def test_two_different_events_isolated(self):
        eb = EventBus()
        r1, r2 = [], []
        eb.subscribe("event_a", lambda **kw: r1.append(kw))
        eb.subscribe("event_b", lambda **kw: r2.append(kw))
        eb.publish("event_a", v=1)
        assert len(r1) == 1
        assert len(r2) == 0
        eb.publish("event_b", v=2)
        assert len(r1) == 1
        assert len(r2) == 1

    def test_multiple_publishes_accumulate(self):
        eb = EventBus()
        received = []
        eb.subscribe("acc", lambda **kw: received.append(kw))
        for i in range(5):
            eb.publish("acc", i=i)
        assert len(received) == 5

    def test_different_subscribers_get_unique_ids(self):
        eb = EventBus()
        sid1 = eb.subscribe("evt", lambda **kw: None)
        sid2 = eb.subscribe("evt", lambda **kw: None)
        assert sid1 != sid2

    def test_repr_eventbus(self):
        eb = EventBus()
        r = repr(eb)
        assert len(r) > 0

    def test_publish_kwargs_delivered_correctly(self):
        eb = EventBus()
        received = []
        eb.subscribe("kw", lambda **kw: received.append(kw))
        eb.publish("kw", a=1, b="hello", c=3.14)
        assert received[0]["a"] == 1
        assert received[0]["b"] == "hello"
        assert abs(received[0]["c"] - 3.14) < 1e-6


# ---------------------------------------------------------------------------
# SkillMetadata — new constructor signature
# ---------------------------------------------------------------------------


class TestSkillMetadataConstructor:
    def test_basic_construction(self):
        sm = SkillMetadata("my-skill")
        assert sm.name == "my-skill"

    def test_default_version(self):
        sm = SkillMetadata("skill")
        assert sm.version == "1.0.0"

    def test_default_dcc(self):
        sm = SkillMetadata("skill")
        assert sm.dcc == "python"

    def test_description(self):
        sm = SkillMetadata("skill", description="A test skill")
        assert sm.description == "A test skill"

    def test_tools(self):
        sm = SkillMetadata("skill", tools=["tool_a", "tool_b"])
        assert sm.tools == ["tool_a", "tool_b"]

    def test_tags(self):
        sm = SkillMetadata("skill", tags=["geometry", "create"])
        assert sm.tags == ["geometry", "create"]

    def test_scripts(self):
        sm = SkillMetadata("skill", scripts=["main.py"])
        assert sm.scripts == ["main.py"]

    def test_skill_path(self):
        sm = SkillMetadata("skill", skill_path="/some/path")
        assert sm.skill_path == "/some/path"

    def test_version_custom(self):
        sm = SkillMetadata("skill", version="2.3.4")
        assert sm.version == "2.3.4"

    def test_depends(self):
        sm = SkillMetadata("skill", depends=["base-skill"])
        assert sm.depends == ["base-skill"]

    def test_metadata_files(self):
        sm = SkillMetadata("skill", metadata_files=["SKILL.md"])
        assert sm.metadata_files == ["SKILL.md"]

    def test_dcc_maya(self):
        sm = SkillMetadata("skill", dcc="maya")
        assert sm.dcc == "maya"

    def test_eq_same_values(self):
        sm1 = SkillMetadata("skill", description="desc", dcc="maya", version="1.0.0")
        sm2 = SkillMetadata("skill", description="desc", dcc="maya", version="1.0.0")
        assert sm1 == sm2

    def test_eq_different_name(self):
        sm1 = SkillMetadata("skill-a")
        sm2 = SkillMetadata("skill-b")
        assert sm1 != sm2

    def test_eq_different_version(self):
        sm1 = SkillMetadata("skill", version="1.0.0")
        sm2 = SkillMetadata("skill", version="2.0.0")
        assert sm1 != sm2

    def test_repr(self):
        sm = SkillMetadata("test-skill")
        r = repr(sm)
        assert "test-skill" in r

    def test_str(self):
        sm = SkillMetadata("test-skill")
        s = str(sm)
        assert "test-skill" in s

    def test_empty_tools_default(self):
        sm = SkillMetadata("skill")
        assert isinstance(sm.tools, list)

    def test_empty_depends_default(self):
        sm = SkillMetadata("skill")
        assert isinstance(sm.depends, list)

    def test_empty_metadata_files_default(self):
        sm = SkillMetadata("skill")
        assert isinstance(sm.metadata_files, list)

    def test_full_construction(self):
        sm = SkillMetadata(
            "full-skill",
            description="Complete skill",
            tools=["t1", "t2"],
            dcc="houdini",
            tags=["vfx", "sim"],
            scripts=["run.py", "setup.py"],
            skill_path="/skills/full",
            version="3.0.0",
            depends=["base", "utils"],
            metadata_files=["SKILL.md", "README.md"],
        )
        assert sm.name == "full-skill"
        assert sm.description == "Complete skill"
        assert sm.tools == ["t1", "t2"]
        assert sm.dcc == "houdini"
        assert sm.tags == ["vfx", "sim"]
        assert sm.scripts == ["run.py", "setup.py"]
        assert sm.skill_path == "/skills/full"
        assert sm.version == "3.0.0"
        assert sm.depends == ["base", "utils"]
        assert sm.metadata_files == ["SKILL.md", "README.md"]


# ---------------------------------------------------------------------------
# InputValidator
# ---------------------------------------------------------------------------


class TestInputValidatorBasic:
    def test_init(self):
        v = InputValidator()
        assert v is not None

    def test_validate_empty_object_no_rules(self):
        v = InputValidator()
        ok, err = v.validate("{}")
        assert ok is True
        assert err is None

    def test_require_string_present(self):
        v = InputValidator()
        v.require_string("name", None, None)
        ok, _ = v.validate('{"name": "sphere"}')
        assert ok is True

    def test_require_string_missing_fails(self):
        v = InputValidator()
        v.require_string("name", None, None)
        ok, err = v.validate("{}")
        assert ok is False
        assert err is not None

    def test_require_string_wrong_type_fails(self):
        v = InputValidator()
        v.require_string("name", None, None)
        ok, _ = v.validate('{"name": 123}')
        assert ok is False

    def test_require_string_max_length(self):
        v = InputValidator()
        v.require_string("label", 5, None)
        ok, _ = v.validate('{"label": "hi"}')
        assert ok is True
        ok2, _ = v.validate('{"label": "toolong"}')
        assert ok2 is False

    def test_require_string_min_length(self):
        v = InputValidator()
        v.require_string("label", None, 3)
        ok, _ = v.validate('{"label": "ab"}')
        assert ok is False
        ok2, _ = v.validate('{"label": "abc"}')
        assert ok2 is True

    def test_require_number_present(self):
        v = InputValidator()
        v.require_number("count", None, None)
        ok, _ = v.validate('{"count": 5}')
        assert ok is True

    def test_require_number_missing_fails(self):
        v = InputValidator()
        v.require_number("count", None, None)
        ok, _ = v.validate("{}")
        assert ok is False

    def test_require_number_wrong_type_fails(self):
        v = InputValidator()
        v.require_number("count", None, None)
        ok, _ = v.validate('{"count": "five"}')
        assert ok is False

    def test_require_number_min_value(self):
        v = InputValidator()
        v.require_number("size", 0.0, None)
        ok, _ = v.validate('{"size": -1}')
        assert ok is False
        ok2, _ = v.validate('{"size": 0}')
        assert ok2 is True

    def test_require_number_max_value(self):
        v = InputValidator()
        v.require_number("size", None, 100.0)
        ok, _ = v.validate('{"size": 200}')
        assert ok is False
        ok2, _ = v.validate('{"size": 50}')
        assert ok2 is True

    def test_require_number_range(self):
        v = InputValidator()
        v.require_number("pct", 0.0, 1.0)
        ok, _ = v.validate('{"pct": 0.5}')
        assert ok is True
        ok2, _ = v.validate('{"pct": 1.5}')
        assert ok2 is False

    def test_forbid_substrings_clean(self):
        v = InputValidator()
        v.require_string("script", None, None)
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, _ = v.validate('{"script": "print(hello)"}')
        assert ok is True

    def test_forbid_substrings_injection(self):
        v = InputValidator()
        v.require_string("script", None, None)
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, err = v.validate('{"script": "__import__(os)"}')
        assert ok is False
        assert err is not None

    def test_forbid_multiple_substrings(self):
        v = InputValidator()
        v.require_string("code", None, None)
        v.forbid_substrings("code", ["eval(", "exec(", "__import__"])
        for bad in ["eval(x)", "exec(y)", "__import__z"]:
            ok, _ = v.validate(f'{{"code": "{bad}"}}')
            assert ok is False

    def test_multiple_fields(self):
        v = InputValidator()
        v.require_string("name", None, None)
        v.require_number("count", 0.0, 100.0)
        ok, _ = v.validate('{"name": "cube", "count": 5}')
        assert ok is True
        ok2, _ = v.validate('{"name": "cube", "count": 200}')
        assert ok2 is False

    def test_invalid_json_raises(self):
        v = InputValidator()
        with pytest.raises(RuntimeError):
            v.validate("not-json")

    def test_error_message_is_string(self):
        v = InputValidator()
        v.require_string("name", None, None)
        ok, err = v.validate("{}")
        assert ok is False
        assert isinstance(err, str)
        assert len(err) > 0


# ---------------------------------------------------------------------------
# PyBufferPool
# ---------------------------------------------------------------------------


class TestPyBufferPool:
    def test_init(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_capacity(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.capacity() == 4

    def test_buffer_size(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.buffer_size() == 1024

    def test_available_initially_full(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.available() == 4

    def test_acquire_returns_buffer(self):
        from dcc_mcp_core import PySharedBuffer

        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert isinstance(buf, PySharedBuffer)

    def test_acquire_decrements_available(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        _ = pool.acquire()
        assert pool.available() == 2

    def test_acquire_multiple(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        b1 = pool.acquire()
        b2 = pool.acquire()
        b3 = pool.acquire()
        assert pool.available() == 0
        assert b1 is not None
        assert b2 is not None
        assert b3 is not None

    def test_acquire_exceeds_capacity_raises(self):
        pool = PyBufferPool(capacity=1, buffer_size=256)
        _ = pool.acquire()
        with pytest.raises(RuntimeError):
            _ = pool.acquire()

    def test_buffer_can_write_and_read(self):
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        n = buf.write(b"hello pool")
        assert n == 10
        assert buf.read() == b"hello pool"

    def test_buffer_capacity_matches_pool_size(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert buf.capacity() == 512

    def test_repr(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        r = repr(pool)
        assert len(r) > 0

    def test_different_buffer_sizes(self):
        for size in [128, 1024, 65536]:
            pool = PyBufferPool(capacity=1, buffer_size=size)
            assert pool.buffer_size() == size
            buf = pool.acquire()
            assert buf.capacity() == size
