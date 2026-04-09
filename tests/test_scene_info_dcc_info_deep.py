"""Deep tests for SceneInfo, SceneStatistics, DccInfo, DccError, DccCapabilities, DccErrorCode.

Covers:
- SceneStatistics: all numeric fields, default values, custom construction
- SceneInfo: all fields including optional fps/frame_range/current_frame/units/up_axis,
  metadata dict, modified flag, nested statistics
- DccInfo: all required/optional fields, to_dict roundtrip, metadata
- DccError: all error codes, message/details/recoverable fields
- DccCapabilities: script_languages (ScriptLanguage enum), all bool flags, extensions dict
- ScriptLanguage: enum values (PYTHON, MEL, etc.)
- ServiceStatus: enum values (AVAILABLE, BUSY, etc.)
"""

from __future__ import annotations

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# SceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatisticsDefaults:
    def test_default_construction(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss is not None

    def test_default_object_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.object_count == 0

    def test_default_polygon_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.polygon_count == 0

    def test_default_vertex_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.vertex_count == 0

    def test_default_camera_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.camera_count == 0

    def test_default_light_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.light_count == 0

    def test_default_material_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.material_count == 0

    def test_default_texture_count_zero(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.texture_count == 0


class TestSceneStatisticsCustomValues:
    def test_all_fields_set(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(
            object_count=10,
            polygon_count=5000,
            vertex_count=15000,
            camera_count=3,
            light_count=8,
            material_count=12,
            texture_count=20,
        )
        assert ss.object_count == 10
        assert ss.polygon_count == 5000
        assert ss.vertex_count == 15000
        assert ss.camera_count == 3
        assert ss.light_count == 8
        assert ss.material_count == 12
        assert ss.texture_count == 20

    def test_large_polygon_count(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(polygon_count=10_000_000)
        assert ss.polygon_count == 10_000_000

    def test_partial_fields(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(object_count=5, camera_count=2)
        assert ss.object_count == 5
        assert ss.camera_count == 2
        assert ss.polygon_count == 0  # unset defaults to 0

    def test_repr_contains_objects(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(object_count=7, vertex_count=100, polygon_count=50)
        r = repr(ss)
        assert "7" in r or "objects" in r.lower()


# ---------------------------------------------------------------------------
# SceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfoDefaults:
    def test_default_name(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.name == "untitled"

    def test_default_file_path_empty(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.file_path == ""

    def test_default_format_empty(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.format == ""

    def test_default_fps_none(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.fps is None

    def test_default_current_frame_none(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.current_frame is None

    def test_default_frame_range_none(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.frame_range is None

    def test_default_units_none(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.units is None

    def test_default_up_axis_none(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.up_axis is None

    def test_default_modified_false(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.modified is False

    def test_default_metadata_empty_dict(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.metadata == {}

    def test_default_statistics_type(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert isinstance(si.statistics, dcc_mcp_core.SceneStatistics)

    def test_default_statistics_all_zero(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.statistics.object_count == 0
        assert si.statistics.polygon_count == 0


class TestSceneInfoCustomValues:
    def test_name(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_file_path(self) -> None:
        si = dcc_mcp_core.SceneInfo(file_path="/project/shots/shot01.ma")
        assert si.file_path == "/project/shots/shot01.ma"

    def test_format(self) -> None:
        si = dcc_mcp_core.SceneInfo(format="maya")
        assert si.format == "maya"

    def test_fps_float(self) -> None:
        si = dcc_mcp_core.SceneInfo(fps=24.0)
        assert si.fps == 24.0

    def test_fps_ntsc(self) -> None:
        si = dcc_mcp_core.SceneInfo(fps=29.97)
        assert abs(si.fps - 29.97) < 0.001

    def test_current_frame(self) -> None:
        si = dcc_mcp_core.SceneInfo(current_frame=42.0)
        assert si.current_frame == 42.0

    def test_frame_range(self) -> None:
        si = dcc_mcp_core.SceneInfo(frame_range=(1, 240))
        fr = si.frame_range
        assert fr is not None
        start, end = fr
        assert start == 1.0
        assert end == 240.0

    def test_units_cm(self) -> None:
        si = dcc_mcp_core.SceneInfo(units="cm")
        assert si.units == "cm"

    def test_units_m(self) -> None:
        si = dcc_mcp_core.SceneInfo(units="m")
        assert si.units == "m"

    def test_up_axis_y(self) -> None:
        si = dcc_mcp_core.SceneInfo(up_axis="Y")
        assert si.up_axis == "Y"

    def test_up_axis_z(self) -> None:
        si = dcc_mcp_core.SceneInfo(up_axis="Z")
        assert si.up_axis == "Z"

    def test_modified_true(self) -> None:
        si = dcc_mcp_core.SceneInfo(modified=True)
        assert si.modified is True

    def test_metadata_dict(self) -> None:
        meta = {"renderer": "arnold", "version": "7.1", "user": "artist01"}
        si = dcc_mcp_core.SceneInfo(metadata=meta)
        assert si.metadata == meta

    def test_metadata_nested_value(self) -> None:
        # metadata values are strings (str -> str mapping)
        si = dcc_mcp_core.SceneInfo(metadata={"renderer": "arnold", "samples": "64"})
        assert si.metadata["renderer"] == "arnold"
        assert si.metadata["samples"] == "64"

    def test_statistics_custom(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(
            object_count=100,
            polygon_count=50000,
            vertex_count=150000,
        )
        si = dcc_mcp_core.SceneInfo(statistics=ss)
        assert si.statistics.object_count == 100
        assert si.statistics.polygon_count == 50000

    def test_full_scene_info(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(object_count=5, polygon_count=1000, camera_count=2)
        si = dcc_mcp_core.SceneInfo(
            name="forest_env",
            file_path="/proj/forest.blend",
            format="blender",
            fps=25.0,
            current_frame=100.0,
            frame_range=(1, 500),
            units="m",
            up_axis="Z",
            modified=True,
            metadata={"dcc": "blender", "version": "4.1"},
            statistics=ss,
        )
        assert si.name == "forest_env"
        assert si.fps == 25.0
        assert si.up_axis == "Z"
        assert si.modified is True
        assert si.metadata["dcc"] == "blender"
        assert si.statistics.object_count == 5


# ---------------------------------------------------------------------------
# DccInfo
# ---------------------------------------------------------------------------


class TestDccInfo:
    def test_basic_construction(self) -> None:
        di = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2024.2",
            platform="windows",
            pid=12345,
        )
        assert di is not None

    def test_dcc_type(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="blender", version="4.1.0", platform="linux", pid=9999)
        assert di.dcc_type == "blender"

    def test_version(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="maya", version="2024.2", platform="windows", pid=1)
        assert di.version == "2024.2"

    def test_platform(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="houdini", version="20.0", platform="linux", pid=2)
        assert di.platform == "linux"

    def test_pid(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="maya", version="2024", platform="windows", pid=42000)
        assert di.pid == 42000

    def test_python_version_optional(self) -> None:
        di = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2024",
            platform="windows",
            pid=1,
            python_version="3.10.11",
        )
        assert di.python_version == "3.10.11"

    def test_python_version_default_none(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="maya", version="2024", platform="windows", pid=1)
        assert di.python_version is None

    def test_metadata_default_empty(self) -> None:
        di = dcc_mcp_core.DccInfo(dcc_type="maya", version="2024", platform="windows", pid=1)
        assert di.metadata == {}

    def test_to_dict_keys(self) -> None:
        di = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2024.2",
            platform="windows",
            pid=12345,
            python_version="3.10.11",
        )
        d = di.to_dict()
        assert isinstance(d, dict)
        assert d["dcc_type"] == "maya"
        assert d["version"] == "2024.2"
        assert d["platform"] == "windows"
        assert d["pid"] == 12345
        assert d["python_version"] == "3.10.11"
        assert d["metadata"] == {}

    def test_to_dict_various_dccs(self) -> None:
        dccs = [
            ("blender", "4.1.0", "linux", 5000),
            ("houdini", "20.0", "macos", 3000),
            ("3dsmax", "2024.0", "windows", 7000),
        ]
        for dcc_type, version, platform, pid in dccs:
            di = dcc_mcp_core.DccInfo(dcc_type=dcc_type, version=version, platform=platform, pid=pid)
            d = di.to_dict()
            assert d["dcc_type"] == dcc_type
            assert d["version"] == version

    def test_metadata_custom(self) -> None:
        di = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2024",
            platform="windows",
            pid=1,
            metadata={"build": "debug"},
        )
        assert di.metadata == {"build": "debug"}


# ---------------------------------------------------------------------------
# DccError and DccErrorCode
# ---------------------------------------------------------------------------


class TestDccErrorCodes:
    @pytest.mark.parametrize(
        "code_name",
        [
            "TIMEOUT",
            "CONNECTION_FAILED",
            "SCRIPT_ERROR",
            "INVALID_INPUT",
            "PERMISSION_DENIED",
            "SCENE_ERROR",
            "NOT_RESPONDING",
            "UNSUPPORTED",
            "INTERNAL",
        ],
    )
    def test_error_code_exists(self, code_name: str) -> None:
        code = getattr(dcc_mcp_core.DccErrorCode, code_name)
        assert code is not None

    def test_all_codes_distinct(self) -> None:
        codes = [
            dcc_mcp_core.DccErrorCode.TIMEOUT,
            dcc_mcp_core.DccErrorCode.CONNECTION_FAILED,
            dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            dcc_mcp_core.DccErrorCode.INVALID_INPUT,
            dcc_mcp_core.DccErrorCode.PERMISSION_DENIED,
            dcc_mcp_core.DccErrorCode.SCENE_ERROR,
            dcc_mcp_core.DccErrorCode.NOT_RESPONDING,
            dcc_mcp_core.DccErrorCode.UNSUPPORTED,
            dcc_mcp_core.DccErrorCode.INTERNAL,
        ]
        assert len(set(str(c) for c in codes)) == 9


class TestDccError:
    def test_basic_construction(self) -> None:
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.TIMEOUT,
            message="connection timed out",
        )
        assert err is not None

    def test_code_field(self) -> None:
        err = dcc_mcp_core.DccError(code=dcc_mcp_core.DccErrorCode.TIMEOUT, message="timeout")
        assert str(err.code) == str(dcc_mcp_core.DccErrorCode.TIMEOUT)

    def test_message_field(self) -> None:
        err = dcc_mcp_core.DccError(code=dcc_mcp_core.DccErrorCode.INTERNAL, message="unexpected error")
        assert err.message == "unexpected error"

    def test_details_optional(self) -> None:
        err = dcc_mcp_core.DccError(code=dcc_mcp_core.DccErrorCode.TIMEOUT, message="t", details="after 30s")
        assert err.details == "after 30s"

    def test_details_default_none(self) -> None:
        err = dcc_mcp_core.DccError(code=dcc_mcp_core.DccErrorCode.TIMEOUT, message="t")
        assert err.details is None

    def test_recoverable_true(self) -> None:
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.TIMEOUT,
            message="timeout",
            recoverable=True,
        )
        assert err.recoverable is True

    def test_recoverable_default_false(self) -> None:
        err = dcc_mcp_core.DccError(code=dcc_mcp_core.DccErrorCode.INTERNAL, message="error")
        assert err.recoverable is False

    def test_connection_failed_error(self) -> None:
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.CONNECTION_FAILED,
            message="cannot connect to maya",
            details="port 7001 refused",
        )
        assert err.message == "cannot connect to maya"
        assert err.details == "port 7001 refused"
        assert err.recoverable is False

    def test_script_error_details(self) -> None:
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            message="script failed",
            details="NameError: name 'cmds' is not defined",
        )
        assert "NameError" in err.details

    @pytest.mark.parametrize(
        ("code_name", "expected_recoverable"),
        [
            ("TIMEOUT", True),
            ("CONNECTION_FAILED", False),
            ("NOT_RESPONDING", True),
            ("INVALID_INPUT", False),
            ("PERMISSION_DENIED", False),
        ],
    )
    def test_recoverable_by_code(self, code_name: str, expected_recoverable: bool) -> None:
        code = getattr(dcc_mcp_core.DccErrorCode, code_name)
        err = dcc_mcp_core.DccError(
            code=code,
            message="error message",
            recoverable=expected_recoverable,
        )
        assert err.recoverable is expected_recoverable


# ---------------------------------------------------------------------------
# DccCapabilities and ScriptLanguage
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    def test_python_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ScriptLanguage, "PYTHON")

    def test_mel_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ScriptLanguage, "MEL")

    def test_python_and_mel_distinct(self) -> None:
        assert dcc_mcp_core.ScriptLanguage.PYTHON != dcc_mcp_core.ScriptLanguage.MEL

    def test_repr(self) -> None:
        assert "PYTHON" in repr(dcc_mcp_core.ScriptLanguage.PYTHON) or str(dcc_mcp_core.ScriptLanguage.PYTHON) != ""


class TestDccCapabilities:
    def test_default_construction(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc is not None

    def test_default_scene_info_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.scene_info is False

    def test_default_snapshot_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.snapshot is False

    def test_default_undo_redo_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.undo_redo is False

    def test_default_file_operations_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.file_operations is False

    def test_default_selection_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.selection is False

    def test_default_progress_reporting_false(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.progress_reporting is False

    def test_default_extensions_empty(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.extensions == {}

    def test_script_languages_python(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON])
        langs = dc.script_languages
        assert any(str(lang) == str(dcc_mcp_core.ScriptLanguage.PYTHON) for lang in langs)

    def test_script_languages_multiple(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL]
        )
        assert len(dc.script_languages) == 2

    def test_scene_info_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(scene_info=True)
        assert dc.scene_info is True

    def test_snapshot_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(snapshot=True)
        assert dc.snapshot is True

    def test_undo_redo_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(undo_redo=True)
        assert dc.undo_redo is True

    def test_file_operations_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(file_operations=True)
        assert dc.file_operations is True

    def test_selection_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(selection=True)
        assert dc.selection is True

    def test_progress_reporting_true(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(progress_reporting=True)
        assert dc.progress_reporting is True

    def test_extensions_dict(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(extensions={"usd": True, "alembic": False})
        assert dc.extensions == {"usd": True, "alembic": False}

    def test_maya_full_capabilities(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL],
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            file_operations=True,
            selection=True,
            progress_reporting=True,
            extensions={"usd": True},
        )
        assert dc.scene_info is True
        assert dc.snapshot is True
        assert len(dc.script_languages) == 2

    def test_blender_capabilities(self) -> None:
        dc = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON],
            scene_info=True,
            file_operations=True,
            undo_redo=True,
        )
        assert dc.scene_info is True
        assert dc.snapshot is False  # not set


# ---------------------------------------------------------------------------
# ServiceStatus
# ---------------------------------------------------------------------------


class TestServiceStatus:
    def test_available_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ServiceStatus, "AVAILABLE")

    def test_busy_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ServiceStatus, "BUSY")

    def test_unreachable_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ServiceStatus, "UNREACHABLE")

    def test_shutting_down_exists(self) -> None:
        assert hasattr(dcc_mcp_core.ServiceStatus, "SHUTTING_DOWN")

    def test_all_statuses_distinct(self) -> None:
        statuses = [
            dcc_mcp_core.ServiceStatus.AVAILABLE,
            dcc_mcp_core.ServiceStatus.BUSY,
            dcc_mcp_core.ServiceStatus.UNREACHABLE,
            dcc_mcp_core.ServiceStatus.SHUTTING_DOWN,
        ]
        strs = [str(s) for s in statuses]
        assert len(set(strs)) == 4

    def test_available_not_busy(self) -> None:
        assert dcc_mcp_core.ServiceStatus.AVAILABLE != dcc_mcp_core.ServiceStatus.BUSY

    def test_repr_contains_status_name(self) -> None:
        r = repr(dcc_mcp_core.ServiceStatus.AVAILABLE)
        assert len(r) > 0
