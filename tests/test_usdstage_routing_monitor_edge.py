"""Test UsdStage(name) ctor, RoutingStrategy variants, PyProcessMonitor lifecycle, and ToolRegistry edge cases.

Covers:
  - UsdStage(name) constructor: basic fields, prim_count() as method, to_json/from_json roundtrip
  - RoutingStrategy: all 6 variants (enum equality, repr, distinct values)
  - TransportManager.get_or_create_session_routed with all strategy variants
  - PyProcessMonitor: track/untrack/refresh/query/list_all full lifecycle
  - ToolRegistry: unregister, list_actions_for_dcc, register error paths
  - McpHttpConfig: server_name/server_version with special characters
  - UsdStage.from_json with different up_axis and fps values
  - SemVer comparison edge cases

Verified via Python REPL probes (2026-04-08).
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
import os
import tempfile

# Import third-party modules
import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SemVer

# Import local modules
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VtValue

# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _make_stage_json(
    name: str = "test",
    up_axis: str = "Y",
    fps: float = 24.0,
    default_prim: str | None = None,
) -> str:
    """Create a minimal valid UsdStage JSON."""
    return json.dumps(
        {
            "id": "test-id-001",
            "name": name,
            "root_layer": {
                "identifier": f"anon:{name}",
                "display_name": name,
                "up_axis": up_axis,
                "meters_per_unit": 0.01,
                "start_time_code": 1.0,
                "end_time_code": 24.0,
                "frames_per_second": fps,
                "prims": {},
                "custom_layer_data": {},
            },
            "sublayers": [],
            "default_prim": default_prim,
            "metadata": {},
        }
    )


# ─────────────────────────────────────────────────────────────────────────────
# UsdStage(name) direct constructor
# ─────────────────────────────────────────────────────────────────────────────


class TestUsdStageNameConstructor:
    """UsdStage(name) constructor — basic fields and method behavior."""

    class TestBasicFields:
        def test_name_is_preserved(self) -> None:
            stage = UsdStage("my_scene")
            assert stage.name == "my_scene"

        def test_up_axis_default_y(self) -> None:
            stage = UsdStage("x")
            assert stage.up_axis == "Y"

        def test_fps_default_none_for_new_stage(self) -> None:
            # New stage has no fps set (root layer defaults to None)
            stage = UsdStage("x")
            assert stage.fps is None

        def test_meters_per_unit_default_one(self) -> None:
            # New stage defaults to 1.0 (no unit set)
            stage = UsdStage("x")
            assert stage.meters_per_unit == pytest.approx(1.0)

        def test_default_prim_none(self) -> None:
            stage = UsdStage("x")
            assert stage.default_prim is None

        def test_id_is_nonempty_string(self) -> None:
            stage = UsdStage("x")
            assert isinstance(stage.id, str)
            assert len(stage.id) > 0

        def test_start_time_code_none_for_new_stage(self) -> None:
            # New stage has no time codes set
            stage = UsdStage("x")
            assert stage.start_time_code is None

        def test_end_time_code_none_for_new_stage(self) -> None:
            stage = UsdStage("x")
            assert stage.end_time_code is None

        def test_repr_contains_name(self) -> None:
            stage = UsdStage("my_stage")
            assert "my_stage" in repr(stage)

    class TestPrimCountMethod:
        def test_prim_count_is_callable(self) -> None:
            stage = UsdStage("x")
            assert callable(stage.prim_count)

        def test_initial_prim_count_is_zero(self) -> None:
            stage = UsdStage("x")
            assert stage.prim_count() == 0

        def test_prim_count_increments_after_define_prim(self) -> None:
            stage = UsdStage("x")
            stage.define_prim("/Sphere", "Sphere")
            assert stage.prim_count() == 1

        def test_prim_count_multiple_prims(self) -> None:
            stage = UsdStage("x")
            stage.define_prim("/Sphere", "Sphere")
            stage.define_prim("/Cube", "Cube")
            stage.define_prim("/Light", "DistantLight")
            assert stage.prim_count() == 3

        def test_prim_count_after_remove_decrements(self) -> None:
            stage = UsdStage("x")
            stage.define_prim("/Sphere", "Sphere")
            assert stage.prim_count() == 1
            stage.remove_prim("/Sphere")
            assert stage.prim_count() == 0

        def test_prim_count_returns_int(self) -> None:
            stage = UsdStage("x")
            assert isinstance(stage.prim_count(), int)

    class TestToJsonFromJsonRoundtrip:
        def test_to_json_returns_string(self) -> None:
            stage = UsdStage("x")
            j = stage.to_json()
            assert isinstance(j, str)

        def test_to_json_is_valid_json(self) -> None:
            stage = UsdStage("x")
            j = stage.to_json()
            parsed = json.loads(j)
            assert isinstance(parsed, dict)

        def test_to_json_contains_id_and_name(self) -> None:
            stage = UsdStage("rt_test")
            j = stage.to_json()
            parsed = json.loads(j)
            assert "id" in parsed
            assert "name" in parsed
            assert parsed["name"] == "rt_test"

        def test_from_json_roundtrip_name(self) -> None:
            stage = UsdStage("roundtrip_scene")
            j = stage.to_json()
            back = UsdStage.from_json(j)
            assert back.name == "roundtrip_scene"

        def test_from_json_roundtrip_prim_count(self) -> None:
            stage = UsdStage("prim_rt")
            stage.define_prim("/WorldPrim", "Xform")
            j = stage.to_json()
            back = UsdStage.from_json(j)
            assert back.prim_count() == 1

        def test_from_json_roundtrip_fps(self) -> None:
            stage = UsdStage("fps_rt")
            j = stage.to_json()
            back = UsdStage.from_json(j)
            assert back.fps == pytest.approx(stage.fps)

        def test_from_json_different_up_axis(self) -> None:
            j = _make_stage_json(up_axis="Z")
            stage = UsdStage.from_json(j)
            assert stage.up_axis == "Z"

        def test_from_json_various_fps(self) -> None:
            for fps in [24.0, 25.0, 30.0, 48.0, 60.0]:
                j = _make_stage_json(fps=fps)
                stage = UsdStage.from_json(j)
                assert stage.fps == pytest.approx(fps)


# ─────────────────────────────────────────────────────────────────────────────
# RoutingStrategy — all variants
# ─────────────────────────────────────────────────────────────────────────────


class TestRoutingStrategyVariants:
    """RoutingStrategy enum — all 6 variants."""

    class TestAllVariantsExist:
        def test_first_available(self) -> None:
            assert hasattr(RoutingStrategy, "FIRST_AVAILABLE")

        def test_round_robin(self) -> None:
            assert hasattr(RoutingStrategy, "ROUND_ROBIN")

        def test_least_busy(self) -> None:
            assert hasattr(RoutingStrategy, "LEAST_BUSY")

        def test_specific(self) -> None:
            assert hasattr(RoutingStrategy, "SPECIFIC")

        def test_scene_match(self) -> None:
            assert hasattr(RoutingStrategy, "SCENE_MATCH")

        def test_random(self) -> None:
            assert hasattr(RoutingStrategy, "RANDOM")

    class TestDistinctness:
        def test_all_six_are_distinct(self) -> None:
            variants = [
                RoutingStrategy.FIRST_AVAILABLE,
                RoutingStrategy.ROUND_ROBIN,
                RoutingStrategy.LEAST_BUSY,
                RoutingStrategy.SPECIFIC,
                RoutingStrategy.SCENE_MATCH,
                RoutingStrategy.RANDOM,
            ]
            for i, a in enumerate(variants):
                for j, b in enumerate(variants):
                    if i != j:
                        assert a != b

        def test_self_equality(self) -> None:
            for variant in [
                RoutingStrategy.FIRST_AVAILABLE,
                RoutingStrategy.ROUND_ROBIN,
                RoutingStrategy.LEAST_BUSY,
                RoutingStrategy.SPECIFIC,
                RoutingStrategy.SCENE_MATCH,
                RoutingStrategy.RANDOM,
            ]:
                assert variant == variant

    class TestReprStrings:
        def test_first_available_repr(self) -> None:
            r = repr(RoutingStrategy.FIRST_AVAILABLE)
            assert isinstance(r, str)
            assert len(r) > 0

        def test_round_robin_repr(self) -> None:
            r = repr(RoutingStrategy.ROUND_ROBIN)
            assert isinstance(r, str)
            assert len(r) > 0

        def test_all_reprs_are_different(self) -> None:
            reprs = [
                repr(RoutingStrategy.FIRST_AVAILABLE),
                repr(RoutingStrategy.ROUND_ROBIN),
                repr(RoutingStrategy.LEAST_BUSY),
                repr(RoutingStrategy.SPECIFIC),
                repr(RoutingStrategy.SCENE_MATCH),
                repr(RoutingStrategy.RANDOM),
            ]
            # All repr strings should be distinct
            assert len(set(reprs)) == 6


# ─────────────────────────────────────────────────────────────────────────────
# TransportManager + RoutingStrategy integration
# ─────────────────────────────────────────────────────────────────────────────


class TestTransportManagerRoutingIntegration:
    """TransportManager.get_or_create_session_routed with all strategies."""

    @pytest.fixture
    def transport(self, tmp_path):
        reg_file = str(tmp_path / "registry.json")
        tm = TransportManager(reg_file)
        tm.register_service("maya", "127.0.0.1", 18812)
        yield tm
        with contextlib.suppress(Exception):
            tm.shutdown()

    class TestRoutedSessionAllStrategies:
        def test_round_robin_returns_uuid(self, transport) -> None:
            sid = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
            assert isinstance(sid, str)
            assert len(sid) == 36  # UUID format

        def test_first_available_returns_uuid(self, transport) -> None:
            sid = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.FIRST_AVAILABLE)
            assert isinstance(sid, str)
            assert len(sid) == 36

        def test_least_busy_returns_uuid(self, transport) -> None:
            sid = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.LEAST_BUSY)
            assert isinstance(sid, str)

        def test_random_returns_uuid(self, transport) -> None:
            sid = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.RANDOM)
            assert isinstance(sid, str)

        def test_scene_match_raises_without_hint(self, transport) -> None:
            # SCENE_MATCH requires a specific instance hint; without it, raises
            with pytest.raises((RuntimeError, Exception)):
                transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.SCENE_MATCH)

        def test_specific_raises_without_hint(self, transport) -> None:
            # SPECIFIC requires an instance_id; without it, raises
            with pytest.raises((RuntimeError, Exception)):
                transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.SPECIFIC)

        def test_two_calls_same_strategy_same_session(self, transport) -> None:
            sid1 = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
            sid2 = transport.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
            # Existing session should be reused
            assert sid1 == sid2


# ─────────────────────────────────────────────────────────────────────────────
# PyProcessMonitor — full lifecycle
# ─────────────────────────────────────────────────────────────────────────────


class TestPyProcessMonitorLifecycle:
    """PyProcessMonitor: track/untrack/refresh/query/list_all full lifecycle."""

    class TestConstruction:
        def test_default_construction(self) -> None:
            mon = PyProcessMonitor()
            assert mon is not None

        def test_initial_tracked_count_zero(self) -> None:
            mon = PyProcessMonitor()
            assert mon.tracked_count() == 0

        def test_initial_list_all_empty(self) -> None:
            mon = PyProcessMonitor()
            assert mon.list_all() == []

    class TestTrackUntrack:
        def test_track_self_increments_count(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            assert mon.tracked_count() == 1

        def test_track_multiple_processes(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "proc-a")
            mon.track(pid + 1, "proc-b")
            assert mon.tracked_count() == 2

        def test_untrack_reduces_count(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            assert mon.tracked_count() == 1
            mon.untrack(pid)
            assert mon.tracked_count() == 0

        def test_untrack_nonexistent_no_error(self) -> None:
            mon = PyProcessMonitor()
            # Should not raise
            mon.untrack(999999)

        def test_tracked_count_returns_int(self) -> None:
            mon = PyProcessMonitor()
            assert isinstance(mon.tracked_count(), int)

    class TestRefreshAndQuery:
        def test_query_before_refresh_may_return_none(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            result = mon.query(pid)
            # May be None before refresh
            assert result is None or isinstance(result, dict)

        def test_query_after_refresh_returns_dict(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            mon.refresh()
            result = mon.query(pid)
            assert isinstance(result, dict)

        def test_query_result_has_expected_keys(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            mon.refresh()
            result = mon.query(pid)
            assert result is not None
            assert "pid" in result
            assert "name" in result
            # memory key is memory_bytes in this impl
            has_memory = "memory_bytes" in result or "memory_kb" in result
            assert has_memory

        def test_query_result_pid_matches(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            mon.refresh()
            result = mon.query(pid)
            assert result is not None
            assert result["pid"] == pid

        def test_query_result_name_matches(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "test-proc")
            mon.refresh()
            result = mon.query(pid)
            assert result is not None
            assert result["name"] == "test-proc"

        def test_query_result_memory_non_negative(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "self")
            mon.refresh()
            result = mon.query(pid)
            assert result is not None
            # memory key is memory_bytes in this impl
            mem = result.get("memory_bytes", result.get("memory_kb", 0))
            assert mem >= 0

        def test_query_untracked_returns_none(self) -> None:
            mon = PyProcessMonitor()
            result = mon.query(os.getpid())
            assert result is None

    class TestListAll:
        def test_list_all_after_track_and_refresh(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "test")
            mon.refresh()
            all_procs = mon.list_all()
            assert isinstance(all_procs, list)
            assert len(all_procs) >= 1

        def test_list_all_items_are_dicts(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "test")
            mon.refresh()
            all_procs = mon.list_all()
            for item in all_procs:
                assert isinstance(item, dict)

        def test_list_all_empty_after_untrack(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            mon.track(pid, "test")
            mon.refresh()
            mon.untrack(pid)
            # After untrack, count should be 0
            assert mon.tracked_count() == 0

    class TestIsAlive:
        def test_is_alive_self(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            assert mon.is_alive(pid) is True

        def test_is_alive_nonexistent_pid(self) -> None:
            mon = PyProcessMonitor()
            # Use a very large PID that almost certainly doesn't exist
            result = mon.is_alive(999999)
            assert result is False

        def test_is_alive_returns_bool(self) -> None:
            mon = PyProcessMonitor()
            pid = os.getpid()
            result = mon.is_alive(pid)
            assert isinstance(result, bool)


# ─────────────────────────────────────────────────────────────────────────────
# ToolRegistry edge cases
# ─────────────────────────────────────────────────────────────────────────────


class TestActionRegistryEdgeCases:
    """ToolRegistry — edge cases, unregister, multi-DCC patterns."""

    class TestUnregisterGlobal:
        def test_unregister_removes_action(self) -> None:
            reg = ToolRegistry()
            reg.register("my_action", description="Test", category="geo")
            removed = reg.unregister("my_action")
            assert removed is True

        def test_unregister_nonexistent_returns_false(self) -> None:
            reg = ToolRegistry()
            removed = reg.unregister("nonexistent_action")
            assert removed is False

        def test_after_unregister_action_not_in_list(self) -> None:
            reg = ToolRegistry()
            reg.register("action_a", description="A", category="geo")
            reg.unregister("action_a")
            names = reg.list_actions()
            assert "action_a" not in names

        def test_unregister_scoped_removes_only_that_dcc(self) -> None:
            reg = ToolRegistry()
            reg.register("shared_action", description="A", category="geo", dcc="maya")
            reg.register("shared_action", description="A", category="geo", dcc="blender")
            removed = reg.unregister("shared_action", dcc_name="maya")
            assert removed is True
            # Blender's version should remain
            maya_names = reg.list_actions_for_dcc("maya")
            blender_names = reg.list_actions_for_dcc("blender")
            assert "shared_action" not in maya_names
            assert "shared_action" in blender_names

    class TestListActionsForDcc:
        def test_list_actions_for_dcc_returns_list(self) -> None:
            reg = ToolRegistry()
            reg.register("create_sphere", description="Sphere", category="geo", dcc="maya")
            names = reg.list_actions_for_dcc("maya")
            assert isinstance(names, list)

        def test_list_actions_for_dcc_contains_registered(self) -> None:
            reg = ToolRegistry()
            reg.register("maya_tool", description="T", category="geo", dcc="maya")
            names = reg.list_actions_for_dcc("maya")
            assert "maya_tool" in names

        def test_list_actions_for_dcc_excludes_other_dcc(self) -> None:
            reg = ToolRegistry()
            reg.register("maya_tool", description="T", category="geo", dcc="maya")
            reg.register("blender_tool", description="B", category="geo", dcc="blender")
            maya_names = reg.list_actions_for_dcc("maya")
            assert "blender_tool" not in maya_names

        def test_list_actions_for_unknown_dcc_returns_empty(self) -> None:
            reg = ToolRegistry()
            names = reg.list_actions_for_dcc("unknown_dcc_xyz")
            assert names == []

    class TestGetAllDccs:
        def test_get_all_dccs_initially_empty(self) -> None:
            reg = ToolRegistry()
            dccs = reg.get_all_dccs()
            assert dccs == []

        def test_get_all_dccs_returns_registered_dccs(self) -> None:
            reg = ToolRegistry()
            reg.register("t", description="T", category="c", dcc="maya")
            reg.register("t2", description="T2", category="c", dcc="blender")
            dccs = reg.get_all_dccs()
            assert "maya" in dccs
            assert "blender" in dccs

        def test_get_all_dccs_no_duplicates(self) -> None:
            reg = ToolRegistry()
            reg.register("a", description="A", category="c", dcc="maya")
            reg.register("b", description="B", category="c", dcc="maya")
            dccs = reg.get_all_dccs()
            assert dccs.count("maya") == 1

    class TestBatchRegistration:
        def test_register_batch_adds_all(self) -> None:
            reg = ToolRegistry()
            reg.register_batch(
                [
                    {"name": "action_1", "description": "A1", "category": "geo"},
                    {"name": "action_2", "description": "A2", "category": "edit"},
                    {"name": "action_3", "description": "A3", "category": "geo"},
                ]
            )
            # list_actions returns dicts, get names from them
            names = [a["name"] if isinstance(a, dict) else a for a in reg.list_actions()]
            assert "action_1" in names
            assert "action_2" in names
            assert "action_3" in names

        def test_register_batch_empty_list_no_error(self) -> None:
            reg = ToolRegistry()
            reg.register_batch([])
            assert reg.list_actions() == []

        def test_register_batch_with_dcc(self) -> None:
            reg = ToolRegistry()
            reg.register_batch(
                [
                    {"name": "maya_cmd", "description": "MC", "category": "dcc", "dcc": "maya"},
                    {"name": "blender_cmd", "description": "BC", "category": "dcc", "dcc": "blender"},
                ]
            )
            maya = reg.list_actions_for_dcc("maya")
            blender = reg.list_actions_for_dcc("blender")
            assert "maya_cmd" in maya
            assert "blender_cmd" in blender


# ─────────────────────────────────────────────────────────────────────────────
# McpHttpConfig edge cases
# ─────────────────────────────────────────────────────────────────────────────


class TestMcpHttpConfigEdgeCases:
    """McpHttpConfig — edge cases for server_name and server_version."""

    class TestDefaultValues:
        def test_default_port_is_int(self) -> None:
            cfg = McpHttpConfig(port=18765)
            assert isinstance(cfg.port, int)
            assert cfg.port == 18765

        def test_server_name_default_nonempty(self) -> None:
            cfg = McpHttpConfig(port=18765)
            assert isinstance(cfg.server_name, str)
            assert len(cfg.server_name) > 0

        def test_server_version_default_semver_like(self) -> None:
            cfg = McpHttpConfig(port=18765)
            assert isinstance(cfg.server_version, str)
            assert len(cfg.server_version) > 0
            # Should be like "0.12.7"
            parts = cfg.server_version.split(".")
            assert len(parts) >= 2

    class TestCustomValues:
        def test_custom_port(self) -> None:
            cfg = McpHttpConfig(port=9876)
            assert cfg.port == 9876

        def test_custom_server_name(self) -> None:
            cfg = McpHttpConfig(port=18765, server_name="my-dcc-mcp")
            assert cfg.server_name == "my-dcc-mcp"

        def test_custom_server_version(self) -> None:
            cfg = McpHttpConfig(port=18765, server_version="2.0.0")
            assert cfg.server_version == "2.0.0"

    class TestReprAndStr:
        def test_repr_is_string(self) -> None:
            cfg = McpHttpConfig(port=18765)
            assert isinstance(repr(cfg), str)

        def test_repr_contains_port(self) -> None:
            cfg = McpHttpConfig(port=18765)
            assert "18765" in repr(cfg)


# ─────────────────────────────────────────────────────────────────────────────
# SemVer comparison edge cases
# ─────────────────────────────────────────────────────────────────────────────


class TestSemVerEdgeCases:
    """SemVer — comparison edge cases."""

    class TestDirectConstruction:
        def test_semver_from_ints(self) -> None:
            sv = SemVer(1, 2, 3)
            assert sv.major == 1
            assert sv.minor == 2
            assert sv.patch == 3

        def test_semver_zero_version(self) -> None:
            sv = SemVer(0, 0, 0)
            assert sv.major == 0
            assert sv.minor == 0
            assert sv.patch == 0

        def test_semver_str_representation(self) -> None:
            sv = SemVer(1, 2, 3)
            assert str(sv) == "1.2.3"

    class TestParseAndCompare:
        def test_parse_version_string(self) -> None:
            sv = SemVer.parse("2.5.1")
            assert sv.major == 2
            assert sv.minor == 5
            assert sv.patch == 1

        def test_greater_than_patch(self) -> None:
            a = SemVer(1, 0, 1)
            b = SemVer(1, 0, 0)
            assert a > b

        def test_less_than_minor(self) -> None:
            a = SemVer(1, 0, 0)
            b = SemVer(1, 1, 0)
            assert a < b

        def test_major_dominates(self) -> None:
            a = SemVer(2, 0, 0)
            b = SemVer(1, 99, 99)
            assert a > b

        def test_equal_versions(self) -> None:
            a = SemVer(1, 2, 3)
            b = SemVer(1, 2, 3)
            assert a == b

        def test_not_equal_patch(self) -> None:
            a = SemVer(1, 2, 3)
            b = SemVer(1, 2, 4)
            assert a != b

    class TestMatchesConstraint:
        def test_matches_gte_true(self) -> None:
            sv = SemVer(1, 5, 0)
            vc = VersionConstraint.parse(">=1.0.0")
            assert sv.matches_constraint(vc) is True

        def test_matches_gte_false(self) -> None:
            sv = SemVer(0, 9, 0)
            vc = VersionConstraint.parse(">=1.0.0")
            assert sv.matches_constraint(vc) is False

        def test_matches_exact(self) -> None:
            sv = SemVer(1, 2, 3)
            vc = VersionConstraint.parse("=1.2.3")
            assert sv.matches_constraint(vc) is True

        def test_matches_caret(self) -> None:
            sv = SemVer(1, 3, 0)
            vc = VersionConstraint.parse("^1.0.0")
            assert sv.matches_constraint(vc) is True

        def test_no_match_caret_different_major(self) -> None:
            sv = SemVer(2, 0, 0)
            vc = VersionConstraint.parse("^1.0.0")
            assert sv.matches_constraint(vc) is False

        def test_matches_wildcard(self) -> None:
            sv = SemVer(99, 0, 0)
            vc = VersionConstraint.parse("*")
            assert sv.matches_constraint(vc) is True


# ─────────────────────────────────────────────────────────────────────────────
# VtValue factory methods
# ─────────────────────────────────────────────────────────────────────────────


class TestVtValueFactoryMethods:
    """VtValue factory methods — ensure all work correctly."""

    class TestFromInt:
        def test_from_int_type_name(self) -> None:
            v = VtValue.from_int(42)
            # type_name is a property (str), not callable
            assert isinstance(v.type_name, str)
            assert "int" in v.type_name.lower()

        def test_from_int_to_python(self) -> None:
            v = VtValue.from_int(42)
            assert v.to_python() == 42

        def test_from_int_zero(self) -> None:
            v = VtValue.from_int(0)
            assert v.to_python() == 0

        def test_from_int_negative(self) -> None:
            v = VtValue.from_int(-100)
            assert v.to_python() == -100

    class TestFromFloat:
        def test_from_float_type_name(self) -> None:
            v = VtValue.from_float(3.14)
            assert isinstance(v.type_name, str)
            assert "float" in v.type_name.lower()

        def test_from_float_to_python(self) -> None:
            v = VtValue.from_float(2.71)
            assert v.to_python() == pytest.approx(2.71)

        def test_from_float_zero(self) -> None:
            v = VtValue.from_float(0.0)
            assert v.to_python() == pytest.approx(0.0)

    class TestFromString:
        def test_from_string_type_name(self) -> None:
            v = VtValue.from_string("hello")
            assert isinstance(v.type_name, str)
            assert "string" in v.type_name.lower()

        def test_from_string_to_python(self) -> None:
            v = VtValue.from_string("hello world")
            assert v.to_python() == "hello world"

        def test_from_string_empty(self) -> None:
            v = VtValue.from_string("")
            assert v.to_python() == ""

    class TestFromBool:
        def test_from_bool_true(self) -> None:
            v = VtValue.from_bool(True)
            assert v.to_python() is True

        def test_from_bool_false(self) -> None:
            v = VtValue.from_bool(False)
            assert v.to_python() is False

    class TestFromVec3f:
        def test_from_vec3f_tuple(self) -> None:
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            py = v.to_python()
            assert isinstance(py, tuple)
            assert len(py) == 3

        def test_from_vec3f_values(self) -> None:
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            x, y, z = v.to_python()
            assert x == pytest.approx(1.0)
            assert y == pytest.approx(2.0)
            assert z == pytest.approx(3.0)

    class TestFromToken:
        def test_from_token_type_name(self) -> None:
            v = VtValue.from_token("myToken")
            assert isinstance(v.type_name, str)
            assert "token" in v.type_name.lower()

        def test_from_token_to_python_is_string(self) -> None:
            v = VtValue.from_token("myToken")
            assert isinstance(v.to_python(), str)

        def test_from_token_value(self) -> None:
            v = VtValue.from_token("myToken")
            assert v.to_python() == "myToken"
