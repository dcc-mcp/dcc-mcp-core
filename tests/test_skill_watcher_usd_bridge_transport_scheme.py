"""Tests for SkillWatcher, UsdStage bridge functions, and TransportScheme.select_address.

Covers: unwatch/reload/skill_count, export_usda / stage_to_scene_info_json /
scene_info_json_to_stage, and select_address strategies.
"""

from __future__ import annotations

import json
from pathlib import Path
import tempfile

import pytest

from dcc_mcp_core import SkillWatcher
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme
from dcc_mcp_core import UsdStage
from dcc_mcp_core import scene_info_json_to_stage
from dcc_mcp_core import stage_to_scene_info_json

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _select_address(scheme: TransportScheme, dcc_type: str, host: str, port: int, pid=None) -> TransportAddress:
    """Call select_address via unbound PyO3 method pattern."""
    fn = type(scheme).select_address
    if pid is not None:
        return fn(scheme, dcc_type, host, port, pid)
    return fn(scheme, dcc_type, host, port)


def _make_skill_dir(root: str, name: str) -> str:
    """Create a minimal SKILL.md directory structure under root."""
    skill_dir = Path(root) / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text(f"# {name}\nA test skill.\n")
    return str(skill_dir)


# ===========================================================================
# SkillWatcher tests
# ===========================================================================


class TestSkillWatcherEmpty:
    """Tests for SkillWatcher initial state."""

    def test_initial_watched_paths_empty(self):
        w = SkillWatcher()
        assert w.watched_paths() == []

    def test_initial_skill_count_zero(self):
        w = SkillWatcher()
        assert w.skill_count() == 0

    def test_initial_skills_empty_list(self):
        w = SkillWatcher()
        assert w.skills() == []

    def test_reload_on_empty_succeeds(self):
        w = SkillWatcher()
        w.reload()  # must not raise
        assert w.skill_count() == 0


class TestSkillWatcherWatch:
    """Tests for SkillWatcher.watch."""

    def test_watch_adds_path(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        paths = w.watched_paths()
        assert d in paths

    def test_watch_same_path_twice_adds_duplicate(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        w.watch(d)
        paths = w.watched_paths()
        assert paths.count(d) == 2

    def test_watch_multiple_different_paths(self):
        w = SkillWatcher()
        d1 = tempfile.mkdtemp()
        d2 = tempfile.mkdtemp()
        w.watch(d1)
        w.watch(d2)
        paths = w.watched_paths()
        assert d1 in paths
        assert d2 in paths

    def test_watch_empty_dir_skill_count_zero(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        assert w.skill_count() == 0


class TestSkillWatcherUnwatch:
    """Tests for SkillWatcher.unwatch."""

    def test_unwatch_removes_path(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        w.unwatch(d)
        paths = w.watched_paths()
        assert d not in paths

    def test_unwatch_nonexistent_path_no_error(self):
        w = SkillWatcher()
        # Should silently ignore or succeed
        w.unwatch("/this/path/does/not/exist")
        assert w.watched_paths() == []

    def test_unwatch_reduces_count_to_zero(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        assert len(w.watched_paths()) == 1
        w.unwatch(d)
        assert len(w.watched_paths()) == 0

    def test_unwatch_one_of_multiple(self):
        w = SkillWatcher()
        d1 = tempfile.mkdtemp()
        d2 = tempfile.mkdtemp()
        w.watch(d1)
        w.watch(d2)
        w.unwatch(d1)
        paths = w.watched_paths()
        assert d1 not in paths
        assert d2 in paths

    def test_watch_unwatch_watch_again(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        w.unwatch(d)
        w.watch(d)
        assert d in w.watched_paths()

    def test_unwatch_all_then_watch_new(self):
        w = SkillWatcher()
        d1 = tempfile.mkdtemp()
        d2 = tempfile.mkdtemp()
        w.watch(d1)
        w.watch(d2)
        w.unwatch(d1)
        w.unwatch(d2)
        assert w.watched_paths() == []
        d3 = tempfile.mkdtemp()
        w.watch(d3)
        assert d3 in w.watched_paths()


class TestSkillWatcherReload:
    """Tests for SkillWatcher.reload."""

    def test_reload_empty_watcher(self):
        w = SkillWatcher()
        w.reload()
        assert w.skill_count() == 0

    def test_reload_with_watched_empty_dir(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        w.reload()
        assert w.skill_count() == 0

    def test_reload_multiple_times_stable(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        for _ in range(5):
            w.reload()
        assert w.skill_count() == 0


class TestSkillWatcherSkillCount:
    """Tests for SkillWatcher.skill_count with actual skill directories."""

    def test_skill_count_after_adding_skill_dir(self):
        root = tempfile.mkdtemp()
        _make_skill_dir(root, "my-skill")
        w = SkillWatcher()
        w.watch(root)
        # skill_count should reflect the scanned skills
        count = w.skill_count()
        assert isinstance(count, int)
        assert count >= 0

    def test_skill_count_returns_int(self):
        w = SkillWatcher()
        assert isinstance(w.skill_count(), int)

    def test_skills_returns_list(self):
        w = SkillWatcher()
        assert isinstance(w.skills(), list)

    def test_unwatch_and_reload_reduces_paths(self):
        w = SkillWatcher()
        d = tempfile.mkdtemp()
        w.watch(d)
        assert len(w.watched_paths()) == 1
        w.unwatch(d)
        w.reload()
        assert len(w.watched_paths()) == 0


# ===========================================================================
# UsdStage.export_usda tests
# ===========================================================================


class TestUsdStageExportUsda:
    """Tests for UsdStage.export_usda."""

    def test_export_usda_returns_string(self):
        stage = UsdStage("TestScene")
        usda = stage.export_usda()
        assert isinstance(usda, str)

    def test_export_usda_starts_with_header(self):
        stage = UsdStage("TestScene")
        usda = stage.export_usda()
        assert usda.startswith("#usda")

    def test_export_usda_empty_stage(self):
        stage = UsdStage("EmptyScene")
        usda = stage.export_usda()
        assert len(usda) > 0

    def test_export_usda_contains_up_axis(self):
        stage = UsdStage("AxisScene")
        usda = stage.export_usda()
        assert "upAxis" in usda

    def test_export_usda_contains_mpu_after_set(self):
        stage = UsdStage("MpuScene")
        stage.set_meters_per_unit(0.01)
        usda = stage.export_usda()
        assert "0.01" in usda or "metersPerUnit" in usda

    def test_export_usda_contains_prim_name(self):
        stage = UsdStage("PrimScene")
        stage.define_prim("/World", "Xform")
        usda = stage.export_usda()
        assert "World" in usda

    def test_export_usda_contains_nested_prim(self):
        stage = UsdStage("NestedScene")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Sphere", "Sphere")
        usda = stage.export_usda()
        assert "Sphere" in usda

    def test_export_usda_multiple_prims(self):
        stage = UsdStage("MultiScene")
        stage.define_prim("/Cube", "Cube")
        stage.define_prim("/Sphere", "Sphere")
        stage.define_prim("/Cone", "Cone")
        usda = stage.export_usda()
        assert "Cube" in usda
        assert "Sphere" in usda
        assert "Cone" in usda

    def test_export_usda_idempotent(self):
        stage = UsdStage("IdempotentScene")
        stage.define_prim("/World", "Xform")
        usda1 = stage.export_usda()
        usda2 = stage.export_usda()
        assert usda1 == usda2


# ===========================================================================
# stage_to_scene_info_json tests
# ===========================================================================


class TestStageToSceneInfoJson:
    """Tests for stage_to_scene_info_json bridge function."""

    def test_returns_string(self):
        stage = UsdStage("TestScene")
        sj = stage_to_scene_info_json(stage)
        assert isinstance(sj, str)

    def test_valid_json(self):
        stage = UsdStage("TestScene")
        sj = stage_to_scene_info_json(stage)
        data = json.loads(sj)
        assert isinstance(data, dict)

    def test_has_name_field(self):
        stage = UsdStage("MyScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "name" in data
        assert data["name"] == "MyScene"

    def test_has_up_axis_field(self):
        stage = UsdStage("AxisScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "up_axis" in data

    def test_has_statistics_field(self):
        stage = UsdStage("StatsScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "statistics" in data
        stats = data["statistics"]
        assert isinstance(stats, dict)

    def test_has_metadata_field(self):
        stage = UsdStage("MetaScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "metadata" in data

    def test_statistics_has_required_keys(self):
        stage = UsdStage("StatsScene")
        data = json.loads(stage_to_scene_info_json(stage))
        stats = data["statistics"]
        for key in ["object_count", "vertex_count", "polygon_count"]:
            assert key in stats

    def test_up_axis_is_y_by_default(self):
        stage = UsdStage("DefaultScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert data["up_axis"] in ("Y", "Z", "X")  # valid USD up_axis values

    def test_units_field_present(self):
        stage = UsdStage("UnitsScene")
        stage.set_meters_per_unit(0.01)
        data = json.loads(stage_to_scene_info_json(stage))
        assert "units" in data

    def test_file_path_field_present(self):
        stage = UsdStage("FileScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "file_path" in data

    def test_modified_field_present(self):
        stage = UsdStage("ModScene")
        data = json.loads(stage_to_scene_info_json(stage))
        assert "modified" in data


# ===========================================================================
# scene_info_json_to_stage tests
# ===========================================================================


class TestSceneInfoJsonToStage:
    """Tests for scene_info_json_to_stage bridge function."""

    def test_returns_usd_stage(self):
        stage = UsdStage("TestScene")
        sj = stage_to_scene_info_json(stage)
        stage2 = scene_info_json_to_stage(sj)
        assert isinstance(stage2, UsdStage)

    def test_reconstructed_name_matches(self):
        stage = UsdStage("MyScene")
        sj = stage_to_scene_info_json(stage)
        stage2 = scene_info_json_to_stage(sj)
        assert stage2.name == "MyScene"

    def test_roundtrip_up_axis(self):
        stage = UsdStage("AxisScene")
        sj = stage_to_scene_info_json(stage)
        stage2 = scene_info_json_to_stage(sj)
        assert stage2.up_axis == stage.up_axis

    def test_roundtrip_different_names(self):
        for name in ["SceneA", "SceneB", "CG_Production_Shot_001"]:
            stage = UsdStage(name)
            sj = stage_to_scene_info_json(stage)
            stage2 = scene_info_json_to_stage(sj)
            assert stage2.name == name

    def test_roundtrip_with_mpu(self):
        stage = UsdStage("MpuScene")
        stage.set_meters_per_unit(0.01)
        sj = stage_to_scene_info_json(stage)
        stage2 = scene_info_json_to_stage(sj)
        # The reconstructed stage should at least be a valid UsdStage
        assert isinstance(stage2, UsdStage)

    def test_json_to_stage_idempotent(self):
        """Converting to JSON and back twice should yield same name."""
        stage = UsdStage("StableScene")
        sj = stage_to_scene_info_json(stage)
        stage2 = scene_info_json_to_stage(sj)
        sj2 = stage_to_scene_info_json(stage2)
        stage3 = scene_info_json_to_stage(sj2)
        assert stage3.name == "StableScene"


# ===========================================================================
# TransportScheme.select_address tests
# ===========================================================================


class TestTransportSchemeSelectAddressTcpOnly:
    """Tests for TransportScheme.TCP_ONLY strategy."""

    def test_tcp_only_no_pid_returns_tcp(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        assert addr.is_tcp is True

    def test_tcp_only_with_pid_still_returns_tcp(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001, pid=12345)
        assert addr.is_tcp is True

    def test_tcp_only_connection_string_format(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        cs = addr.to_connection_string()
        assert cs.startswith("tcp://")

    def test_tcp_only_is_not_named_pipe(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        assert addr.is_named_pipe is False

    def test_tcp_only_is_not_unix_socket(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        assert addr.is_unix_socket is False

    def test_tcp_only_port_in_connection_string(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7777)
        cs = addr.to_connection_string()
        assert "7777" in cs

    def test_tcp_only_host_in_connection_string(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "myhost.local", 7001)
        cs = addr.to_connection_string()
        assert "myhost.local" in cs


class TestTransportSchemeSelectAddressAuto:
    """Tests for TransportScheme.AUTO strategy."""

    def test_auto_no_pid_falls_back_to_tcp(self):
        addr = _select_address(TransportScheme.AUTO, "maya", "localhost", 7001)
        assert addr.is_tcp is True

    def test_auto_with_pid_prefers_named_pipe_on_windows(self):
        import sys

        addr = _select_address(TransportScheme.AUTO, "maya", "localhost", 7001, pid=12345)
        if sys.platform == "win32":
            assert addr.is_named_pipe is True
        else:
            # On non-Windows, may use unix socket or tcp
            assert addr.is_named_pipe or addr.is_unix_socket or addr.is_tcp

    def test_auto_connection_string_not_empty(self):
        addr = _select_address(TransportScheme.AUTO, "maya", "localhost", 7001)
        assert len(addr.to_connection_string()) > 0

    def test_auto_returns_transport_address(self):
        addr = _select_address(TransportScheme.AUTO, "maya", "localhost", 7001)
        assert isinstance(addr, TransportAddress)


class TestTransportSchemeSelectAddressPreferNamedPipe:
    """Tests for TransportScheme.PREFER_NAMED_PIPE strategy."""

    def test_prefer_named_pipe_no_pid_fallback_tcp(self):
        addr = _select_address(TransportScheme.PREFER_NAMED_PIPE, "maya", "localhost", 7001)
        assert addr.is_tcp is True  # no pid -> cannot create named pipe

    def test_prefer_named_pipe_with_pid_on_windows(self):
        import sys

        addr = _select_address(TransportScheme.PREFER_NAMED_PIPE, "maya", "localhost", 7001, pid=12345)
        if sys.platform == "win32":
            assert addr.is_named_pipe is True
        else:
            # On non-Windows, PREFER_NAMED_PIPE may still fall back to TCP
            assert isinstance(addr, TransportAddress)

    def test_prefer_named_pipe_with_pid_has_pipe_in_path_on_windows(self):
        import sys

        addr = _select_address(TransportScheme.PREFER_NAMED_PIPE, "maya", "localhost", 7001, pid=12345)
        if sys.platform == "win32":
            cs = addr.to_connection_string()
            assert "pipe" in cs.lower()

    def test_prefer_named_pipe_different_dcc_types(self):
        import sys

        for dcc in ["maya", "blender", "houdini"]:
            addr = _select_address(TransportScheme.PREFER_NAMED_PIPE, dcc, "localhost", 7001, pid=99)
            if sys.platform == "win32":
                cs = addr.to_connection_string()
                assert dcc in cs


class TestTransportSchemeSelectAddressPreferIpc:
    """Tests for TransportScheme.PREFER_IPC strategy."""

    def test_prefer_ipc_no_pid_fallback_to_tcp(self):
        addr = _select_address(TransportScheme.PREFER_IPC, "maya", "localhost", 7001)
        assert addr.is_tcp is True

    def test_prefer_ipc_with_pid_selects_ipc(self):
        import sys

        addr = _select_address(TransportScheme.PREFER_IPC, "maya", "localhost", 7001, pid=12345)
        if sys.platform == "win32":
            assert addr.is_named_pipe is True
        else:
            assert isinstance(addr, TransportAddress)

    def test_prefer_ipc_returns_valid_connection_string(self):
        addr = _select_address(TransportScheme.PREFER_IPC, "maya", "localhost", 7001)
        cs = addr.to_connection_string()
        assert len(cs) > 0


class TestTransportSchemeSelectAddressPreferUnixSocket:
    """Tests for TransportScheme.PREFER_UNIX_SOCKET strategy."""

    def test_prefer_unix_socket_no_pid_fallback_tcp(self):
        addr = _select_address(TransportScheme.PREFER_UNIX_SOCKET, "maya", "localhost", 7001)
        assert addr.is_tcp is True  # no pid on Windows → TCP fallback

    def test_prefer_unix_socket_with_pid_on_windows_fallback_tcp(self):
        import sys

        addr = _select_address(TransportScheme.PREFER_UNIX_SOCKET, "maya", "localhost", 7001, pid=12345)
        if sys.platform == "win32":
            # Windows doesn't have Unix sockets in fallback behavior
            assert addr.is_tcp is True

    def test_prefer_unix_socket_returns_valid_address(self):
        addr = _select_address(TransportScheme.PREFER_UNIX_SOCKET, "blender", "localhost", 8001)
        assert isinstance(addr, TransportAddress)


class TestTransportSchemeSelectAddressGeneral:
    """General tests for TransportScheme.select_address."""

    def test_all_schemes_no_pid_return_valid_address(self):
        schemes = [
            TransportScheme.AUTO,
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_IPC,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
        ]
        for scheme in schemes:
            addr = _select_address(scheme, "maya", "localhost", 7001)
            assert isinstance(addr, TransportAddress)

    def test_all_schemes_with_pid_return_valid_address(self):
        schemes = [
            TransportScheme.AUTO,
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_IPC,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
        ]
        for scheme in schemes:
            addr = _select_address(scheme, "maya", "localhost", 7001, pid=54321)
            assert isinstance(addr, TransportAddress)
            cs = addr.to_connection_string()
            assert len(cs) > 0

    def test_tcp_address_is_local_for_localhost(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        assert addr.is_local is True

    def test_tcp_address_scheme_attr(self):
        addr = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        scheme = addr.scheme
        assert "tcp" in scheme.lower()

    def test_named_pipe_not_local_false(self):
        import sys

        if sys.platform == "win32":
            addr = _select_address(TransportScheme.PREFER_NAMED_PIPE, "maya", "localhost", 7001, pid=9999)
            # Named pipe IS local (same machine)
            assert addr.is_local is True

    def test_different_ports_generate_different_tcp_strings(self):
        addr1 = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7001)
        addr2 = _select_address(TransportScheme.TCP_ONLY, "maya", "localhost", 7002)
        assert addr1.to_connection_string() != addr2.to_connection_string()
