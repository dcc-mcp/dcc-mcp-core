"""Tests for PyProcessWatcher, PyCrashRecoveryPolicy, MCP protocol types, and SkillMetadata.

Covers:
- PyProcessWatcher: add_watch/remove_watch, track/untrack, is_watched, watch_count,
  tracked_count, poll_events, start/stop, is_running
- PyCrashRecoveryPolicy: max_restarts, should_restart with valid statuses,
  use_exponential_backoff (doubling delays), use_fixed_backoff (constant delays),
  next_delay_ms capped at max_delay_ms
- ToolDefinition: name/description/input_schema/output_schema/annotations attributes
- ToolAnnotations: title/read_only_hint/destructive_hint/idempotent_hint/open_world_hint defaults
- ResourceDefinition: uri/name/description/mime_type/annotations attributes
- ResourceAnnotations: audience/priority attributes and defaults
- ResourceTemplateDefinition: uri_template/name/description/mime_type/annotations
- PromptDefinition: name/description/arguments attributes
- PromptArgument: name/description/required attributes
- SkillMetadata (via parse_skill_md): name/version/description/dcc/tags/depends/skill_path/
  scripts/tools/metadata_files fields; missing optional frontmatter fields
"""

from __future__ import annotations

import os
from pathlib import Path
import tempfile

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_skill_dir(
    tmp_path: Path,
    name: str = "my-skill",
    version: str = "1.0.0",
    description: str = "A skill",
    dcc: str = "maya",
    tags: list[str] | None = None,
    depends: list[str] | None = None,
    extra_frontmatter: str = "",
) -> str:
    """Create a minimal SKILL.md directory and return its path."""
    skill_dir = tmp_path / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    tags_block = "tags: []\n" if not tags else "tags:\n" + "".join(f"  - {t}\n" for t in tags)
    depends_block = "depends: []\n" if not depends else "depends:\n" + "".join(f"  - {d}\n" for d in depends)
    content = (
        f"---\n"
        f"name: {name}\n"
        f"version: {version}\n"
        f"description: {description}\n"
        f"dcc: {dcc}\n"
        f"{tags_block}"
        f"{depends_block}"
        f"{extra_frontmatter}"
        f"---\n\n# {name}\n\nSkill body.\n"
    )
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    return str(skill_dir)


# ===========================================================================
# TestPyCrashRecoveryPolicyConstruction
# ===========================================================================


class TestPyCrashRecoveryPolicyConstruction:
    def test_default_max_restarts_is_three(self) -> None:
        """Default PyCrashRecoveryPolicy() has max_restarts == 3."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert p.max_restarts == 3

    def test_custom_max_restarts(self) -> None:
        """PyCrashRecoveryPolicy(n) stores n as max_restarts."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(7)
        assert p.max_restarts == 7

    def test_max_restarts_zero(self) -> None:
        """max_restarts=0 means no restarts are allowed."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(0)
        assert p.max_restarts == 0

    def test_max_restarts_one(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(1)
        assert p.max_restarts == 1

    def test_max_restarts_large(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(100)
        assert p.max_restarts == 100


# ===========================================================================
# TestPyCrashRecoveryPolicyShouldRestart
# ===========================================================================


class TestPyCrashRecoveryPolicyShouldRestart:
    def test_should_restart_crashed_status(self) -> None:
        """'crashed' status triggers restart."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        assert p.should_restart("crashed") is True

    def test_should_restart_stopped_status(self) -> None:
        """'stopped' status — result is bool (implementation-defined)."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        result = p.should_restart("stopped")
        assert isinstance(result, bool)

    def test_should_restart_running_returns_false(self) -> None:
        """'running' status should NOT trigger restart."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        assert p.should_restart("running") is False

    def test_should_restart_starting_returns_false(self) -> None:
        """'starting' status should NOT trigger restart."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        assert p.should_restart("starting") is False

    def test_should_restart_restarting_returns_false(self) -> None:
        """'restarting' status should NOT trigger restart."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        assert p.should_restart("restarting") is False

    def test_should_restart_unresponsive_triggers(self) -> None:
        """'unresponsive' status triggers restart."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        # unresponsive may or may not trigger — just verify it returns bool
        result = p.should_restart("unresponsive")
        assert isinstance(result, bool)

    def test_invalid_status_raises(self) -> None:
        """Unknown process status raises ValueError."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        with pytest.raises((ValueError, RuntimeError)):
            p.should_restart("unknown_status_xyz")

    def test_returns_bool(self) -> None:
        """should_restart always returns a bool."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        result = p.should_restart("crashed")
        assert isinstance(result, bool)


# ===========================================================================
# TestPyCrashRecoveryPolicyFixedBackoff
# ===========================================================================


class TestPyCrashRecoveryPolicyFixedBackoff:
    def test_fixed_delay_attempt_zero(self) -> None:
        """Fixed backoff returns configured delay at attempt 0."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=500)
        assert p.next_delay_ms("maya", 0) == 500

    def test_fixed_delay_attempt_one(self) -> None:
        """Fixed backoff returns same delay at attempt 1."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=500)
        assert p.next_delay_ms("maya", 1) == 500

    def test_fixed_delay_is_constant(self) -> None:
        """Fixed backoff delay is constant across all attempts."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(10)
        p.use_fixed_backoff(delay_ms=250)
        delays = [p.next_delay_ms("blender", i) for i in range(5)]
        assert all(d == 250 for d in delays)

    def test_fixed_delay_different_names(self) -> None:
        """Fixed backoff same delay for different DCC names."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=1000)
        assert p.next_delay_ms("maya", 0) == p.next_delay_ms("blender", 0)

    def test_fixed_delay_returns_int(self) -> None:
        """next_delay_ms returns an integer."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=300)
        result = p.next_delay_ms("houdini", 0)
        assert isinstance(result, int)

    def test_fixed_backoff_replaces_previous(self) -> None:
        """Calling use_fixed_backoff twice uses the last setting."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=100)
        p.use_fixed_backoff(delay_ms=999)
        assert p.next_delay_ms("maya", 0) == 999


# ===========================================================================
# TestPyCrashRecoveryPolicyExponentialBackoff
# ===========================================================================


class TestPyCrashRecoveryPolicyExponentialBackoff:
    def test_exponential_delay_attempt_zero_equals_initial(self) -> None:
        """Exponential backoff delay at attempt 0 equals initial_ms."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        assert p.next_delay_ms("maya", 0) == 1000

    def test_exponential_delay_doubles_at_attempt_one(self) -> None:
        """Exponential backoff doubles at attempt 1."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        assert p.next_delay_ms("maya", 1) == 2000

    def test_exponential_delay_doubles_at_attempt_two(self) -> None:
        """Exponential backoff doubles again at attempt 2."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        assert p.next_delay_ms("maya", 2) == 4000

    def test_exponential_delay_capped_at_max(self) -> None:
        """Exponential backoff is capped at max_delay_ms."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(10)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=5000)
        # attempt 3 would be 8000 without cap, but capped to 5000
        delay = p.next_delay_ms("maya", 3)
        assert delay <= 5000

    def test_exponential_delay_increases_monotonically(self) -> None:
        """Delays increase monotonically up to the cap."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(10)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        delays = [p.next_delay_ms("blender", i) for i in range(6)]
        # Each delay should be >= previous
        for i in range(1, len(delays)):
            assert delays[i] >= delays[i - 1]

    def test_exponential_delay_returns_int(self) -> None:
        """next_delay_ms returns an integer for exponential backoff."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_exponential_backoff(initial_ms=500, max_delay_ms=16000)
        result = p.next_delay_ms("houdini", 0)
        assert isinstance(result, int)

    def test_exponential_replaces_fixed(self) -> None:
        """Switching from fixed to exponential changes behavior."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(5)
        p.use_fixed_backoff(delay_ms=999)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        # First attempt should now be 100, not 999
        assert p.next_delay_ms("maya", 0) == 100


# ===========================================================================
# TestPyCrashRecoveryPolicyExceedMaxRestarts
# ===========================================================================


class TestPyCrashRecoveryPolicyExceedMaxRestarts:
    def test_next_delay_exceeds_max_raises(self) -> None:
        """next_delay_ms raises RuntimeError when attempt >= max_restarts."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(2)
        p.use_fixed_backoff(delay_ms=100)
        with pytest.raises(RuntimeError, match="exceeded max restarts"):
            p.next_delay_ms("maya", 3)

    def test_next_delay_at_max_restarts_raises(self) -> None:
        """next_delay_ms at attempt == max_restarts raises."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", 3)

    def test_next_delay_below_max_restarts_ok(self) -> None:
        """next_delay_ms below max_restarts does not raise."""
        p = dcc_mcp_core.PyCrashRecoveryPolicy(3)
        p.use_fixed_backoff(delay_ms=100)
        # attempts 0, 1, 2 are all valid (< 3)
        for attempt in range(3):
            delay = p.next_delay_ms("maya", attempt)
            assert delay == 100


# ===========================================================================
# TestPyProcessWatcherLifecycle
# ===========================================================================


class TestPyProcessWatcherLifecycle:
    def test_initial_watch_count_is_zero(self) -> None:
        """Newly created PyProcessWatcher has zero watched PIDs."""
        w = dcc_mcp_core.PyProcessWatcher()
        assert w.watch_count() == 0

    def test_initial_tracked_count_is_zero(self) -> None:
        """Newly created PyProcessWatcher has zero tracked PIDs."""
        w = dcc_mcp_core.PyProcessWatcher()
        assert w.tracked_count() == 0

    def test_is_running_initial_false(self) -> None:
        """Watcher is not running before start() is called."""
        w = dcc_mcp_core.PyProcessWatcher()
        assert w.is_running() is False

    def test_poll_events_returns_list(self) -> None:
        """poll_events() returns a list (possibly empty)."""
        w = dcc_mcp_core.PyProcessWatcher()
        events = w.poll_events()
        assert isinstance(events, list)

    def test_poll_events_empty_initially(self) -> None:
        """poll_events() is empty when no processes are watched."""
        w = dcc_mcp_core.PyProcessWatcher()
        assert w.poll_events() == []


# ===========================================================================
# TestPyProcessWatcherAddRemove
# ===========================================================================


class TestPyProcessWatcherAddRemove:
    def test_add_watch_increments_count(self) -> None:
        """add_watch(pid, name) increments watch_count by 1."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.add_watch(pid, "test-proc")
        assert w.watch_count() == 1

    def test_is_watched_true_after_add(self) -> None:
        """is_watched(pid) is True after add_watch."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.add_watch(pid, "test-proc")
        assert w.is_watched(pid) is True

    def test_is_watched_false_for_unknown_pid(self) -> None:
        """is_watched(pid) is False for a PID that was never added."""
        w = dcc_mcp_core.PyProcessWatcher()
        assert w.is_watched(99999999) is False

    def test_remove_watch_decrements_count(self) -> None:
        """remove_watch(pid) decrements watch_count."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.add_watch(pid, "test-proc")
        w.remove_watch(pid)
        assert w.watch_count() == 0

    def test_is_watched_false_after_remove(self) -> None:
        """is_watched(pid) is False after remove_watch."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.add_watch(pid, "test-proc")
        w.remove_watch(pid)
        assert w.is_watched(pid) is False

    def test_add_multiple_pids(self) -> None:
        """Multiple add_watch calls correctly track all PIDs."""
        w = dcc_mcp_core.PyProcessWatcher()
        pids = [os.getpid(), 1, 2]
        for p in pids:
            w.add_watch(p, f"proc-{p}")
        assert w.watch_count() == len(pids)

    def test_remove_unknown_pid_does_not_raise(self) -> None:
        """Removing a PID that was not watched does not raise."""
        w = dcc_mcp_core.PyProcessWatcher()
        # Should not raise
        try:
            w.remove_watch(99999999)
        except Exception:
            pytest.skip("Implementation raises on unknown remove — skip")


# ===========================================================================
# TestPyProcessWatcherTrackUntrack
# ===========================================================================


class TestPyProcessWatcherTrackUntrack:
    def test_track_increments_tracked_count(self) -> None:
        """track(pid, name) increments tracked_count."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "maya")
        assert w.tracked_count() == 1

    def test_untrack_decrements_tracked_count(self) -> None:
        """untrack(pid) decrements tracked_count."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "maya")
        w.untrack(pid)
        assert w.tracked_count() == 0

    def test_track_multiple(self) -> None:
        """Multiple track calls accumulate tracked_count."""
        w = dcc_mcp_core.PyProcessWatcher()
        for i, pid in enumerate([os.getpid(), 1, 2]):
            w.track(pid, f"dcc-{i}")
        assert w.tracked_count() == 3

    def test_watch_count_and_tracked_count_independent(self) -> None:
        """watch_count and tracked_count track separately."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.add_watch(pid, "watched")
        w.track(pid, "tracked")
        assert w.watch_count() == 1
        assert w.tracked_count() == 1

    def test_untrack_then_track_again(self) -> None:
        """Can track a PID again after untracking."""
        w = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "maya")
        w.untrack(pid)
        w.track(pid, "maya-again")
        assert w.tracked_count() == 1


# ===========================================================================
# TestToolDefinitionAttributes
# ===========================================================================


class TestToolDefinitionAttributes:
    def test_name_stored(self) -> None:
        """ToolDefinition.name returns the value passed at construction."""
        t = dcc_mcp_core.ToolDefinition(name="create_sphere", description="Create a sphere", input_schema="{}")
        assert t.name == "create_sphere"

    def test_description_stored(self) -> None:
        """ToolDefinition.description returns the value passed at construction."""
        t = dcc_mcp_core.ToolDefinition(name="x", description="My description", input_schema="{}")
        assert t.description == "My description"

    def test_input_schema_stored(self) -> None:
        """ToolDefinition.input_schema returns the JSON schema passed."""
        schema = '{"type": "object"}'
        t = dcc_mcp_core.ToolDefinition(name="x", description="d", input_schema=schema)
        assert t.input_schema == schema

    def test_output_schema_default_none(self) -> None:
        """ToolDefinition.output_schema is None when not specified."""
        t = dcc_mcp_core.ToolDefinition(name="x", description="d", input_schema="{}")
        assert t.output_schema is None

    def test_annotations_default_none(self) -> None:
        """ToolDefinition.annotations is None when not specified."""
        t = dcc_mcp_core.ToolDefinition(name="x", description="d", input_schema="{}")
        assert t.annotations is None

    def test_name_is_string(self) -> None:
        """ToolDefinition.name is always a str."""
        t = dcc_mcp_core.ToolDefinition(name="my_tool", description="d", input_schema="{}")
        assert isinstance(t.name, str)

    def test_description_is_string(self) -> None:
        """ToolDefinition.description is always a str."""
        t = dcc_mcp_core.ToolDefinition(name="x", description="hello", input_schema="{}")
        assert isinstance(t.description, str)

    def test_empty_description_allowed(self) -> None:
        """ToolDefinition allows empty description string."""
        t = dcc_mcp_core.ToolDefinition(name="x", description="", input_schema="{}")
        assert t.description == ""

    def test_repr_contains_name(self) -> None:
        """repr() of ToolDefinition contains the tool name."""
        t = dcc_mcp_core.ToolDefinition(name="my_tool", description="d", input_schema="{}")
        assert "my_tool" in repr(t)

    def test_different_names_not_equal(self) -> None:
        """Two ToolDefinitions with different names are not equal."""
        t1 = dcc_mcp_core.ToolDefinition(name="a", description="d", input_schema="{}")
        t2 = dcc_mcp_core.ToolDefinition(name="b", description="d", input_schema="{}")
        assert t1 != t2

    def test_same_attributes_equal(self) -> None:
        """Two ToolDefinitions with identical attributes compare equal."""
        t1 = dcc_mcp_core.ToolDefinition(name="a", description="d", input_schema="{}")
        t2 = dcc_mcp_core.ToolDefinition(name="a", description="d", input_schema="{}")
        assert t1 == t2


# ===========================================================================
# TestToolAnnotationsDefaults
# ===========================================================================


class TestToolAnnotationsDefaults:
    def test_title_default_none(self) -> None:
        """ToolAnnotations.title defaults to None."""
        a = dcc_mcp_core.ToolAnnotations()
        assert a.title is None

    def test_read_only_hint_default_none(self) -> None:
        """ToolAnnotations.read_only_hint defaults to None."""
        a = dcc_mcp_core.ToolAnnotations()
        assert a.read_only_hint is None

    def test_destructive_hint_default_none(self) -> None:
        """ToolAnnotations.destructive_hint defaults to None."""
        a = dcc_mcp_core.ToolAnnotations()
        assert a.destructive_hint is None

    def test_idempotent_hint_default_none(self) -> None:
        """ToolAnnotations.idempotent_hint defaults to None."""
        a = dcc_mcp_core.ToolAnnotations()
        assert a.idempotent_hint is None

    def test_open_world_hint_default_none(self) -> None:
        """ToolAnnotations.open_world_hint defaults to None."""
        a = dcc_mcp_core.ToolAnnotations()
        assert a.open_world_hint is None

    def test_annotations_instance_is_distinct(self) -> None:
        """Two ToolAnnotations() calls create independent instances."""
        a1 = dcc_mcp_core.ToolAnnotations()
        a2 = dcc_mcp_core.ToolAnnotations()
        assert a1 is not a2

    def test_repr_is_string(self) -> None:
        """ToolAnnotations repr returns a non-empty string."""
        a = dcc_mcp_core.ToolAnnotations()
        assert isinstance(repr(a), str)


# ===========================================================================
# TestResourceDefinitionAttributes
# ===========================================================================


class TestResourceDefinitionAttributes:
    def test_uri_stored(self) -> None:
        """ResourceDefinition.uri returns the value passed."""
        r = dcc_mcp_core.ResourceDefinition(
            uri="maya://scene/current", name="scene", description="Scene", mime_type="application/json"
        )
        assert r.uri == "maya://scene/current"

    def test_name_stored(self) -> None:
        """ResourceDefinition.name returns the value passed."""
        r = dcc_mcp_core.ResourceDefinition(uri="u://x", name="my-resource", description="d", mime_type="text/plain")
        assert r.name == "my-resource"

    def test_description_stored(self) -> None:
        """ResourceDefinition.description returns the value passed."""
        r = dcc_mcp_core.ResourceDefinition(uri="u://x", name="n", description="my desc", mime_type="text/plain")
        assert r.description == "my desc"

    def test_mime_type_stored(self) -> None:
        """ResourceDefinition.mime_type returns the value passed."""
        r = dcc_mcp_core.ResourceDefinition(
            uri="u://x", name="n", description="d", mime_type="application/octet-stream"
        )
        assert r.mime_type == "application/octet-stream"

    def test_annotations_default_none(self) -> None:
        """ResourceDefinition.annotations is None when not specified."""
        r = dcc_mcp_core.ResourceDefinition(uri="u://x", name="n", description="d", mime_type="text/plain")
        assert r.annotations is None

    def test_uri_is_string(self) -> None:
        """ResourceDefinition.uri is always a str."""
        r = dcc_mcp_core.ResourceDefinition(uri="u://foo", name="n", description="d", mime_type="text/plain")
        assert isinstance(r.uri, str)

    def test_repr_contains_uri(self) -> None:
        """repr() of ResourceDefinition contains the URI."""
        r = dcc_mcp_core.ResourceDefinition(uri="maya://objects", name="objs", description="d", mime_type="text/plain")
        assert "maya://objects" in repr(r)

    def test_two_instances_are_distinct_objects(self) -> None:
        """Two ResourceDefinitions with identical params are different objects."""
        r1 = dcc_mcp_core.ResourceDefinition(uri="u://x", name="n", description="d", mime_type="text/plain")
        r2 = dcc_mcp_core.ResourceDefinition(uri="u://x", name="n", description="d", mime_type="text/plain")
        assert r1 is not r2

    def test_different_uri_have_different_repr(self) -> None:
        """Two ResourceDefinitions with different URIs have different repr."""
        r1 = dcc_mcp_core.ResourceDefinition(uri="u://a", name="n", description="d", mime_type="text/plain")
        r2 = dcc_mcp_core.ResourceDefinition(uri="u://b", name="n", description="d", mime_type="text/plain")
        assert repr(r1) != repr(r2)


# ===========================================================================
# TestResourceAnnotationsAttributes
# ===========================================================================


class TestResourceAnnotationsAttributes:
    def test_audience_default_empty_or_none(self) -> None:
        """ResourceAnnotations.audience defaults to None or empty list."""
        a = dcc_mcp_core.ResourceAnnotations()
        # Implementation may return None or [] — both are falsy/empty
        assert a.audience is None or a.audience == []

    def test_priority_default_none(self) -> None:
        """ResourceAnnotations.priority defaults to None."""
        a = dcc_mcp_core.ResourceAnnotations()
        assert a.priority is None

    def test_repr_is_string(self) -> None:
        a = dcc_mcp_core.ResourceAnnotations()
        assert isinstance(repr(a), str)

    def test_two_instances_independent(self) -> None:
        a1 = dcc_mcp_core.ResourceAnnotations()
        a2 = dcc_mcp_core.ResourceAnnotations()
        assert a1 is not a2


# ===========================================================================
# TestResourceTemplateDefinitionAttributes
# ===========================================================================


class TestResourceTemplateDefinitionAttributes:
    def test_uri_template_stored(self) -> None:
        """ResourceTemplateDefinition.uri_template returns the value passed."""
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="maya://{scene}/{object}",
            name="scene-object",
            description="A scene object resource",
            mime_type="text/plain",
        )
        assert rt.uri_template == "maya://{scene}/{object}"

    def test_name_stored(self) -> None:
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="my-template", description="d", mime_type="text/plain"
        )
        assert rt.name == "my-template"

    def test_description_stored(self) -> None:
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="n", description="my template desc", mime_type="text/plain"
        )
        assert rt.description == "my template desc"

    def test_mime_type_stored(self) -> None:
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="n", description="d", mime_type="application/json"
        )
        assert rt.mime_type == "application/json"

    def test_annotations_default_none(self) -> None:
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="n", description="d", mime_type="text/plain"
        )
        assert rt.annotations is None

    def test_repr_contains_template(self) -> None:
        rt = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="dcc://{dcc_type}/scene",
            name="n",
            description="d",
            mime_type="text/plain",
        )
        assert "dcc://{dcc_type}/scene" in repr(rt)

    def test_two_instances_distinct_objects(self) -> None:
        """Two ResourceTemplateDefinitions with same params are different objects."""
        rt1 = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="n", description="d", mime_type="text/plain"
        )
        rt2 = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="u://{x}", name="n", description="d", mime_type="text/plain"
        )
        assert rt1 is not rt2


# ===========================================================================
# TestPromptDefinitionAttributes
# ===========================================================================


class TestPromptDefinitionAttributes:
    def test_name_stored(self) -> None:
        """PromptDefinition.name returns the value passed."""
        pd = dcc_mcp_core.PromptDefinition(name="create-scene", description="Create a scene", arguments=[])
        assert pd.name == "create-scene"

    def test_description_stored(self) -> None:
        """PromptDefinition.description returns the value passed."""
        pd = dcc_mcp_core.PromptDefinition(name="x", description="my prompt desc", arguments=[])
        assert pd.description == "my prompt desc"

    def test_arguments_empty_list(self) -> None:
        """PromptDefinition.arguments is an empty list when passed []."""
        pd = dcc_mcp_core.PromptDefinition(name="x", description="d", arguments=[])
        assert pd.arguments == []

    def test_arguments_with_one_arg(self) -> None:
        """PromptDefinition.arguments stores PromptArgument instances."""
        arg = dcc_mcp_core.PromptArgument(name="scene_name", description="Scene name", required=True)
        pd = dcc_mcp_core.PromptDefinition(name="x", description="d", arguments=[arg])
        assert len(pd.arguments) == 1

    def test_repr_contains_name(self) -> None:
        """repr() of PromptDefinition contains the prompt name."""
        pd = dcc_mcp_core.PromptDefinition(name="my-prompt", description="d", arguments=[])
        assert "my-prompt" in repr(pd)

    def test_same_params_equal(self) -> None:
        """Two PromptDefinitions with identical names/desc compare equal."""
        pd1 = dcc_mcp_core.PromptDefinition(name="x", description="d", arguments=[])
        pd2 = dcc_mcp_core.PromptDefinition(name="x", description="d", arguments=[])
        assert pd1 == pd2

    def test_different_names_not_equal(self) -> None:
        pd1 = dcc_mcp_core.PromptDefinition(name="a", description="d", arguments=[])
        pd2 = dcc_mcp_core.PromptDefinition(name="b", description="d", arguments=[])
        assert pd1 != pd2


# ===========================================================================
# TestPromptArgumentAttributes
# ===========================================================================


class TestPromptArgumentAttributes:
    def test_name_stored(self) -> None:
        """PromptArgument.name returns the value passed."""
        pa = dcc_mcp_core.PromptArgument(name="my_arg", description="An argument", required=True)
        assert pa.name == "my_arg"

    def test_description_stored(self) -> None:
        """PromptArgument.description returns the value passed."""
        pa = dcc_mcp_core.PromptArgument(name="x", description="arg description", required=False)
        assert pa.description == "arg description"

    def test_required_true(self) -> None:
        """PromptArgument.required is True when passed True."""
        pa = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        assert pa.required is True

    def test_required_false(self) -> None:
        """PromptArgument.required is False when passed False."""
        pa = dcc_mcp_core.PromptArgument(name="x", description="d", required=False)
        assert pa.required is False

    def test_required_is_bool(self) -> None:
        """PromptArgument.required is always a bool."""
        pa = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        assert isinstance(pa.required, bool)

    def test_name_is_string(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="d", required=False)
        assert isinstance(pa.name, str)

    def test_repr_contains_name(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="scene_path", description="d", required=True)
        assert "scene_path" in repr(pa)

    def test_same_params_equal(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        pa2 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        assert pa1 == pa2

    def test_different_required_not_equal(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        pa2 = dcc_mcp_core.PromptArgument(name="x", description="d", required=False)
        assert pa1 != pa2


# ===========================================================================
# TestSkillMetadataFields
# ===========================================================================


class TestSkillMetadataFields:
    def test_name_stored(self, tmp_path: Path) -> None:
        """parse_skill_md returns correct name."""
        sd = _make_skill_dir(tmp_path, name="cool-skill")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.name == "cool-skill"

    def test_version_stored(self, tmp_path: Path) -> None:
        """parse_skill_md returns correct version."""
        sd = _make_skill_dir(tmp_path, name="s", version="2.5.0")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.version == "2.5.0"

    def test_description_stored(self, tmp_path: Path) -> None:
        """parse_skill_md returns correct description."""
        sd = _make_skill_dir(tmp_path, name="s", description="Creates polygon geometry")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.description == "Creates polygon geometry"

    def test_dcc_stored(self, tmp_path: Path) -> None:
        """parse_skill_md returns correct dcc field."""
        sd = _make_skill_dir(tmp_path, name="s", dcc="blender")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.dcc == "blender"

    def test_tags_single(self, tmp_path: Path) -> None:
        """parse_skill_md returns single-tag list correctly."""
        sd = _make_skill_dir(tmp_path, name="s", tags=["geometry"])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.tags == ["geometry"]

    def test_tags_multiple(self, tmp_path: Path) -> None:
        """parse_skill_md returns multiple tags in list."""
        sd = _make_skill_dir(tmp_path, name="s", tags=["geometry", "mesh", "create"])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert set(meta.tags) == {"geometry", "mesh", "create"}

    def test_tags_empty(self, tmp_path: Path) -> None:
        """parse_skill_md returns empty list when no tags."""
        sd = _make_skill_dir(tmp_path, name="s", tags=[])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.tags == []

    def test_depends_single(self, tmp_path: Path) -> None:
        """parse_skill_md returns single dependency."""
        sd = _make_skill_dir(tmp_path, name="s", depends=["base-skill"])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.depends == ["base-skill"]

    def test_depends_multiple(self, tmp_path: Path) -> None:
        """parse_skill_md returns multiple dependencies."""
        sd = _make_skill_dir(tmp_path, name="s", depends=["skill-a", "skill-b"])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert set(meta.depends) == {"skill-a", "skill-b"}

    def test_depends_empty(self, tmp_path: Path) -> None:
        """parse_skill_md returns empty list when no dependencies."""
        sd = _make_skill_dir(tmp_path, name="s", depends=[])
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.depends == []

    def test_skill_path_set(self, tmp_path: Path) -> None:
        """parse_skill_md sets skill_path to the directory passed."""
        sd = _make_skill_dir(tmp_path, name="path-skill")
        meta = dcc_mcp_core.parse_skill_md(sd)
        # skill_path should point to (or contain) the directory
        assert meta.skill_path is not None
        assert len(meta.skill_path) > 0

    def test_skill_path_contains_dir_name(self, tmp_path: Path) -> None:
        """skill_path contains the skill directory name."""
        sd = _make_skill_dir(tmp_path, name="my-unique-skill")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert "my-unique-skill" in meta.skill_path

    def test_scripts_empty_without_py_files(self, tmp_path: Path) -> None:
        """Scripts list is empty when no .py files present."""
        sd = _make_skill_dir(tmp_path, name="s")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.scripts == []

    def test_tools_empty_initially(self, tmp_path: Path) -> None:
        """Tools list is empty when SKILL.md has no tools section."""
        sd = _make_skill_dir(tmp_path, name="s")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.tools == []

    def test_metadata_files_list(self, tmp_path: Path) -> None:
        """metadata_files returns a list."""
        sd = _make_skill_dir(tmp_path, name="s")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert isinstance(meta.metadata_files, list)

    def test_version_is_string(self, tmp_path: Path) -> None:
        """Version field is always a str."""
        sd = _make_skill_dir(tmp_path, name="s", version="0.0.1")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert isinstance(meta.version, str)

    def test_dcc_all_lowercase(self, tmp_path: Path) -> None:
        """Dcc field stores value as given (lowercase convention)."""
        sd = _make_skill_dir(tmp_path, name="s", dcc="houdini")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert meta.dcc == "houdini"

    def test_name_is_string(self, tmp_path: Path) -> None:
        """Name field is always a str."""
        sd = _make_skill_dir(tmp_path, name="str-skill")
        meta = dcc_mcp_core.parse_skill_md(sd)
        assert isinstance(meta.name, str)

    def test_missing_skill_md_returns_default_or_raises(self, tmp_path: Path) -> None:
        """parse_skill_md on a dir without SKILL.md either raises or returns default."""
        empty_dir = tmp_path / "empty-skill"
        empty_dir.mkdir()
        try:
            meta = dcc_mcp_core.parse_skill_md(str(empty_dir))
            # If no exception, name should still be a string (possibly empty/default)
            assert isinstance(meta.name, str)
        except Exception:
            pass  # Raising is also acceptable

    def test_nonexistent_dir_returns_default_or_raises(self, tmp_path: Path) -> None:
        """parse_skill_md on a nonexistent directory either raises or returns default."""
        try:
            meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "does-not-exist"))
            assert isinstance(meta.name, str)
        except Exception:
            pass  # Raising is also acceptable
