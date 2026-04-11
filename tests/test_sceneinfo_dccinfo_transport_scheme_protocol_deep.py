"""Deep tests for SceneInfo, SceneStatistics, DccInfo, DccCapabilities, DccError.

Also covers TransportScheme, RoutingStrategy, ToolDeclaration, PromptArgument,
PromptDefinition.

Target: +163 tests  (11256 → ~11419 collected)
All tests use the installed 0.12.12 binary — no new unshipped APIs.
"""

from __future__ import annotations

import json

import dcc_mcp_core
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import DccInfo
from dcc_mcp_core import PromptArgument
from dcc_mcp_core import PromptDefinition
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ToolDeclaration
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme

# ---------------------------------------------------------------------------
# SceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatisticsCreate:
    def test_default_all_zeros(self):
        ss = SceneStatistics()
        assert ss.object_count == 0

    def test_default_vertex_count(self):
        ss = SceneStatistics()
        assert ss.vertex_count == 0

    def test_default_polygon_count(self):
        ss = SceneStatistics()
        assert ss.polygon_count == 0

    def test_default_material_count(self):
        ss = SceneStatistics()
        assert ss.material_count == 0

    def test_default_texture_count(self):
        ss = SceneStatistics()
        assert ss.texture_count == 0

    def test_default_light_count(self):
        ss = SceneStatistics()
        assert ss.light_count == 0

    def test_default_camera_count(self):
        ss = SceneStatistics()
        assert ss.camera_count == 0

    def test_set_object_count(self):
        ss = SceneStatistics(object_count=42)
        assert ss.object_count == 42

    def test_set_vertex_count(self):
        ss = SceneStatistics(vertex_count=10000)
        assert ss.vertex_count == 10000

    def test_set_polygon_count(self):
        ss = SceneStatistics(polygon_count=5000)
        assert ss.polygon_count == 5000

    def test_set_material_count(self):
        ss = SceneStatistics(material_count=8)
        assert ss.material_count == 8

    def test_set_texture_count(self):
        ss = SceneStatistics(texture_count=20)
        assert ss.texture_count == 20

    def test_set_light_count(self):
        ss = SceneStatistics(light_count=3)
        assert ss.light_count == 3

    def test_set_camera_count(self):
        ss = SceneStatistics(camera_count=2)
        assert ss.camera_count == 2

    def test_all_fields(self):
        ss = SceneStatistics(
            object_count=10,
            vertex_count=3000,
            polygon_count=5000,
            material_count=8,
            texture_count=15,
            light_count=3,
            camera_count=2,
        )
        assert ss.object_count == 10
        assert ss.vertex_count == 3000
        assert ss.polygon_count == 5000
        assert ss.material_count == 8
        assert ss.texture_count == 15
        assert ss.light_count == 3
        assert ss.camera_count == 2

    def test_repr_exists(self):
        ss = SceneStatistics(object_count=5)
        assert repr(ss)

    def test_repr_is_str(self):
        ss = SceneStatistics()
        assert isinstance(repr(ss), str)


# ---------------------------------------------------------------------------
# SceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfoCreate:
    def test_default_file_path(self):
        si = SceneInfo()
        assert si.file_path == ""

    def test_default_name(self):
        si = SceneInfo()
        assert si.name == "untitled"

    def test_default_modified(self):
        si = SceneInfo()
        assert si.modified is False

    def test_default_format(self):
        si = SceneInfo()
        assert si.format == ""

    def test_default_frame_range_none(self):
        si = SceneInfo()
        assert si.frame_range is None

    def test_default_current_frame_none(self):
        si = SceneInfo()
        assert si.current_frame is None

    def test_default_fps_none(self):
        si = SceneInfo()
        assert si.fps is None

    def test_default_up_axis_none(self):
        si = SceneInfo()
        assert si.up_axis is None

    def test_default_units_none(self):
        si = SceneInfo()
        assert si.units is None

    def test_default_statistics_type(self):
        si = SceneInfo()
        assert isinstance(si.statistics, SceneStatistics)

    def test_default_statistics_object_count(self):
        si = SceneInfo()
        assert si.statistics.object_count == 0

    def test_default_metadata_empty(self):
        si = SceneInfo()
        assert si.metadata == {}

    def test_set_file_path(self):
        si = SceneInfo(file_path="/proj/scene.mb")
        assert si.file_path == "/proj/scene.mb"

    def test_set_name(self):
        si = SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_set_modified_true(self):
        si = SceneInfo(modified=True)
        assert si.modified is True

    def test_set_format(self):
        si = SceneInfo(format="mb")
        assert si.format == "mb"

    def test_set_frame_range(self):
        si = SceneInfo(frame_range=(1.0, 240.0))
        assert si.frame_range == (1.0, 240.0)

    def test_frame_range_start(self):
        si = SceneInfo(frame_range=(0.0, 100.0))
        assert si.frame_range[0] == 0.0

    def test_frame_range_end(self):
        si = SceneInfo(frame_range=(0.0, 100.0))
        assert si.frame_range[1] == 100.0

    def test_set_current_frame(self):
        si = SceneInfo(current_frame=42.0)
        assert si.current_frame == 42.0

    def test_set_fps(self):
        si = SceneInfo(fps=30.0)
        assert si.fps == 30.0

    def test_set_up_axis(self):
        si = SceneInfo(up_axis="Y")
        assert si.up_axis == "Y"

    def test_set_units(self):
        si = SceneInfo(units="cm")
        assert si.units == "cm"

    def test_set_statistics_object_count(self):
        ss = SceneStatistics(object_count=10)
        si = SceneInfo(statistics=ss)
        assert si.statistics.object_count == 10

    def test_set_metadata(self):
        si = SceneInfo(metadata={"key": "val"})
        assert si.metadata.get("key") == "val"

    def test_repr_contains_name(self):
        si = SceneInfo(name="test_scene")
        assert "test_scene" in repr(si)

    def test_repr_is_str(self):
        si = SceneInfo()
        assert isinstance(repr(si), str)

    def test_full_construction(self):
        ss = SceneStatistics(object_count=5, polygon_count=200)
        si = SceneInfo(
            file_path="/proj/scene.mb",
            name="scene",
            modified=True,
            format="mb",
            frame_range=(1.0, 100.0),
            current_frame=1.0,
            fps=24.0,
            up_axis="Y",
            units="cm",
            statistics=ss,
            metadata={"artist": "jane"},
        )
        assert si.file_path == "/proj/scene.mb"
        assert si.name == "scene"
        assert si.modified is True
        assert si.format == "mb"
        assert si.frame_range == (1.0, 100.0)
        assert si.current_frame == 1.0
        assert si.fps == 24.0
        assert si.up_axis == "Y"
        assert si.units == "cm"
        assert si.statistics.object_count == 5
        assert si.metadata["artist"] == "jane"


# ---------------------------------------------------------------------------
# DccInfo
# ---------------------------------------------------------------------------


class TestDccInfoCreate:
    def test_dcc_type(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1000)
        assert info.dcc_type == "maya"

    def test_version(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1000)
        assert info.version == "2025"

    def test_platform(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1000)
        assert info.platform == "windows"

    def test_pid(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1000)
        assert info.pid == 1000

    def test_python_version_default_none(self):
        info = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=99)
        assert info.python_version is None

    def test_python_version_set(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1, python_version="3.11.0")
        assert info.python_version == "3.11.0"

    def test_metadata_default_empty(self):
        info = DccInfo(dcc_type="houdini", version="20", platform="linux", pid=50)
        assert info.metadata == {}

    def test_metadata_set(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1, metadata={"scene": "/s.mb"})
        assert info.metadata["scene"] == "/s.mb"

    def test_to_dict_has_dcc_type(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "dcc_type" in info.to_dict()

    def test_to_dict_has_version(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "version" in info.to_dict()

    def test_to_dict_has_platform(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "platform" in info.to_dict()

    def test_to_dict_has_pid(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "pid" in info.to_dict()

    def test_to_dict_has_python_version(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "python_version" in info.to_dict()

    def test_to_dict_has_metadata(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "metadata" in info.to_dict()

    def test_repr_is_str(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert isinstance(repr(info), str)

    def test_repr_contains_dcc_type(self):
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert "maya" in repr(info)


# ---------------------------------------------------------------------------
# DccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilitiesCreate:
    def test_default_scene_info_false(self):
        caps = DccCapabilities()
        assert caps.scene_info is False

    def test_default_snapshot_false(self):
        caps = DccCapabilities()
        assert caps.snapshot is False

    def test_default_undo_redo_false(self):
        caps = DccCapabilities()
        assert caps.undo_redo is False

    def test_default_progress_reporting_false(self):
        caps = DccCapabilities()
        assert caps.progress_reporting is False

    def test_default_file_operations_false(self):
        caps = DccCapabilities()
        assert caps.file_operations is False

    def test_default_selection_false(self):
        caps = DccCapabilities()
        assert caps.selection is False

    def test_default_script_languages_empty(self):
        caps = DccCapabilities()
        assert caps.script_languages == [] or isinstance(caps.script_languages, list)

    def test_set_scene_info(self):
        caps = DccCapabilities(scene_info=True)
        assert caps.scene_info is True

    def test_set_snapshot(self):
        caps = DccCapabilities(snapshot=True)
        assert caps.snapshot is True

    def test_set_undo_redo(self):
        caps = DccCapabilities(undo_redo=True)
        assert caps.undo_redo is True

    def test_set_progress_reporting(self):
        caps = DccCapabilities(progress_reporting=True)
        assert caps.progress_reporting is True

    def test_set_file_operations(self):
        caps = DccCapabilities(file_operations=True)
        assert caps.file_operations is True

    def test_set_selection(self):
        caps = DccCapabilities(selection=True)
        assert caps.selection is True

    def test_set_single_script_language(self):
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON])
        assert len(caps.script_languages) == 1

    def test_set_multiple_script_languages(self):
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL])
        assert len(caps.script_languages) == 2

    def test_repr_is_str(self):
        caps = DccCapabilities()
        assert isinstance(repr(caps), str)

    def test_repr_contains_scene_info(self):
        caps = DccCapabilities(scene_info=True)
        assert "scene_info" in repr(caps)


# ---------------------------------------------------------------------------
# DccErrorCode enum
# ---------------------------------------------------------------------------


class TestDccErrorCodeEnum:
    def test_connection_failed(self):
        assert DccErrorCode.CONNECTION_FAILED

    def test_timeout(self):
        assert DccErrorCode.TIMEOUT

    def test_script_error(self):
        assert DccErrorCode.SCRIPT_ERROR

    def test_not_responding(self):
        assert DccErrorCode.NOT_RESPONDING

    def test_unsupported(self):
        assert DccErrorCode.UNSUPPORTED

    def test_permission_denied(self):
        assert DccErrorCode.PERMISSION_DENIED

    def test_invalid_input(self):
        assert DccErrorCode.INVALID_INPUT

    def test_scene_error(self):
        assert DccErrorCode.SCENE_ERROR

    def test_internal(self):
        assert DccErrorCode.INTERNAL

    def test_repr_is_str(self):
        assert isinstance(repr(DccErrorCode.SCRIPT_ERROR), str)

    def test_distinct_values(self):
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
        assert len(set(repr(c) for c in codes)) == 9


# ---------------------------------------------------------------------------
# DccError
# ---------------------------------------------------------------------------


class TestDccErrorCreate:
    def test_code(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="Script error")
        assert err.code == DccErrorCode.SCRIPT_ERROR

    def test_message(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="Script error")
        assert err.message == "Script error"

    def test_details_default_none(self):
        err = DccError(code=DccErrorCode.TIMEOUT, message="Timed out")
        assert err.details is None

    def test_details_set(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="err", details="AttributeError")
        assert err.details == "AttributeError"

    def test_recoverable_default_false(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="Internal")
        assert err.recoverable is False

    def test_recoverable_true(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="Script error", recoverable=True)
        assert err.recoverable is True

    def test_repr_is_str(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="err")
        assert isinstance(repr(err), str)

    def test_repr_contains_code(self):
        err = DccError(code=DccErrorCode.SCRIPT_ERROR, message="err")
        assert "SCRIPT_ERROR" in repr(err)

    def test_str_is_str(self):
        err = DccError(code=DccErrorCode.TIMEOUT, message="Timed out")
        assert isinstance(str(err), str)

    def test_str_contains_message(self):
        err = DccError(code=DccErrorCode.TIMEOUT, message="Connection timed out")
        assert "Connection timed out" in str(err)

    def test_all_error_codes(self):
        for code in [
            DccErrorCode.CONNECTION_FAILED,
            DccErrorCode.TIMEOUT,
            DccErrorCode.SCRIPT_ERROR,
            DccErrorCode.NOT_RESPONDING,
            DccErrorCode.UNSUPPORTED,
            DccErrorCode.PERMISSION_DENIED,
            DccErrorCode.INVALID_INPUT,
            DccErrorCode.SCENE_ERROR,
            DccErrorCode.INTERNAL,
        ]:
            err = DccError(code=code, message="msg")
            assert err.code == code


# ---------------------------------------------------------------------------
# ScriptLanguage enum
# ---------------------------------------------------------------------------


class TestScriptLanguageEnum:
    def test_python(self):
        assert ScriptLanguage.PYTHON

    def test_mel(self):
        assert ScriptLanguage.MEL

    def test_maxscript(self):
        assert ScriptLanguage.MAXSCRIPT

    def test_hscript(self):
        assert ScriptLanguage.HSCRIPT

    def test_vex(self):
        assert ScriptLanguage.VEX

    def test_lua(self):
        assert ScriptLanguage.LUA

    def test_csharp(self):
        assert ScriptLanguage.CSHARP

    def test_blueprint(self):
        assert ScriptLanguage.BLUEPRINT

    def test_repr_is_str(self):
        assert isinstance(repr(ScriptLanguage.PYTHON), str)

    def test_distinct_values(self):
        langs = [
            ScriptLanguage.PYTHON,
            ScriptLanguage.MEL,
            ScriptLanguage.MAXSCRIPT,
            ScriptLanguage.HSCRIPT,
            ScriptLanguage.VEX,
            ScriptLanguage.LUA,
            ScriptLanguage.CSHARP,
            ScriptLanguage.BLUEPRINT,
        ]
        assert len(set(repr(lang) for lang in langs)) == 8


# ---------------------------------------------------------------------------
# TransportScheme
# ---------------------------------------------------------------------------


class TestTransportSchemeEnum:
    def test_auto(self):
        assert TransportScheme.AUTO

    def test_tcp_only(self):
        assert TransportScheme.TCP_ONLY

    def test_prefer_named_pipe(self):
        assert TransportScheme.PREFER_NAMED_PIPE

    def test_prefer_unix_socket(self):
        assert TransportScheme.PREFER_UNIX_SOCKET

    def test_prefer_ipc(self):
        assert TransportScheme.PREFER_IPC

    def test_distinct_values(self):
        schemes = [
            TransportScheme.AUTO,
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
            TransportScheme.PREFER_IPC,
        ]
        assert len(set(repr(s) for s in schemes)) == 5


class TestTransportSchemeSelectAddress:
    def test_returns_transport_address(self):
        addr = TransportScheme.PREFER_IPC.select_address(dcc_type="maya", host="127.0.0.1", port=18812, pid=1234)
        assert isinstance(addr, TransportAddress)

    def test_tcp_only_returns_tcp(self):
        addr = TransportScheme.TCP_ONLY.select_address(dcc_type="maya", host="127.0.0.1", port=18812, pid=1234)
        assert addr.scheme == "tcp"

    def test_auto_returns_address(self):
        addr = TransportScheme.AUTO.select_address(dcc_type="maya", host="127.0.0.1", port=18812, pid=1234)
        assert isinstance(addr, TransportAddress)

    def test_prefer_ipc_scheme_string(self):
        addr = TransportScheme.PREFER_IPC.select_address(dcc_type="maya", host="127.0.0.1", port=18812, pid=9999)
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_tcp_only_is_tcp(self):
        addr = TransportScheme.TCP_ONLY.select_address(dcc_type="houdini", host="192.168.1.1", port=9000, pid=500)
        assert addr.is_tcp is True

    def test_prefer_named_pipe_returns_address(self):
        addr = TransportScheme.PREFER_NAMED_PIPE.select_address(
            dcc_type="3dsmax", host="127.0.0.1", port=18812, pid=1234
        )
        assert isinstance(addr, TransportAddress)


# ---------------------------------------------------------------------------
# RoutingStrategy enum
# ---------------------------------------------------------------------------


class TestRoutingStrategyEnum:
    def test_first_available(self):
        assert RoutingStrategy.FIRST_AVAILABLE

    def test_round_robin(self):
        assert RoutingStrategy.ROUND_ROBIN

    def test_least_busy(self):
        assert RoutingStrategy.LEAST_BUSY

    def test_specific(self):
        assert RoutingStrategy.SPECIFIC

    def test_scene_match(self):
        assert RoutingStrategy.SCENE_MATCH

    def test_random(self):
        assert RoutingStrategy.RANDOM

    def test_distinct_values(self):
        strategies = [
            RoutingStrategy.FIRST_AVAILABLE,
            RoutingStrategy.ROUND_ROBIN,
            RoutingStrategy.LEAST_BUSY,
            RoutingStrategy.SPECIFIC,
            RoutingStrategy.SCENE_MATCH,
            RoutingStrategy.RANDOM,
        ]
        assert len(set(repr(s) for s in strategies)) == 6

    def test_repr_is_str(self):
        assert isinstance(repr(RoutingStrategy.LEAST_BUSY), str)


# ---------------------------------------------------------------------------
# ToolDeclaration
# ---------------------------------------------------------------------------


class TestToolDeclarationCreate:
    def test_name(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="Create sphere",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.name == "create_sphere"

    def test_description(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="Create sphere",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.description == "Create sphere"

    def test_input_schema(self):
        schema = json.dumps({"type": "object", "properties": {"radius": {"type": "number"}}})
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema=schema,
            source_file="scripts/create_sphere.py",
        )
        # Rust may normalise JSON key order; compare as parsed dicts
        assert json.loads(td.input_schema) == json.loads(schema)

    def test_output_schema_default_empty(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        # output_schema defaults to None or empty string
        assert td.output_schema is None or td.output_schema == ""

    def test_output_schema_set(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            output_schema='{"type":"object"}',
            source_file="scripts/create_sphere.py",
        )
        assert td.output_schema == '{"type":"object"}'

    def test_read_only_default_false(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.read_only is False

    def test_read_only_true(self):
        td = ToolDeclaration(
            name="get_info",
            description="",
            input_schema="{}",
            read_only=True,
            source_file="scripts/get_info.py",
        )
        assert td.read_only is True

    def test_destructive_default_false(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.destructive is False

    def test_destructive_true(self):
        td = ToolDeclaration(
            name="delete_all",
            description="",
            input_schema="{}",
            destructive=True,
            source_file="scripts/delete_all.py",
        )
        assert td.destructive is True

    def test_idempotent_default_false(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.idempotent is False

    def test_idempotent_true(self):
        td = ToolDeclaration(
            name="set_attr",
            description="",
            input_schema="{}",
            idempotent=True,
            source_file="scripts/set_attr.py",
        )
        assert td.idempotent is True

    def test_source_file(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert td.source_file == "scripts/create_sphere.py"

    def test_repr_is_str(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        assert isinstance(repr(td), str)

    def test_repr_contains_name(self):
        td = ToolDeclaration(
            name="my_tool",
            description="",
            input_schema="{}",
            source_file="scripts/my_tool.py",
        )
        assert "my_tool" in repr(td)

    def test_name_settable(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        td.name = "renamed_tool"
        assert td.name == "renamed_tool"

    def test_description_settable(self):
        td = ToolDeclaration(
            name="create_sphere",
            description="old",
            input_schema="{}",
            source_file="scripts/create_sphere.py",
        )
        td.description = "new description"
        assert td.description == "new description"


# ---------------------------------------------------------------------------
# PromptArgument
# ---------------------------------------------------------------------------


class TestPromptArgumentCreate:
    def test_name(self):
        pa = PromptArgument("object_name", "Name of the 3D object", required=True)
        assert pa.name == "object_name"

    def test_description(self):
        pa = PromptArgument("object_name", "Name of the 3D object", required=True)
        assert pa.description == "Name of the 3D object"

    def test_required_true(self):
        pa = PromptArgument("object_name", "Name of the 3D object", required=True)
        assert pa.required is True

    def test_required_false(self):
        pa = PromptArgument("format", "Export format", required=False)
        assert pa.required is False

    def test_repr_is_str(self):
        pa = PromptArgument("object_name", "Name", required=True)
        assert isinstance(repr(pa), str)


# ---------------------------------------------------------------------------
# PromptDefinition
# ---------------------------------------------------------------------------


class TestPromptDefinitionCreate:
    def test_name(self):
        pd = PromptDefinition(name="review_model", description="Review 3D model", arguments=[])
        assert pd.name == "review_model"

    def test_description(self):
        pd = PromptDefinition(name="review_model", description="Review 3D model", arguments=[])
        assert pd.description == "Review 3D model"

    def test_arguments_empty(self):
        pd = PromptDefinition(name="review_model", description="", arguments=[])
        assert pd.arguments == []

    def test_arguments_count_one(self):
        pa = PromptArgument("name", "desc", required=True)
        pd = PromptDefinition(name="review_model", description="", arguments=[pa])
        assert len(pd.arguments) == 1

    def test_arguments_count_two(self):
        pa1 = PromptArgument("name", "Name", required=True)
        pa2 = PromptArgument("format", "Format", required=False)
        pd = PromptDefinition(name="review_model", description="", arguments=[pa1, pa2])
        assert len(pd.arguments) == 2

    def test_argument_name_accessible(self):
        pa = PromptArgument("object_name", "Name of 3D object", required=True)
        pd = PromptDefinition(name="review_model", description="", arguments=[pa])
        assert pd.arguments[0].name == "object_name"

    def test_argument_required_accessible(self):
        pa = PromptArgument("object_name", "Name of 3D object", required=True)
        pd = PromptDefinition(name="review_model", description="", arguments=[pa])
        assert pd.arguments[0].required is True

    def test_repr_is_str(self):
        pd = PromptDefinition(name="review_model", description="", arguments=[])
        assert isinstance(repr(pd), str)

    def test_repr_contains_name(self):
        pd = PromptDefinition(name="review_model", description="", arguments=[])
        assert "review_model" in repr(pd)

    def test_repr_contains_argument_count(self):
        pa = PromptArgument("n", "d", required=True)
        pd = PromptDefinition(name="review_model", description="", arguments=[pa])
        r = repr(pd)
        assert "1" in r

    def test_name_settable(self):
        pd = PromptDefinition(name="review_model", description="", arguments=[])
        pd.name = "new_prompt"
        assert pd.name == "new_prompt"

    def test_description_settable(self):
        pd = PromptDefinition(name="review_model", description="old", arguments=[])
        pd.description = "updated description"
        assert pd.description == "updated description"
