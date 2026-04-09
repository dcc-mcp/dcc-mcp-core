"""Deep tests for DCC adapter protocol types.

Covers:
- SceneStatistics — all 7 fields, default, repr
- SceneInfo — all fields (full + default), repr
- DccInfo — all fields, to_dict, no python_version, repr
- DccCapabilities — all booleans, extensions, default, repr
- DccError / DccErrorCode — all 9 error codes, fields, str/repr
- ScriptLanguage — all 8 enum values, eq, repr
- ScriptResult — success/failure, to_dict, context, repr
- CaptureResult — data_size, viewport optional, repr
- ToolAnnotations — all optional bool fields, None defaults, eq, repr
- ToolDefinition — name/desc/schema, optional output_schema/annotations, eq, repr
- ResourceAnnotations — audience/priority, empty, repr
- ResourceDefinition — uri/name/desc/mime_type, annotations optional, repr
- ResourceTemplateDefinition — uri_template/name/desc/mime_type, repr
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import CaptureResult
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import DccInfo
from dcc_mcp_core import ResourceAnnotations
from dcc_mcp_core import ResourceDefinition
from dcc_mcp_core import ResourceTemplateDefinition
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import ToolAnnotations
from dcc_mcp_core import ToolDefinition

# ---------------------------------------------------------------------------
# SceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatistics:
    """Tests for SceneStatistics data class."""

    def test_default_construction(self) -> None:
        ss = SceneStatistics()
        assert ss.object_count == 0
        assert ss.vertex_count == 0
        assert ss.polygon_count == 0
        assert ss.material_count == 0
        assert ss.texture_count == 0
        assert ss.light_count == 0
        assert ss.camera_count == 0

    def test_all_fields_set(self) -> None:
        ss = SceneStatistics(
            object_count=10,
            vertex_count=200,
            polygon_count=50,
            material_count=3,
            texture_count=5,
            light_count=2,
            camera_count=1,
        )
        assert ss.object_count == 10
        assert ss.vertex_count == 200
        assert ss.polygon_count == 50
        assert ss.material_count == 3
        assert ss.texture_count == 5
        assert ss.light_count == 2
        assert ss.camera_count == 1

    def test_repr_contains_object_count(self) -> None:
        ss = SceneStatistics(object_count=7)
        r = repr(ss)
        assert "7" in r

    def test_repr_contains_vertex_count(self) -> None:
        ss = SceneStatistics(vertex_count=999)
        r = repr(ss)
        assert "999" in r

    def test_partial_construction(self) -> None:
        ss = SceneStatistics(object_count=5)
        assert ss.object_count == 5
        assert ss.vertex_count == 0

    def test_large_values(self) -> None:
        ss = SceneStatistics(vertex_count=10_000_000)
        assert ss.vertex_count == 10_000_000

    def test_repr_is_string(self) -> None:
        ss = SceneStatistics()
        assert isinstance(repr(ss), str)


# ---------------------------------------------------------------------------
# SceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfoDefault:
    """Default SceneInfo construction."""

    def test_default_name(self) -> None:
        si = SceneInfo()
        assert si.name == "untitled"

    def test_default_file_path_empty(self) -> None:
        si = SceneInfo()
        assert si.file_path == ""

    def test_default_modified_false(self) -> None:
        si = SceneInfo()
        assert si.modified is False

    def test_default_fps_none(self) -> None:
        si = SceneInfo()
        assert si.fps is None

    def test_default_up_axis_none(self) -> None:
        si = SceneInfo()
        assert si.up_axis is None

    def test_default_frame_range_none(self) -> None:
        si = SceneInfo()
        assert si.frame_range is None

    def test_default_current_frame_none(self) -> None:
        si = SceneInfo()
        assert si.current_frame is None

    def test_default_units_none(self) -> None:
        si = SceneInfo()
        assert si.units is None

    def test_default_format_empty(self) -> None:
        si = SceneInfo()
        assert si.format == ""

    def test_default_metadata_empty(self) -> None:
        si = SceneInfo()
        assert si.metadata == {}

    def test_default_statistics_zero_objects(self) -> None:
        si = SceneInfo()
        assert si.statistics.object_count == 0


class TestSceneInfoFull:
    """Full SceneInfo construction with all fields."""

    def test_file_path(self) -> None:
        si = SceneInfo(file_path="/scenes/test.ma")
        assert si.file_path == "/scenes/test.ma"

    def test_name(self) -> None:
        si = SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_modified_true(self) -> None:
        si = SceneInfo(modified=True)
        assert si.modified is True

    def test_format(self) -> None:
        si = SceneInfo(format="maya_ascii")
        assert si.format == "maya_ascii"

    def test_frame_range(self) -> None:
        si = SceneInfo(frame_range=(1.0, 240.0))
        assert si.frame_range == (1.0, 240.0)

    def test_current_frame(self) -> None:
        si = SceneInfo(current_frame=42.5)
        assert si.current_frame == 42.5

    def test_fps(self) -> None:
        si = SceneInfo(fps=24.0)
        assert si.fps == pytest.approx(24.0)

    def test_up_axis_y(self) -> None:
        si = SceneInfo(up_axis="Y")
        assert si.up_axis == "Y"

    def test_up_axis_z(self) -> None:
        si = SceneInfo(up_axis="Z")
        assert si.up_axis == "Z"

    def test_units(self) -> None:
        si = SceneInfo(units="cm")
        assert si.units == "cm"

    def test_metadata(self) -> None:
        si = SceneInfo(metadata={"artist": "bob"})
        assert si.metadata == {"artist": "bob"}

    def test_statistics_passed(self) -> None:
        ss = SceneStatistics(object_count=15, vertex_count=300)
        si = SceneInfo(statistics=ss)
        assert si.statistics.object_count == 15
        assert si.statistics.vertex_count == 300

    def test_repr_contains_name(self) -> None:
        si = SceneInfo(name="probe_scene")
        assert "probe_scene" in repr(si)

    def test_repr_is_string(self) -> None:
        si = SceneInfo()
        assert isinstance(repr(si), str)


# ---------------------------------------------------------------------------
# DccInfo
# ---------------------------------------------------------------------------


class TestDccInfo:
    """Tests for DccInfo data class."""

    def test_basic_fields(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=12345)
        assert di.dcc_type == "maya"
        assert di.version == "2025"
        assert di.platform == "win64"
        assert di.pid == 12345

    def test_python_version_set(self) -> None:
        di = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=9999, python_version="3.10")
        assert di.python_version == "3.10"

    def test_python_version_none_by_default(self) -> None:
        di = DccInfo(dcc_type="unreal", version="5.3", platform="win64", pid=111)
        assert di.python_version is None

    def test_metadata_set(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=42, metadata={"plugin": "v2"})
        assert di.metadata["plugin"] == "v2"

    def test_metadata_default_empty(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=42)
        assert di.metadata == {}

    def test_to_dict_keys(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=42)
        d = di.to_dict()
        assert "dcc_type" in d
        assert "version" in d
        assert "platform" in d
        assert "pid" in d

    def test_to_dict_dcc_type_value(self) -> None:
        di = DccInfo(dcc_type="houdini", version="20.0", platform="macos", pid=777)
        d = di.to_dict()
        assert d["dcc_type"] == "houdini"

    def test_to_dict_pid_value(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=12345)
        d = di.to_dict()
        assert d["pid"] == 12345

    def test_repr_contains_dcc_type(self) -> None:
        di = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=100)
        assert "blender" in repr(di)

    def test_repr_contains_pid(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=55555)
        assert "55555" in repr(di)

    def test_repr_is_string(self) -> None:
        di = DccInfo(dcc_type="maya", version="2025", platform="win64", pid=1)
        assert isinstance(repr(di), str)


# ---------------------------------------------------------------------------
# DccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilities:
    """Tests for DccCapabilities data class."""

    def test_default_all_false(self) -> None:
        cap = DccCapabilities()
        assert cap.scene_info is False
        assert cap.snapshot is False
        assert cap.undo_redo is False
        assert cap.progress_reporting is False
        assert cap.file_operations is False
        assert cap.selection is False

    def test_default_empty_languages(self) -> None:
        cap = DccCapabilities()
        assert cap.script_languages == []

    def test_default_empty_extensions(self) -> None:
        cap = DccCapabilities()
        assert cap.extensions == {}

    def test_scene_info_true(self) -> None:
        cap = DccCapabilities(scene_info=True)
        assert cap.scene_info is True

    def test_snapshot_true(self) -> None:
        cap = DccCapabilities(snapshot=True)
        assert cap.snapshot is True

    def test_undo_redo_true(self) -> None:
        cap = DccCapabilities(undo_redo=True)
        assert cap.undo_redo is True

    def test_progress_reporting_true(self) -> None:
        cap = DccCapabilities(progress_reporting=True)
        assert cap.progress_reporting is True

    def test_file_operations_true(self) -> None:
        cap = DccCapabilities(file_operations=True)
        assert cap.file_operations is True

    def test_selection_true(self) -> None:
        cap = DccCapabilities(selection=True)
        assert cap.selection is True

    def test_script_languages_python_mel(self) -> None:
        cap = DccCapabilities(script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL])
        assert ScriptLanguage.PYTHON in cap.script_languages
        assert ScriptLanguage.MEL in cap.script_languages

    def test_script_languages_count(self) -> None:
        cap = DccCapabilities(script_languages=[ScriptLanguage.PYTHON])
        assert len(cap.script_languages) == 1

    def test_extensions_dict(self) -> None:
        cap = DccCapabilities(extensions={"gpu": True, "vr": False})
        assert cap.extensions["gpu"] is True
        assert cap.extensions["vr"] is False

    def test_repr_is_string(self) -> None:
        cap = DccCapabilities(scene_info=True)
        assert isinstance(repr(cap), str)

    def test_repr_contains_scene_info(self) -> None:
        cap = DccCapabilities(scene_info=True)
        assert "true" in repr(cap).lower() or "True" in repr(cap)


# ---------------------------------------------------------------------------
# DccErrorCode enum
# ---------------------------------------------------------------------------


class TestDccErrorCode:
    """Tests for all DccErrorCode enum variants."""

    def test_connection_failed(self) -> None:
        code = DccErrorCode.CONNECTION_FAILED
        assert code == DccErrorCode.CONNECTION_FAILED

    def test_timeout(self) -> None:
        assert DccErrorCode.TIMEOUT == DccErrorCode.TIMEOUT

    def test_script_error(self) -> None:
        assert DccErrorCode.SCRIPT_ERROR == DccErrorCode.SCRIPT_ERROR

    def test_not_responding(self) -> None:
        assert DccErrorCode.NOT_RESPONDING == DccErrorCode.NOT_RESPONDING

    def test_unsupported(self) -> None:
        assert DccErrorCode.UNSUPPORTED == DccErrorCode.UNSUPPORTED

    def test_permission_denied(self) -> None:
        assert DccErrorCode.PERMISSION_DENIED == DccErrorCode.PERMISSION_DENIED

    def test_invalid_input(self) -> None:
        assert DccErrorCode.INVALID_INPUT == DccErrorCode.INVALID_INPUT

    def test_scene_error(self) -> None:
        assert DccErrorCode.SCENE_ERROR == DccErrorCode.SCENE_ERROR

    def test_internal(self) -> None:
        assert DccErrorCode.INTERNAL == DccErrorCode.INTERNAL

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(DccErrorCode.TIMEOUT), str)

    def test_different_codes_not_equal(self) -> None:
        assert DccErrorCode.TIMEOUT != DccErrorCode.INTERNAL


# ---------------------------------------------------------------------------
# ScriptLanguage enum
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    """Tests for all ScriptLanguage enum variants."""

    def test_python(self) -> None:
        assert ScriptLanguage.PYTHON == ScriptLanguage.PYTHON

    def test_mel(self) -> None:
        assert ScriptLanguage.MEL == ScriptLanguage.MEL

    def test_maxscript(self) -> None:
        assert ScriptLanguage.MAXSCRIPT == ScriptLanguage.MAXSCRIPT

    def test_hscript(self) -> None:
        assert ScriptLanguage.HSCRIPT == ScriptLanguage.HSCRIPT

    def test_vex(self) -> None:
        assert ScriptLanguage.VEX == ScriptLanguage.VEX

    def test_lua(self) -> None:
        assert ScriptLanguage.LUA == ScriptLanguage.LUA

    def test_csharp(self) -> None:
        assert ScriptLanguage.CSHARP == ScriptLanguage.CSHARP

    def test_blueprint(self) -> None:
        assert ScriptLanguage.BLUEPRINT == ScriptLanguage.BLUEPRINT

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(ScriptLanguage.PYTHON), str)

    def test_different_languages_not_equal(self) -> None:
        assert ScriptLanguage.PYTHON != ScriptLanguage.MEL

    def test_str_is_string(self) -> None:
        assert isinstance(str(ScriptLanguage.MEL), str)


# ---------------------------------------------------------------------------
# DccError
# ---------------------------------------------------------------------------


class TestDccError:
    """Tests for DccError data class."""

    def test_code_stored(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timed out")
        assert e.code == DccErrorCode.TIMEOUT

    def test_message_stored(self) -> None:
        e = DccError(code=DccErrorCode.SCRIPT_ERROR, message="syntax error")
        assert e.message == "syntax error"

    def test_details_none_by_default(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="err")
        assert e.details is None

    def test_details_set(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timed out", details="30s elapsed")
        assert e.details == "30s elapsed"

    def test_recoverable_false_by_default(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="err")
        assert e.recoverable is False

    def test_recoverable_true(self) -> None:
        e = DccError(code=DccErrorCode.CONNECTION_FAILED, message="conn failed", recoverable=True)
        assert e.recoverable is True

    def test_str_contains_code(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timed out")
        s = str(e)
        assert "TIMEOUT" in s

    def test_str_contains_message(self) -> None:
        e = DccError(code=DccErrorCode.SCRIPT_ERROR, message="syntax error")
        assert "syntax error" in str(e)

    def test_repr_is_string(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="err")
        assert isinstance(repr(e), str)

    def test_repr_contains_code(self) -> None:
        e = DccError(code=DccErrorCode.PERMISSION_DENIED, message="denied")
        assert "PERMISSION_DENIED" in repr(e)

    def test_all_error_codes_constructable(self) -> None:
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


# ---------------------------------------------------------------------------
# ScriptResult
# ---------------------------------------------------------------------------


class TestScriptResult:
    """Tests for ScriptResult data class."""

    def test_success_true(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=10)
        assert sr.success is True

    def test_success_false(self) -> None:
        sr = ScriptResult(success=False, execution_time_ms=100, error="SyntaxError")
        assert sr.success is False

    def test_execution_time_ms(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=42)
        assert sr.execution_time_ms == 42

    def test_output_set(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5, output="result")
        assert sr.output == "result"

    def test_output_none_by_default(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5)
        assert sr.output is None

    def test_error_none_on_success(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5)
        assert sr.error is None

    def test_error_set_on_failure(self) -> None:
        sr = ScriptResult(success=False, execution_time_ms=100, error="AttributeError")
        assert sr.error == "AttributeError"

    def test_context_empty_by_default(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5)
        assert sr.context == {}

    def test_context_set(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5, context={"frame": "1"})
        assert sr.context["frame"] == "1"

    def test_to_dict_has_success(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=5)
        assert "success" in sr.to_dict()

    def test_to_dict_has_execution_time_ms(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=99)
        assert "execution_time_ms" in sr.to_dict()

    def test_to_dict_success_value(self) -> None:
        sr = ScriptResult(success=False, execution_time_ms=50)
        assert sr.to_dict()["success"] is False

    def test_repr_is_string(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=1)
        assert isinstance(repr(sr), str)

    def test_repr_contains_success(self) -> None:
        sr = ScriptResult(success=True, execution_time_ms=1)
        assert "true" in repr(sr).lower() or "True" in repr(sr)


# ---------------------------------------------------------------------------
# CaptureResult
# ---------------------------------------------------------------------------


class TestCaptureResult:
    """Tests for CaptureResult data class."""

    def test_data_stored(self) -> None:
        cr = CaptureResult(data=b"\x89PNG", width=800, height=600, format="png")
        assert cr.data == b"\x89PNG"

    def test_width(self) -> None:
        cr = CaptureResult(data=b"d", width=1920, height=1080, format="png")
        assert cr.width == 1920

    def test_height(self) -> None:
        cr = CaptureResult(data=b"d", width=100, height=200, format="jpeg")
        assert cr.height == 200

    def test_format(self) -> None:
        cr = CaptureResult(data=b"d", width=100, height=100, format="raw_bgra")
        assert cr.format == "raw_bgra"

    def test_viewport_set(self) -> None:
        cr = CaptureResult(data=b"d", width=100, height=100, format="png", viewport="main")
        assert cr.viewport == "main"

    def test_viewport_none_by_default(self) -> None:
        cr = CaptureResult(data=b"d", width=100, height=100, format="png")
        assert cr.viewport is None

    def test_data_size_correct(self) -> None:
        payload = b"x" * 1024
        cr = CaptureResult(data=payload, width=100, height=100, format="png")
        assert cr.data_size() == 1024

    def test_data_size_empty(self) -> None:
        cr = CaptureResult(data=b"", width=0, height=0, format="png")
        assert cr.data_size() == 0

    def test_repr_contains_dimensions(self) -> None:
        cr = CaptureResult(data=b"d", width=1280, height=720, format="png")
        r = repr(cr)
        assert "1280" in r
        assert "720" in r

    def test_repr_is_string(self) -> None:
        cr = CaptureResult(data=b"d", width=10, height=10, format="png")
        assert isinstance(repr(cr), str)


# ---------------------------------------------------------------------------
# ToolAnnotations
# ---------------------------------------------------------------------------


class TestToolAnnotations:
    """Tests for ToolAnnotations MCP hints."""

    def test_all_none_by_default(self) -> None:
        ta = ToolAnnotations()
        assert ta.title is None
        assert ta.read_only_hint is None
        assert ta.destructive_hint is None
        assert ta.idempotent_hint is None
        assert ta.open_world_hint is None

    def test_title_set(self) -> None:
        ta = ToolAnnotations(title="my tool")
        assert ta.title == "my tool"

    def test_read_only_hint_true(self) -> None:
        ta = ToolAnnotations(read_only_hint=True)
        assert ta.read_only_hint is True

    def test_read_only_hint_false(self) -> None:
        ta = ToolAnnotations(read_only_hint=False)
        assert ta.read_only_hint is False

    def test_destructive_hint_true(self) -> None:
        ta = ToolAnnotations(destructive_hint=True)
        assert ta.destructive_hint is True

    def test_idempotent_hint_true(self) -> None:
        ta = ToolAnnotations(idempotent_hint=True)
        assert ta.idempotent_hint is True

    def test_open_world_hint_true(self) -> None:
        ta = ToolAnnotations(open_world_hint=True)
        assert ta.open_world_hint is True

    def test_eq_same_values(self) -> None:
        ta1 = ToolAnnotations(title="t", read_only_hint=True)
        ta2 = ToolAnnotations(title="t", read_only_hint=True)
        assert ta1 == ta2

    def test_ne_different_title(self) -> None:
        ta1 = ToolAnnotations(title="a")
        ta2 = ToolAnnotations(title="b")
        assert ta1 != ta2

    def test_ne_different_hint(self) -> None:
        ta1 = ToolAnnotations(read_only_hint=True)
        ta2 = ToolAnnotations(read_only_hint=False)
        assert ta1 != ta2

    def test_repr_is_string(self) -> None:
        ta = ToolAnnotations(title="t")
        assert isinstance(repr(ta), str)

    def test_repr_contains_title(self) -> None:
        ta = ToolAnnotations(title="special_tool")
        assert "special_tool" in repr(ta)


# ---------------------------------------------------------------------------
# ToolDefinition
# ---------------------------------------------------------------------------


class TestToolDefinition:
    """Tests for ToolDefinition MCP tool schema."""

    def test_name(self) -> None:
        td = ToolDefinition(name="create_sphere", description="desc", input_schema="{}")
        assert td.name == "create_sphere"

    def test_description(self) -> None:
        td = ToolDefinition(name="create_sphere", description="creates a sphere", input_schema="{}")
        assert td.description == "creates a sphere"

    def test_input_schema(self) -> None:
        schema = '{"type": "object"}'
        td = ToolDefinition(name="t", description="d", input_schema=schema)
        assert td.input_schema == schema

    def test_output_schema_none_by_default(self) -> None:
        td = ToolDefinition(name="t", description="d", input_schema="{}")
        assert td.output_schema is None

    def test_output_schema_set(self) -> None:
        out = '{"type": "string"}'
        td = ToolDefinition(name="t", description="d", input_schema="{}", output_schema=out)
        assert td.output_schema == out

    def test_annotations_none_by_default(self) -> None:
        td = ToolDefinition(name="t", description="d", input_schema="{}")
        assert td.annotations is None

    def test_annotations_set(self) -> None:
        ta = ToolAnnotations(title="my tool", read_only_hint=True)
        td = ToolDefinition(name="t", description="d", input_schema="{}", annotations=ta)
        assert td.annotations is not None
        assert td.annotations.title == "my tool"

    def test_eq_same(self) -> None:
        td1 = ToolDefinition(name="t", description="d", input_schema="{}")
        td2 = ToolDefinition(name="t", description="d", input_schema="{}")
        assert td1 == td2

    def test_ne_different_name(self) -> None:
        td1 = ToolDefinition(name="a", description="d", input_schema="{}")
        td2 = ToolDefinition(name="b", description="d", input_schema="{}")
        assert td1 != td2

    def test_ne_different_schema(self) -> None:
        td1 = ToolDefinition(name="t", description="d", input_schema="{}")
        td2 = ToolDefinition(name="t", description="d", input_schema='{"type":"object"}')
        assert td1 != td2

    def test_repr_is_string(self) -> None:
        td = ToolDefinition(name="t", description="d", input_schema="{}")
        assert isinstance(repr(td), str)

    def test_repr_contains_name(self) -> None:
        td = ToolDefinition(name="render_scene", description="d", input_schema="{}")
        assert "render_scene" in repr(td)


# ---------------------------------------------------------------------------
# ResourceAnnotations
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    """Tests for ResourceAnnotations MCP hints."""

    def test_audience_empty_by_default(self) -> None:
        ra = ResourceAnnotations()
        assert ra.audience == []

    def test_priority_none_by_default(self) -> None:
        ra = ResourceAnnotations()
        assert ra.priority is None

    def test_audience_set(self) -> None:
        ra = ResourceAnnotations(audience=["user", "agent"])
        assert "user" in ra.audience
        assert "agent" in ra.audience

    def test_audience_single(self) -> None:
        ra = ResourceAnnotations(audience=["assistant"])
        assert len(ra.audience) == 1

    def test_priority_set(self) -> None:
        ra = ResourceAnnotations(priority=0.75)
        assert ra.priority == pytest.approx(0.75)

    def test_priority_zero(self) -> None:
        ra = ResourceAnnotations(priority=0.0)
        assert ra.priority == pytest.approx(0.0)

    def test_priority_one(self) -> None:
        ra = ResourceAnnotations(priority=1.0)
        assert ra.priority == pytest.approx(1.0)

    def test_repr_is_string(self) -> None:
        ra = ResourceAnnotations(audience=["user"], priority=0.5)
        assert isinstance(repr(ra), str)

    def test_repr_contains_audience(self) -> None:
        ra = ResourceAnnotations(audience=["designer"])
        assert "designer" in repr(ra)


# ---------------------------------------------------------------------------
# ResourceDefinition
# ---------------------------------------------------------------------------


class TestResourceDefinition:
    """Tests for ResourceDefinition MCP resource schema."""

    def test_uri(self) -> None:
        rd = ResourceDefinition(uri="scene://current", name="scene", description="d")
        assert rd.uri == "scene://current"

    def test_name(self) -> None:
        rd = ResourceDefinition(uri="u", name="my_resource", description="d")
        assert rd.name == "my_resource"

    def test_description(self) -> None:
        rd = ResourceDefinition(uri="u", name="n", description="my desc")
        assert rd.description == "my desc"

    def test_mime_type_default_text_plain(self) -> None:
        rd = ResourceDefinition(uri="u", name="n", description="d")
        assert rd.mime_type == "text/plain"

    def test_mime_type_set(self) -> None:
        rd = ResourceDefinition(uri="u", name="n", description="d", mime_type="application/json")
        assert rd.mime_type == "application/json"

    def test_annotations_none_by_default(self) -> None:
        rd = ResourceDefinition(uri="u", name="n", description="d")
        assert rd.annotations is None

    def test_annotations_set(self) -> None:
        ra = ResourceAnnotations(audience=["user"])
        rd = ResourceDefinition(uri="u", name="n", description="d", annotations=ra)
        assert rd.annotations is not None

    def test_repr_is_string(self) -> None:
        rd = ResourceDefinition(uri="u", name="n", description="d")
        assert isinstance(repr(rd), str)

    def test_repr_contains_name(self) -> None:
        rd = ResourceDefinition(uri="u", name="my_resource", description="d")
        assert "my_resource" in repr(rd)

    def test_repr_contains_uri(self) -> None:
        rd = ResourceDefinition(uri="scene://active", name="n", description="d")
        assert "scene://active" in repr(rd)


# ---------------------------------------------------------------------------
# ResourceTemplateDefinition
# ---------------------------------------------------------------------------


class TestResourceTemplateDefinition:
    """Tests for ResourceTemplateDefinition MCP resource template schema."""

    def test_uri_template(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="scene://{name}", name="tmpl", description="d")
        assert rt.uri_template == "scene://{name}"

    def test_name(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="my_tmpl", description="d")
        assert rt.name == "my_tmpl"

    def test_description(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="my template desc")
        assert rt.description == "my template desc"

    def test_mime_type_default_text_plain(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="d")
        assert rt.mime_type == "text/plain"

    def test_mime_type_set(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="d", mime_type="image/png")
        assert rt.mime_type == "image/png"

    def test_annotations_none_by_default(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="d")
        assert rt.annotations is None

    def test_annotations_set(self) -> None:
        ra = ResourceAnnotations(priority=0.8)
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="d", annotations=ra)
        assert rt.annotations is not None

    def test_repr_is_string(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="n", description="d")
        assert isinstance(repr(rt), str)

    def test_repr_contains_name(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="u", name="scene_tmpl", description="d")
        assert "scene_tmpl" in repr(rt)

    def test_repr_contains_uri_template(self) -> None:
        rt = ResourceTemplateDefinition(uri_template="asset://{id}", name="n", description="d")
        assert "asset://{id}" in repr(rt)
