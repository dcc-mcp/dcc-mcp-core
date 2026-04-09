"""Tests for VersionedRegistry deep API, SemVer.matches_constraint, and EventBus pub/sub.

Covers VersionedRegistry.resolve_all/latest_version/versions/remove/keys,
SemVer comparison and matches_constraint, EventBus subscribe/publish/unsubscribe.
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── VersionedRegistry deep API ────────────────────────────────────────────────


class TestVersionedRegistryDeep:
    def _make_registry(self) -> dcc_mcp_core.VersionedRegistry:
        vreg = dcc_mcp_core.VersionedRegistry()
        vreg.register_versioned("render", dcc="maya", version="1.0.0")
        vreg.register_versioned("render", dcc="maya", version="1.5.0")
        vreg.register_versioned("render", dcc="maya", version="2.0.0")
        return vreg

    def test_resolve_all_wildcard_returns_all(self) -> None:
        vreg = self._make_registry()
        results = vreg.resolve_all("render", dcc="maya", constraint="*")
        assert len(results) == 3

    def test_resolve_all_caret_returns_compatible(self) -> None:
        vreg = self._make_registry()
        results = vreg.resolve_all("render", dcc="maya", constraint="^1.0.0")
        versions = [str(r) for r in results]
        assert any("1.0.0" in v or "1.5.0" in v for v in versions)
        assert not any("2.0.0" in v for v in versions)

    def test_resolve_all_gte_constraint(self) -> None:
        vreg = self._make_registry()
        results = vreg.resolve_all("render", dcc="maya", constraint=">=1.5.0")
        assert len(results) >= 2

    def test_latest_version_returns_highest(self) -> None:
        vreg = self._make_registry()
        latest = vreg.latest_version("render", dcc="maya")
        assert latest == "2.0.0"

    def test_versions_returns_sorted_list(self) -> None:
        vreg = self._make_registry()
        versions = vreg.versions("render", dcc="maya")
        assert versions == ["1.0.0", "1.5.0", "2.0.0"]

    def test_keys_contains_action_dcc_tuple(self) -> None:
        vreg = self._make_registry()
        keys = vreg.keys()
        assert ("render", "maya") in keys

    def test_remove_caret_constraint_removes_compat_only(self) -> None:
        vreg = self._make_registry()
        removed = vreg.remove("render", dcc="maya", constraint="^1.0.0")
        assert removed == 2
        remaining = vreg.versions("render", dcc="maya")
        assert remaining == ["2.0.0"]

    def test_remove_wildcard_removes_all(self) -> None:
        vreg = self._make_registry()
        removed = vreg.remove("render", dcc="maya", constraint="*")
        assert removed == 3

    def test_remove_exact_version(self) -> None:
        vreg = self._make_registry()
        removed = vreg.remove("render", dcc="maya", constraint="=1.0.0")
        assert removed == 1
        remaining = vreg.versions("render", dcc="maya")
        assert "1.0.0" not in remaining

    def test_resolve_after_remove_returns_remaining(self) -> None:
        vreg = self._make_registry()
        vreg.remove("render", dcc="maya", constraint="^1.0.0")
        result = vreg.resolve("render", dcc="maya", constraint="*")
        assert result is not None
        assert "2.0.0" in str(result)

    def test_multiple_dccs_independent(self) -> None:
        vreg = dcc_mcp_core.VersionedRegistry()
        vreg.register_versioned("render", dcc="maya", version="1.0.0")
        vreg.register_versioned("render", dcc="blender", version="2.0.0")
        maya_latest = vreg.latest_version("render", dcc="maya")
        blender_latest = vreg.latest_version("render", dcc="blender")
        assert maya_latest == "1.0.0"
        assert blender_latest == "2.0.0"

    def test_keys_multiple_actions(self) -> None:
        vreg = dcc_mcp_core.VersionedRegistry()
        vreg.register_versioned("render", dcc="maya", version="1.0.0")
        vreg.register_versioned("export", dcc="maya", version="1.0.0")
        keys = vreg.keys()
        assert len(keys) >= 2

    def test_resolve_nonexistent_action_returns_none(self) -> None:
        vreg = dcc_mcp_core.VersionedRegistry()
        result = vreg.resolve("nonexistent_action", dcc="maya", constraint="*")
        assert result is None


# ── SemVer deep API ───────────────────────────────────────────────────────────


class TestSemVerDeep:
    def test_parse_major_minor_patch(self) -> None:
        v = dcc_mcp_core.SemVer.parse("3.7.12")
        assert v.major == 3
        assert v.minor == 7
        assert v.patch == 12

    def test_parse_zero_version(self) -> None:
        v = dcc_mcp_core.SemVer.parse("0.0.0")
        assert v.major == 0
        assert v.minor == 0
        assert v.patch == 0

    def test_comparison_less_than(self) -> None:
        v1 = dcc_mcp_core.SemVer.parse("1.0.0")
        v2 = dcc_mcp_core.SemVer.parse("2.0.0")
        assert v1 < v2

    def test_comparison_greater_than(self) -> None:
        v1 = dcc_mcp_core.SemVer.parse("2.0.0")
        v2 = dcc_mcp_core.SemVer.parse("1.9.9")
        assert v1 > v2

    def test_equality(self) -> None:
        v1 = dcc_mcp_core.SemVer.parse("1.2.3")
        v2 = dcc_mcp_core.SemVer.parse("1.2.3")
        assert v1 == v2

    def test_inequality_different_patch(self) -> None:
        v1 = dcc_mcp_core.SemVer.parse("1.2.3")
        v2 = dcc_mcp_core.SemVer.parse("1.2.4")
        assert v1 != v2

    def test_matches_constraint_wildcard(self) -> None:
        v = dcc_mcp_core.SemVer.parse("1.5.0")
        vc = dcc_mcp_core.VersionConstraint.parse("*")
        assert v.matches_constraint(vc) is True

    def test_matches_constraint_gte(self) -> None:
        v = dcc_mcp_core.SemVer.parse("2.0.0")
        assert v.matches_constraint(dcc_mcp_core.VersionConstraint.parse(">=1.0.0")) is True
        assert v.matches_constraint(dcc_mcp_core.VersionConstraint.parse(">=3.0.0")) is False

    def test_matches_constraint_caret_major(self) -> None:
        v1 = dcc_mcp_core.SemVer.parse("1.5.0")
        assert v1.matches_constraint(dcc_mcp_core.VersionConstraint.parse("^1.0.0")) is True
        v2 = dcc_mcp_core.SemVer.parse("2.0.0")
        assert v2.matches_constraint(dcc_mcp_core.VersionConstraint.parse("^1.0.0")) is False

    def test_matches_constraint_exact(self) -> None:
        v = dcc_mcp_core.SemVer.parse("1.2.3")
        assert v.matches_constraint(dcc_mcp_core.VersionConstraint.parse("=1.2.3")) is True
        assert v.matches_constraint(dcc_mcp_core.VersionConstraint.parse("=1.2.4")) is False

    def test_str_representation(self) -> None:
        v = dcc_mcp_core.SemVer.parse("1.2.3")
        s = str(v)
        assert "1" in s
        assert "2" in s
        assert "3" in s

    def test_repr_is_string(self) -> None:
        v = dcc_mcp_core.SemVer.parse("1.0.0")
        assert isinstance(repr(v), str)


# ── EventBus pub/sub ──────────────────────────────────────────────────────────


class TestEventBus:
    def test_subscribe_returns_int_id(self) -> None:
        eb = dcc_mcp_core.EventBus()
        sub_id = eb.subscribe("test:event", lambda **kw: None)
        assert isinstance(sub_id, int)

    def test_subscribe_increments_id(self) -> None:
        eb = dcc_mcp_core.EventBus()
        id1 = eb.subscribe("event:a", lambda **kw: None)
        id2 = eb.subscribe("event:a", lambda **kw: None)
        assert id2 > id1

    def test_publish_triggers_subscriber(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received: list = []
        eb.subscribe("action:start", lambda **kw: received.append(kw))
        eb.publish("action:start", action="create_sphere", dcc="maya")
        assert len(received) == 1
        assert received[0]["action"] == "create_sphere"

    def test_publish_kwargs_preserved(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received: list = []
        eb.subscribe("test", lambda **kw: received.append(kw))
        eb.publish("test", key1="val1", key2=42)
        assert received[0]["key1"] == "val1"
        assert received[0]["key2"] == 42

    def test_publish_no_subscribers_no_error(self) -> None:
        eb = dcc_mcp_core.EventBus()
        eb.publish("nonexistent:event", data="test")

    def test_multiple_subscribers_all_called(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        eb.subscribe("event", lambda **kw: calls.append("a"))
        eb.subscribe("event", lambda **kw: calls.append("b"))
        eb.publish("event")
        assert len(calls) == 2

    def test_unsubscribe_stops_callback(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received: list = []
        sub_id = eb.subscribe("action", lambda **kw: received.append(kw))
        eb.publish("action", x=1)
        eb.unsubscribe("action", sub_id)
        eb.publish("action", x=2)
        assert len(received) == 1

    def test_publish_multiple_times(self) -> None:
        eb = dcc_mcp_core.EventBus()
        count: list = []
        eb.subscribe("tick", lambda **kw: count.append(1))
        for _ in range(5):
            eb.publish("tick")
        assert len(count) == 5

    def test_different_events_isolated(self) -> None:
        eb = dcc_mcp_core.EventBus()
        a_calls: list = []
        b_calls: list = []
        eb.subscribe("event:a", lambda **kw: a_calls.append(1))
        eb.subscribe("event:b", lambda **kw: b_calls.append(1))
        eb.publish("event:a")
        assert len(a_calls) == 1
        assert len(b_calls) == 0

    def test_repr_contains_eventbus(self) -> None:
        eb = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(eb)
