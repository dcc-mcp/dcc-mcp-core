"""Deep tests for CaptureFrame, SkillMetadata, TransportManager high-level APIs.

ActionRecorder/ActionMetrics/RecordingGuard, DccInfo, SceneInfo, SceneStatistics,
StringWrapper, PyCrashRecoveryPolicy, TimingMiddleware, AuditMiddleware,
RateLimitMiddleware (+167 tests).
"""

from __future__ import annotations

import tempfile
import time

import pytest

from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionMetrics
from dcc_mcp_core import ActionRecorder
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import CaptureFrame
from dcc_mcp_core import Capturer
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import DccInfo
from dcc_mcp_core import LoggingMiddleware
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import RateLimitMiddleware
from dcc_mcp_core import RecordingGuard
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import TimingMiddleware
from dcc_mcp_core import ToolDeclaration
from dcc_mcp_core import TransportManager

# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _make_capturer() -> Capturer:
    return Capturer.new_mock(width=640, height=480)


def _make_registry_dispatcher(action_name: str = "ping") -> tuple[ActionRegistry, ActionDispatcher]:
    reg = ActionRegistry()
    reg.register(action_name, description="test", category="util")
    d = ActionDispatcher(reg)
    d.register_handler(action_name, lambda _params: "pong")
    return reg, d


# ─────────────────────────────────────────────────────────────────────────────
# CaptureFrame
# ─────────────────────────────────────────────────────────────────────────────


class TestCaptureFrame:
    """Tests for CaptureFrame returned by Capturer.capture()."""

    def setup_method(self) -> None:
        self.capturer = _make_capturer()

    def test_capture_returns_capture_frame_png(self) -> None:
        frame = self.capturer.capture(format="png")
        assert isinstance(frame, CaptureFrame)

    def test_width_matches_mock_dimensions(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.width == 640

    def test_height_matches_mock_dimensions(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.height == 480

    def test_format_is_png(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.format == "png"

    def test_mime_type_is_image_png(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.mime_type == "image/png"

    def test_data_is_bytes(self) -> None:
        frame = self.capturer.capture(format="png")
        assert isinstance(frame.data, bytes)

    def test_byte_len_positive(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.byte_len() > 0

    def test_byte_len_equals_len_data(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.byte_len() == len(frame.data)

    def test_dpi_scale_is_float(self) -> None:
        frame = self.capturer.capture(format="png")
        assert isinstance(frame.dpi_scale, float)

    def test_dpi_scale_positive(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.dpi_scale > 0.0

    def test_timestamp_ms_is_int(self) -> None:
        frame = self.capturer.capture(format="png")
        assert isinstance(frame.timestamp_ms, int)

    def test_timestamp_ms_positive(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.timestamp_ms > 0

    def test_repr_contains_dimensions(self) -> None:
        frame = self.capturer.capture(format="png")
        r = repr(frame)
        assert "640" in r
        assert "480" in r

    def test_capture_jpeg_format_field(self) -> None:
        frame = self.capturer.capture(format="jpeg")
        assert frame.format == "jpeg"

    def test_capture_jpeg_mime_type(self) -> None:
        frame = self.capturer.capture(format="jpeg")
        assert "jpeg" in frame.mime_type or "image" in frame.mime_type

    def test_capture_raw_bgra_format_field(self) -> None:
        frame = self.capturer.capture(format="raw_bgra")
        assert frame.format == "raw_bgra"

    def test_png_data_starts_with_png_signature(self) -> None:
        frame = self.capturer.capture(format="png")
        assert frame.data[:4] == b"\x89PNG"

    def test_scale_0_5_reduces_dimensions(self) -> None:
        full = self.capturer.capture(format="raw_bgra", scale=1.0)
        half = self.capturer.capture(format="raw_bgra", scale=0.5)
        assert half.width <= full.width
        assert half.height <= full.height


class TestCapturerStats:
    """Tests for Capturer statistics."""

    def test_stats_returns_three_tuple(self) -> None:
        cap = Capturer.new_mock(100, 100)
        stats = cap.stats()
        assert isinstance(stats, tuple)
        assert len(stats) == 3

    def test_stats_initial_zero_captures(self) -> None:
        cap = Capturer.new_mock(100, 100)
        count, _total, errors = cap.stats()
        assert count == 0
        assert errors == 0

    def test_stats_increments_after_capture(self) -> None:
        cap = Capturer.new_mock(100, 100)
        cap.capture(format="png")
        cap.capture(format="png")
        count, total_bytes, errors = cap.stats()
        assert count == 2
        assert total_bytes > 0
        assert errors == 0

    def test_backend_name_is_string(self) -> None:
        cap = Capturer.new_mock()
        assert isinstance(cap.backend_name(), str)

    def test_mock_backend_name_contains_mock(self) -> None:
        cap = Capturer.new_mock()
        assert "Mock" in cap.backend_name() or "mock" in cap.backend_name().lower()


# ─────────────────────────────────────────────────────────────────────────────
# SkillMetadata deep
# ─────────────────────────────────────────────────────────────────────────────


class TestSkillMetadataFields:
    """Deep attribute tests for SkillMetadata."""

    def _make_full(self) -> SkillMetadata:
        return SkillMetadata(
            name="render-scene",
            description="Render current scene to disk",
            tools=[ToolDeclaration(name="render"), ToolDeclaration(name="preview")],
            dcc="maya",
            tags=["render", "output"],
            scripts=["render_scene.py"],
            skill_path="/skills/render-scene",
            version="3.1.0",
            depends=["scene-setup", "lighting"],
            metadata_files=["SKILL.md", "schema.json"],
        )

    def test_name_field(self) -> None:
        sm = self._make_full()
        assert sm.name == "render-scene"

    def test_description_field(self) -> None:
        sm = self._make_full()
        assert sm.description == "Render current scene to disk"

    def test_tools_list(self) -> None:
        sm = self._make_full()
        assert [t.name for t in sm.tools] == ["render", "preview"]

    def test_dcc_field(self) -> None:
        sm = self._make_full()
        assert sm.dcc == "maya"

    def test_tags_list(self) -> None:
        sm = self._make_full()
        assert "render" in sm.tags
        assert "output" in sm.tags

    def test_scripts_list(self) -> None:
        sm = self._make_full()
        assert sm.scripts == ["render_scene.py"]

    def test_skill_path_field(self) -> None:
        sm = self._make_full()
        assert sm.skill_path == "/skills/render-scene"

    def test_version_field(self) -> None:
        sm = self._make_full()
        assert sm.version == "3.1.0"

    def test_depends_list(self) -> None:
        sm = self._make_full()
        assert "scene-setup" in sm.depends
        assert "lighting" in sm.depends

    def test_metadata_files_list(self) -> None:
        sm = self._make_full()
        assert "SKILL.md" in sm.metadata_files
        assert "schema.json" in sm.metadata_files

    def test_defaults_are_sane(self) -> None:
        sm = SkillMetadata(name="minimal")
        assert sm.description == ""
        assert sm.tools == []
        assert sm.dcc == "python"
        assert sm.tags == []
        assert sm.scripts == []
        assert sm.skill_path == ""
        assert sm.version == "1.0.0"
        assert sm.depends == []
        assert sm.metadata_files == []

    def test_repr_contains_name(self) -> None:
        sm = self._make_full()
        assert "render-scene" in repr(sm)

    def test_str_contains_name(self) -> None:
        sm = self._make_full()
        assert "render-scene" in str(sm)

    def test_eq_same_name_same_dcc(self) -> None:
        a = SkillMetadata(name="skill-a", dcc="maya")
        b = SkillMetadata(name="skill-a", dcc="maya")
        assert a == b

    def test_eq_different_names(self) -> None:
        a = SkillMetadata(name="skill-a")
        b = SkillMetadata(name="skill-b")
        assert a != b


# ─────────────────────────────────────────────────────────────────────────────
# TransportManager high-level methods
# ─────────────────────────────────────────────────────────────────────────────


class TestTransportManagerHighLevel:
    """Tests for TransportManager.find_best_service, rank_services, etc."""

    def _make_mgr_with_services(self) -> tuple[TransportManager, str, str, str]:
        tmpdir = tempfile.mkdtemp()
        mgr = TransportManager(tmpdir, max_connections_per_dcc=5)
        id_maya1 = mgr.register_service("maya", "127.0.0.1", 18812, version="2025")
        id_maya2 = mgr.register_service("maya", "127.0.0.1", 18813, version="2024")
        id_blender = mgr.register_service("blender", "192.168.1.10", 19000)
        return mgr, id_maya1, id_maya2, id_blender

    def test_find_best_service_returns_service_entry(self) -> None:
        from dcc_mcp_core import ServiceEntry

        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            best = mgr.find_best_service("maya")
            assert isinstance(best, ServiceEntry)
        finally:
            mgr.shutdown()

    def test_find_best_service_correct_dcc_type(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            best = mgr.find_best_service("maya")
            assert best.dcc_type == "maya"
        finally:
            mgr.shutdown()

    def test_find_best_service_no_instances_raises(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            with pytest.raises(RuntimeError):
                mgr.find_best_service("nonexistent_dcc")
            mgr.shutdown()

    def test_rank_services_returns_list(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            ranked = mgr.rank_services("maya")
            assert isinstance(ranked, list)
        finally:
            mgr.shutdown()

    def test_rank_services_count_equals_maya_instances(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            ranked = mgr.rank_services("maya")
            assert len(ranked) == 2
        finally:
            mgr.shutdown()

    def test_rank_services_excludes_other_dcc(self) -> None:
        from dcc_mcp_core import ServiceEntry

        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            ranked = mgr.rank_services("maya")
            for entry in ranked:
                assert isinstance(entry, ServiceEntry)
                assert entry.dcc_type == "maya"
        finally:
            mgr.shutdown()

    def test_rank_services_no_instances_raises(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            with pytest.raises(RuntimeError):
                mgr.rank_services("nonexistent_dcc")
            mgr.shutdown()

    def test_update_service_status_returns_true(self) -> None:
        mgr, id1, _, _ = self._make_mgr_with_services()
        try:
            result = mgr.update_service_status("maya", id1, ServiceStatus.BUSY)
            assert result is True
        finally:
            mgr.shutdown()

    def test_update_service_status_unknown_returns_false(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            # Use a valid UUID format that simply doesn't exist in the registry
            result = mgr.update_service_status("maya", "00000000-0000-0000-0000-000000000000", ServiceStatus.BUSY)
            assert result is False
            mgr.shutdown()

    def test_pool_count_for_dcc_returns_int(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            count = mgr.pool_count_for_dcc("maya")
            assert isinstance(count, int)
        finally:
            mgr.shutdown()

    def test_pool_count_for_unknown_dcc_is_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            count = mgr.pool_count_for_dcc("unknown_dcc")
            assert count == 0
            mgr.shutdown()

    def test_get_or_create_session_routed_returns_uuid(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            sid = mgr.get_or_create_session_routed("maya")
            assert isinstance(sid, str)
            assert len(sid) == 36  # UUID format
        finally:
            mgr.shutdown()

    def test_get_or_create_session_routed_with_round_robin(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            sid = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
            assert isinstance(sid, str)
            assert len(sid) == 36
        finally:
            mgr.shutdown()

    def test_get_or_create_session_routed_with_first_available(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            sid = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.FIRST_AVAILABLE)
            assert isinstance(sid, str)
        finally:
            mgr.shutdown()

    def test_list_all_instances_alias(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            all_inst = mgr.list_all_instances()
            all_svc = mgr.list_all_services()
            assert len(all_inst) == len(all_svc)
        finally:
            mgr.shutdown()

    def test_list_all_instances_count(self) -> None:
        mgr, _, _, _ = self._make_mgr_with_services()
        try:
            all_inst = mgr.list_all_instances()
            assert len(all_inst) == 3  # 2 maya + 1 blender
        finally:
            mgr.shutdown()


# ─────────────────────────────────────────────────────────────────────────────
# ActionRecorder / ActionMetrics / RecordingGuard
# ─────────────────────────────────────────────────────────────────────────────


class TestActionRecorder:
    """Tests for ActionRecorder and ActionMetrics."""

    def test_start_returns_recording_guard(self) -> None:
        rec = ActionRecorder("scope")
        guard = rec.start("my_action", "maya")
        assert isinstance(guard, RecordingGuard)
        guard.finish(success=True)

    def test_metrics_returns_none_before_any_recording(self) -> None:
        rec = ActionRecorder("scope")
        assert rec.metrics("never_recorded") is None

    def test_metrics_returns_action_metrics_after_record(self) -> None:
        rec = ActionRecorder("scope")
        guard = rec.start("create_sphere", "maya")
        guard.finish(success=True)
        m = rec.metrics("create_sphere")
        assert isinstance(m, ActionMetrics)

    def test_action_name_field(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        assert rec.metrics("sphere").action_name == "sphere"

    def test_invocation_count_increments(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        rec.start("sphere", "maya").finish(success=True)
        assert rec.metrics("sphere").invocation_count == 2

    def test_success_count_tracks_successes(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        rec.start("sphere", "maya").finish(success=False)
        m = rec.metrics("sphere")
        assert m.success_count == 1
        assert m.failure_count == 1

    def test_success_rate_all_success(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        rec.start("sphere", "maya").finish(success=True)
        assert rec.metrics("sphere").success_rate() == 1.0

    def test_success_rate_all_failure(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=False)
        rec.start("sphere", "maya").finish(success=False)
        assert rec.metrics("sphere").success_rate() == 0.0

    def test_avg_duration_ms_is_float(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        assert isinstance(rec.metrics("sphere").avg_duration_ms, float)

    def test_p95_duration_ms_is_float(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        assert isinstance(rec.metrics("sphere").p95_duration_ms, float)

    def test_p99_duration_ms_is_float(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        assert isinstance(rec.metrics("sphere").p99_duration_ms, float)

    def test_all_metrics_returns_list(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("a", "maya").finish(success=True)
        rec.start("b", "maya").finish(success=True)
        all_m = rec.all_metrics()
        assert isinstance(all_m, list)
        assert len(all_m) == 2

    def test_reset_clears_metrics(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        rec.reset()
        assert rec.all_metrics() == []
        assert rec.metrics("sphere") is None

    def test_recording_guard_context_manager_success(self) -> None:
        rec = ActionRecorder("scope")
        with rec.start("ctx_action", "blender"):
            pass
        m = rec.metrics("ctx_action")
        assert m is not None
        assert m.invocation_count == 1

    def test_recording_guard_context_manager_exception(self) -> None:
        rec = ActionRecorder("scope")
        try:
            with rec.start("failing_action", "blender"):
                raise ValueError("intentional")
        except ValueError:
            pass
        m = rec.metrics("failing_action")
        assert m is not None
        assert m.invocation_count == 1

    def test_metrics_repr_contains_action_name(self) -> None:
        rec = ActionRecorder("scope")
        rec.start("sphere", "maya").finish(success=True)
        r = repr(rec.metrics("sphere"))
        assert "sphere" in r


# ─────────────────────────────────────────────────────────────────────────────
# DccInfo, SceneInfo, SceneStatistics, ScriptResult
# ─────────────────────────────────────────────────────────────────────────────


class TestDccInfo:
    """Tests for DccInfo."""

    def _make(self) -> DccInfo:
        return DccInfo(
            dcc_type="houdini",
            version="20.5",
            platform="linux",
            pid=9999,
            python_version="3.11.0",
            metadata={"renderer": "karma", "license": "apprentice"},
        )

    def test_dcc_type_field(self) -> None:
        assert self._make().dcc_type == "houdini"

    def test_version_field(self) -> None:
        assert self._make().version == "20.5"

    def test_platform_field(self) -> None:
        assert self._make().platform == "linux"

    def test_pid_field(self) -> None:
        assert self._make().pid == 9999

    def test_python_version_field(self) -> None:
        assert self._make().python_version == "3.11.0"

    def test_metadata_field(self) -> None:
        m = self._make().metadata
        assert m["renderer"] == "karma"

    def test_to_dict_has_dcc_type_key(self) -> None:
        d = self._make().to_dict()
        assert "dcc_type" in d

    def test_to_dict_has_pid_key(self) -> None:
        d = self._make().to_dict()
        assert "pid" in d

    def test_to_dict_correct_version(self) -> None:
        d = self._make().to_dict()
        assert d["version"] == "20.5"

    def test_python_version_optional_none(self) -> None:
        di = DccInfo(dcc_type="unreal", version="5.3", platform="windows", pid=1)
        assert di.python_version is None

    def test_repr_contains_dcc_type(self) -> None:
        r = repr(self._make())
        assert "houdini" in r


class TestSceneStatistics:
    """Tests for SceneStatistics."""

    def test_all_fields_set(self) -> None:
        s = SceneStatistics(
            object_count=5,
            vertex_count=200,
            polygon_count=100,
            material_count=3,
            texture_count=4,
            light_count=2,
            camera_count=1,
        )
        assert s.object_count == 5
        assert s.vertex_count == 200
        assert s.polygon_count == 100
        assert s.material_count == 3
        assert s.texture_count == 4
        assert s.light_count == 2
        assert s.camera_count == 1

    def test_defaults_are_zero(self) -> None:
        s = SceneStatistics()
        assert s.object_count == 0
        assert s.vertex_count == 0
        assert s.polygon_count == 0
        assert s.material_count == 0
        assert s.texture_count == 0
        assert s.light_count == 0
        assert s.camera_count == 0

    def test_repr_is_string(self) -> None:
        s = SceneStatistics(object_count=10)
        assert isinstance(repr(s), str)


class TestSceneInfo:
    """Tests for SceneInfo."""

    def _make(self) -> SceneInfo:
        stats = SceneStatistics(object_count=20, vertex_count=5000)
        return SceneInfo(
            file_path="/project/scene.hip",
            name="vfx_shot",
            modified=True,
            format="houdini",
            frame_range=(1001.0, 1100.0),
            current_frame=1042.0,
            fps=24.0,
            up_axis="Y",
            units="cm",
            statistics=stats,
            metadata={"renderer": "karma"},
        )

    def test_file_path_field(self) -> None:
        assert self._make().file_path == "/project/scene.hip"

    def test_name_field(self) -> None:
        assert self._make().name == "vfx_shot"

    def test_modified_true(self) -> None:
        assert self._make().modified is True

    def test_format_field(self) -> None:
        assert self._make().format == "houdini"

    def test_frame_range_is_tuple(self) -> None:
        fr = self._make().frame_range
        assert isinstance(fr, tuple)
        assert fr[0] == 1001.0
        assert fr[1] == 1100.0

    def test_current_frame_field(self) -> None:
        assert self._make().current_frame == 1042.0

    def test_fps_field(self) -> None:
        assert self._make().fps == 24.0

    def test_up_axis_field(self) -> None:
        assert self._make().up_axis == "Y"

    def test_units_field(self) -> None:
        assert self._make().units == "cm"

    def test_statistics_type(self) -> None:
        assert isinstance(self._make().statistics, SceneStatistics)

    def test_statistics_object_count(self) -> None:
        assert self._make().statistics.object_count == 20

    def test_metadata_field(self) -> None:
        assert self._make().metadata["renderer"] == "karma"

    def test_frame_range_none(self) -> None:
        si = SceneInfo()
        assert si.frame_range is None

    def test_current_frame_none(self) -> None:
        si = SceneInfo()
        assert si.current_frame is None

    def test_fps_none(self) -> None:
        si = SceneInfo()
        assert si.fps is None

    def test_up_axis_none(self) -> None:
        si = SceneInfo()
        assert si.up_axis is None

    def test_units_none(self) -> None:
        si = SceneInfo()
        assert si.units is None


# ─────────────────────────────────────────────────────────────────────────────
# StringWrapper
# ─────────────────────────────────────────────────────────────────────────────


class TestStringWrapper:
    """Tests for StringWrapper."""

    def test_value_field(self) -> None:
        sw = StringWrapper("hello")
        assert sw.value == "hello"

    def test_str_returns_inner_value(self) -> None:
        sw = StringWrapper("world")
        assert str(sw) == "world"

    def test_repr_is_string(self) -> None:
        sw = StringWrapper("test")
        assert isinstance(repr(sw), str)

    def test_hash_is_int(self) -> None:
        sw = StringWrapper("abc")
        assert isinstance(hash(sw), int)

    def test_empty_string_value(self) -> None:
        sw = StringWrapper("")
        assert sw.value == ""
        assert str(sw) == ""

    def test_unicode_value(self) -> None:
        sw = StringWrapper("你好世界")
        assert sw.value == "你好世界"


# ─────────────────────────────────────────────────────────────────────────────
# PyCrashRecoveryPolicy
# ─────────────────────────────────────────────────────────────────────────────


class TestPyCrashRecoveryPolicy:
    """Tests for PyCrashRecoveryPolicy."""

    def test_max_restarts_field(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=7)
        assert p.max_restarts == 7

    def test_default_max_restarts(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.max_restarts == 3

    def test_should_restart_crashed_true(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("crashed") is True

    def test_should_restart_unresponsive_true(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("unresponsive") is True

    def test_should_restart_running_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("running") is False

    def test_should_restart_stopped_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("stopped") is False

    def test_exponential_backoff_increases(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=10)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=60000)
        d0 = p.next_delay_ms("maya", 0)
        d1 = p.next_delay_ms("maya", 1)
        d2 = p.next_delay_ms("maya", 2)
        assert d1 > d0
        assert d2 >= d1

    def test_exponential_backoff_initial_value(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=10)
        p.use_exponential_backoff(initial_ms=500, max_delay_ms=30000)
        d0 = p.next_delay_ms("maya", 0)
        assert d0 == 500

    def test_fixed_backoff_constant_delay(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=10)
        p.use_fixed_backoff(delay_ms=3000)
        d0 = p.next_delay_ms("blender", 0)
        d1 = p.next_delay_ms("blender", 1)
        d2 = p.next_delay_ms("blender", 2)
        assert d0 == d1 == d2 == 3000

    def test_repr_contains_max_restarts(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        assert "5" in repr(p)

    def test_unknown_status_raises(self) -> None:
        p = PyCrashRecoveryPolicy()
        with pytest.raises(ValueError):
            p.should_restart("unknown_status_xyz")


# ─────────────────────────────────────────────────────────────────────────────
# TimingMiddleware (direct construction)
# ─────────────────────────────────────────────────────────────────────────────


class TestTimingMiddlewareDirect:
    """Tests for TimingMiddleware direct construction."""

    def test_can_construct_directly(self) -> None:
        tm = TimingMiddleware()
        assert isinstance(tm, TimingMiddleware)

    def test_repr_is_string(self) -> None:
        tm = TimingMiddleware()
        assert isinstance(repr(tm), str)

    def test_last_elapsed_ms_returns_none_for_unknown(self) -> None:
        tm = TimingMiddleware()
        assert tm.last_elapsed_ms("never_dispatched") is None

    def test_last_elapsed_ms_returns_int_after_dispatch(self) -> None:
        reg = ActionRegistry()
        reg.register("fast_action", description="fast")
        d = ActionDispatcher(reg)
        d.register_handler("fast_action", lambda _p: 42)

        from dcc_mcp_core import ActionPipeline

        pipeline = ActionPipeline(d)
        tm = pipeline.add_timing()
        pipeline.dispatch("fast_action", "{}")
        elapsed = tm.last_elapsed_ms("fast_action")
        assert isinstance(elapsed, int)
        assert elapsed >= 0


# ─────────────────────────────────────────────────────────────────────────────
# AuditMiddleware (direct construction)
# ─────────────────────────────────────────────────────────────────────────────


class TestAuditMiddlewareDirect:
    """Tests for AuditMiddleware direct construction."""

    def test_can_construct_with_record_params_false(self) -> None:
        am = AuditMiddleware(record_params=False)
        assert isinstance(am, AuditMiddleware)

    def test_can_construct_with_record_params_true(self) -> None:
        am = AuditMiddleware(record_params=True)
        assert isinstance(am, AuditMiddleware)

    def test_default_empty_records(self) -> None:
        am = AuditMiddleware()
        assert am.records() == []
        assert am.record_count() == 0

    def test_clear_is_idempotent(self) -> None:
        am = AuditMiddleware()
        am.clear()
        am.clear()
        assert am.record_count() == 0

    def test_repr_is_string(self) -> None:
        am = AuditMiddleware()
        assert isinstance(repr(am), str)

    def test_records_for_action_empty_for_unknown(self) -> None:
        am = AuditMiddleware()
        assert am.records_for_action("unknown") == []


# ─────────────────────────────────────────────────────────────────────────────
# RateLimitMiddleware (direct construction)
# ─────────────────────────────────────────────────────────────────────────────


class TestRateLimitMiddlewareDirect:
    """Tests for RateLimitMiddleware direct construction."""

    def test_can_construct_directly(self) -> None:
        rl = RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert isinstance(rl, RateLimitMiddleware)

    def test_max_calls_property(self) -> None:
        rl = RateLimitMiddleware(max_calls=5, window_ms=500)
        assert rl.max_calls == 5

    def test_window_ms_property(self) -> None:
        rl = RateLimitMiddleware(max_calls=5, window_ms=500)
        assert rl.window_ms == 500

    def test_call_count_zero_for_unknown(self) -> None:
        rl = RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert rl.call_count("unknown_action") == 0

    def test_repr_contains_max_calls(self) -> None:
        rl = RateLimitMiddleware(max_calls=7, window_ms=2000)
        assert "7" in repr(rl)

    def test_repr_contains_window_ms(self) -> None:
        rl = RateLimitMiddleware(max_calls=7, window_ms=2000)
        assert "2000" in repr(rl)


# ─────────────────────────────────────────────────────────────────────────────
# DccCapabilities deep
# ─────────────────────────────────────────────────────────────────────────────


class TestDccCapabilities:
    """Deep tests for DccCapabilities."""

    def test_default_scene_info_false(self) -> None:
        caps = DccCapabilities()
        assert caps.scene_info is False

    def test_default_snapshot_false(self) -> None:
        caps = DccCapabilities()
        assert caps.snapshot is False

    def test_default_undo_redo_false(self) -> None:
        caps = DccCapabilities()
        assert caps.undo_redo is False

    def test_default_progress_reporting_false(self) -> None:
        caps = DccCapabilities()
        assert caps.progress_reporting is False

    def test_default_file_operations_false(self) -> None:
        caps = DccCapabilities()
        assert caps.file_operations is False

    def test_default_selection_false(self) -> None:
        caps = DccCapabilities()
        assert caps.selection is False

    def test_default_extensions_empty_dict(self) -> None:
        caps = DccCapabilities()
        assert caps.extensions == {}

    def test_set_scene_info_true(self) -> None:
        caps = DccCapabilities(scene_info=True)
        assert caps.scene_info is True

    def test_set_snapshot_true(self) -> None:
        caps = DccCapabilities(snapshot=True)
        assert caps.snapshot is True

    def test_set_all_bool_flags(self) -> None:
        caps = DccCapabilities(
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            progress_reporting=True,
            file_operations=True,
            selection=True,
        )
        assert all(
            [
                caps.scene_info,
                caps.snapshot,
                caps.undo_redo,
                caps.progress_reporting,
                caps.file_operations,
                caps.selection,
            ]
        )

    def test_script_languages_list(self) -> None:
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL])
        assert len(caps.script_languages) == 2

    def test_script_languages_contains_python(self) -> None:
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON])
        assert ScriptLanguage.PYTHON in caps.script_languages

    def test_extensions_dict(self) -> None:
        caps = DccCapabilities(extensions={"bifrost": True, "xgen": False})
        assert caps.extensions["bifrost"] is True
        assert caps.extensions["xgen"] is False

    def test_repr_is_string(self) -> None:
        caps = DccCapabilities(scene_info=True)
        assert isinstance(repr(caps), str)


# ─────────────────────────────────────────────────────────────────────────────
# ScriptResult
# ─────────────────────────────────────────────────────────────────────────────


class TestScriptResult:
    """Tests for ScriptResult."""

    def test_success_field_true(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=10, output="done")
        assert sr.success is True

    def test_success_field_false(self) -> None:
        sr = ScriptResult(success=False, execution_time_ms=5, error="failed")
        assert sr.success is False

    def test_output_field(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=20, output="result_data")
        assert sr.output == "result_data"

    def test_error_field(self) -> None:
        sr = ScriptResult(success=False, execution_time_ms=1, error="NameError: x")
        assert sr.error == "NameError: x"

    def test_execution_time_ms_field(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=42)
        assert sr.execution_time_ms == 42

    def test_context_field(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=1, context={"frame": "42"})
        assert sr.context["frame"] == "42"

    def test_defaults_none_output(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=0)
        assert sr.output is None

    def test_defaults_none_error(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=0)
        assert sr.error is None

    def test_to_dict_has_success_key(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=10)
        d = sr.to_dict()
        assert "success" in d

    def test_repr_is_string(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5)
        assert isinstance(repr(sr), str)


# ─────────────────────────────────────────────────────────────────────────────
# DccError
# ─────────────────────────────────────────────────────────────────────────────


class TestDccError:
    """Tests for DccError."""

    def test_code_field(self) -> None:
        e = DccError(code=DccErrorCode.CONNECTION_FAILED, message="connect failed")
        assert e.code == DccErrorCode.CONNECTION_FAILED

    def test_message_field(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timed out after 5s")
        assert e.message == "timed out after 5s"

    def test_details_none_default(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="internal")
        assert e.details is None

    def test_details_with_value(self) -> None:
        e = DccError(code=DccErrorCode.SCRIPT_ERROR, message="err", details="line 42")
        assert e.details == "line 42"

    def test_recoverable_false_default(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="x")
        assert e.recoverable is False

    def test_recoverable_true(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timeout", recoverable=True)
        assert e.recoverable is True

    def test_str_contains_message(self) -> None:
        e = DccError(code=DccErrorCode.PERMISSION_DENIED, message="not allowed")
        assert "not allowed" in str(e)

    def test_repr_is_string(self) -> None:
        e = DccError(code=DccErrorCode.INVALID_INPUT, message="bad input")
        assert isinstance(repr(e), str)

    def test_all_error_codes_constructible(self) -> None:
        codes = [
            DccErrorCode.CONNECTION_FAILED,
            DccErrorCode.TIMEOUT,
            DccErrorCode.SCRIPT_ERROR,
            DccErrorCode.NOT_RESPONDING,
            DccErrorCode.UNSUPPORTED,
            DccErrorCode.PERMISSION_DENIED,
            DccErrorCode.INVALID_INPUT,
            DccErrorCode.SCENE_ERROR,
            DccErrorCode.INTERNAL,
        ]
        for code in codes:
            e = DccError(code=code, message="test")
            assert e.code == code
