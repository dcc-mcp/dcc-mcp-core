"""Tests for DCC adapter Python types.

Covers PyScriptLanguage, PyDccErrorCode, PyDccInfo, PyScriptResult,
PySceneStatistics, PySceneInfo, PyDccCapabilities, PyDccError,
and PyCaptureResult exposed through adapters_python.rs.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import pytest

# Import local modules
import dcc_mcp_core

# ── ScriptLanguage ──


class TestScriptLanguage:
    def test_python_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.PYTHON
        assert str(lang) == "PYTHON"
        assert "PYTHON" in repr(lang)

    def test_mel_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.MEL
        assert str(lang) == "MEL"

    def test_maxscript_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.MAXSCRIPT
        assert str(lang) == "MAXSCRIPT"

    def test_hscript_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.HSCRIPT
        assert str(lang) == "HSCRIPT"

    def test_vex_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.VEX
        assert str(lang) == "VEX"

    def test_lua_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.LUA
        assert str(lang) == "LUA"

    def test_csharp_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.CSHARP
        assert str(lang) == "CSHARP"

    def test_blueprint_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.BLUEPRINT
        assert str(lang) == "BLUEPRINT"

    def test_equality(self) -> None:
        assert dcc_mcp_core.ScriptLanguage.PYTHON == dcc_mcp_core.ScriptLanguage.PYTHON
        assert dcc_mcp_core.ScriptLanguage.MEL != dcc_mcp_core.ScriptLanguage.PYTHON

    def test_repr_format(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.PYTHON
        r = repr(lang)
        assert r.startswith("ScriptLanguage.")


# ── DccErrorCode ──


class TestDccErrorCode:
    def test_connection_failed(self) -> None:
        code = dcc_mcp_core.DccErrorCode.CONNECTION_FAILED
        assert str(code) == "CONNECTION_FAILED"
        assert "CONNECTION_FAILED" in repr(code)

    def test_timeout(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.TIMEOUT) == "TIMEOUT"

    def test_script_error(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.SCRIPT_ERROR) == "SCRIPT_ERROR"

    def test_not_responding(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.NOT_RESPONDING) == "NOT_RESPONDING"

    def test_unsupported(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.UNSUPPORTED) == "UNSUPPORTED"

    def test_permission_denied(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.PERMISSION_DENIED) == "PERMISSION_DENIED"

    def test_invalid_input(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.INVALID_INPUT) == "INVALID_INPUT"

    def test_scene_error(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.SCENE_ERROR) == "SCENE_ERROR"

    def test_internal(self) -> None:
        assert str(dcc_mcp_core.DccErrorCode.INTERNAL) == "INTERNAL"

    def test_equality(self) -> None:
        assert dcc_mcp_core.DccErrorCode.TIMEOUT == dcc_mcp_core.DccErrorCode.TIMEOUT
        assert dcc_mcp_core.DccErrorCode.TIMEOUT != dcc_mcp_core.DccErrorCode.INTERNAL

    def test_repr_format(self) -> None:
        r = repr(dcc_mcp_core.DccErrorCode.INTERNAL)
        assert r.startswith("DccErrorCode.")


# ── DccInfo ──


class TestDccInfo:
    def test_required_fields(self) -> None:
        info = dcc_mcp_core.DccInfo("maya", "2024.2", "windows", 12345)
        assert info.dcc_type == "maya"
        assert info.version == "2024.2"
        assert info.platform == "windows"
        assert info.pid == 12345
        assert info.python_version is None
        assert info.metadata == {}

    def test_optional_python_version(self) -> None:
        info = dcc_mcp_core.DccInfo("blender", "4.0", "linux", 9999, python_version="3.11.0")
        assert info.python_version == "3.11.0"

    def test_optional_metadata(self) -> None:
        info = dcc_mcp_core.DccInfo("houdini", "20.0", "windows", 5000, metadata={"key": "value"})
        assert info.metadata == {"key": "value"}

    def test_to_dict_basic(self) -> None:
        info = dcc_mcp_core.DccInfo("maya", "2024.2", "windows", 12345)
        d = info.to_dict()
        assert d["dcc_type"] == "maya"
        assert d["version"] == "2024.2"
        assert d["platform"] == "windows"
        assert d["pid"] == 12345
        assert d["python_version"] is None
        assert d["metadata"] == {}

    def test_to_dict_full(self) -> None:
        info = dcc_mcp_core.DccInfo(
            "maya",
            "2024.2",
            "windows",
            12345,
            python_version="3.10",
            metadata={"build": "release"},
        )
        d = info.to_dict()
        assert d["python_version"] == "3.10"
        assert d["metadata"] == {"build": "release"}

    def test_repr(self) -> None:
        info = dcc_mcp_core.DccInfo("maya", "2024.2", "windows", 12345)
        r = repr(info)
        assert "maya" in r
        assert "12345" in r

    def test_missing_required_args(self) -> None:
        with pytest.raises(TypeError):
            dcc_mcp_core.DccInfo("maya")  # type: ignore[call-arg]


# ── DccError ──


class TestDccError:
    def test_basic_construction(self) -> None:
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.SCRIPT_ERROR, "execution failed")
        assert err.code == dcc_mcp_core.DccErrorCode.SCRIPT_ERROR
        assert err.message == "execution failed"
        assert err.details is None
        assert err.recoverable is False

    def test_optional_details(self) -> None:
        err = dcc_mcp_core.DccError(
            dcc_mcp_core.DccErrorCode.TIMEOUT,
            "timed out",
            details="stack trace here",
        )
        assert err.details == "stack trace here"

    def test_recoverable_flag(self) -> None:
        err = dcc_mcp_core.DccError(
            dcc_mcp_core.DccErrorCode.CONNECTION_FAILED,
            "could not connect",
            recoverable=True,
        )
        assert err.recoverable is True

    def test_str_format(self) -> None:
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.SCRIPT_ERROR, "bad script")
        s = str(err)
        assert "SCRIPT_ERROR" in s
        assert "bad script" in s

    def test_repr(self) -> None:
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.INTERNAL, "internal error")
        r = repr(err)
        assert "INTERNAL" in r
        assert "internal error" in r

    def test_repr_includes_recoverable(self) -> None:
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.TIMEOUT, "timed", recoverable=True)
        r = repr(err)
        assert "true" in r.lower() or "True" in r


# ── SceneStatistics ──


class TestSceneStatistics:
    def test_default_construction(self) -> None:
        stats = dcc_mcp_core.SceneStatistics()
        assert stats.object_count == 0
        assert stats.vertex_count == 0
        assert stats.polygon_count == 0
        assert stats.material_count == 0
        assert stats.texture_count == 0
        assert stats.light_count == 0
        assert stats.camera_count == 0

    def test_construction_with_values(self) -> None:
        stats = dcc_mcp_core.SceneStatistics(
            object_count=10,
            vertex_count=2000,
            polygon_count=1000,
            material_count=5,
            texture_count=8,
            light_count=3,
            camera_count=2,
        )
        assert stats.object_count == 10
        assert stats.vertex_count == 2000
        assert stats.polygon_count == 1000
        assert stats.material_count == 5
        assert stats.texture_count == 8
        assert stats.light_count == 3
        assert stats.camera_count == 2

    def test_setters(self) -> None:
        stats = dcc_mcp_core.SceneStatistics()
        stats.object_count = 50
        stats.vertex_count = 5000
        stats.polygon_count = 2500
        stats.material_count = 10
        stats.texture_count = 20
        stats.light_count = 4
        stats.camera_count = 1
        assert stats.object_count == 50
        assert stats.vertex_count == 5000
        assert stats.polygon_count == 2500
        assert stats.material_count == 10
        assert stats.texture_count == 20
        assert stats.light_count == 4
        assert stats.camera_count == 1

    def test_repr(self) -> None:
        stats = dcc_mcp_core.SceneStatistics(object_count=5, vertex_count=500, polygon_count=250)
        r = repr(stats)
        assert "5" in r
        assert "500" in r
        assert "250" in r


# ── SceneInfo ──


class TestSceneInfo:
    def test_default_construction(self) -> None:
        scene = dcc_mcp_core.SceneInfo()
        assert scene.file_path == ""
        assert scene.name == "untitled"
        assert scene.modified is False
        assert scene.format == ""
        assert scene.frame_range is None
        assert scene.current_frame is None
        assert scene.fps is None
        assert scene.up_axis is None
        assert scene.units is None
        assert scene.metadata == {}

    def test_construction_with_values(self) -> None:
        scene = dcc_mcp_core.SceneInfo(
            file_path="/projects/shot.ma",
            name="shot_001",
            modified=True,
            format="maya",
            frame_range=(1.0, 120.0),
            current_frame=42.0,
            fps=24.0,
            up_axis="Y",
            units="cm",
        )
        assert scene.file_path == "/projects/shot.ma"
        assert scene.name == "shot_001"
        assert scene.modified is True
        assert scene.format == "maya"
        assert scene.frame_range == (1.0, 120.0)
        assert scene.current_frame == 42.0
        assert scene.fps == 24.0
        assert scene.up_axis == "Y"
        assert scene.units == "cm"

    def test_statistics_default(self) -> None:
        scene = dcc_mcp_core.SceneInfo()
        assert scene.statistics.object_count == 0

    def test_statistics_custom(self) -> None:
        stats = dcc_mcp_core.SceneStatistics(object_count=100)
        scene = dcc_mcp_core.SceneInfo(statistics=stats)
        assert scene.statistics.object_count == 100

    def test_metadata(self) -> None:
        scene = dcc_mcp_core.SceneInfo(metadata={"render_engine": "arnold"})
        assert scene.metadata == {"render_engine": "arnold"}

    def test_repr(self) -> None:
        scene = dcc_mcp_core.SceneInfo(name="test_scene", modified=True)
        r = repr(scene)
        assert "test_scene" in r
        assert "true" in r.lower() or "True" in r


# ── DccCapabilities ──


class TestDccCapabilities:
    def test_default_construction(self) -> None:
        caps = dcc_mcp_core.DccCapabilities()
        assert caps.script_languages == []
        assert caps.scene_info is False
        assert caps.snapshot is False
        assert caps.undo_redo is False
        assert caps.progress_reporting is False
        assert caps.file_operations is False
        assert caps.selection is False
        assert caps.extensions == {}

    def test_construction_with_languages(self) -> None:
        caps = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL]
        )
        assert len(caps.script_languages) == 2
        assert dcc_mcp_core.ScriptLanguage.PYTHON in caps.script_languages
        assert dcc_mcp_core.ScriptLanguage.MEL in caps.script_languages

    def test_boolean_fields(self) -> None:
        caps = dcc_mcp_core.DccCapabilities(
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            progress_reporting=True,
            file_operations=True,
            selection=True,
        )
        assert caps.scene_info is True
        assert caps.snapshot is True
        assert caps.undo_redo is True
        assert caps.progress_reporting is True
        assert caps.file_operations is True
        assert caps.selection is True

    def test_extensions(self) -> None:
        caps = dcc_mcp_core.DccCapabilities(extensions={"xgen": True, "bifrost": False})
        assert caps.extensions == {"xgen": True, "bifrost": False}

    def test_setters(self) -> None:
        caps = dcc_mcp_core.DccCapabilities()
        caps.scene_info = True
        caps.snapshot = True
        caps.undo_redo = False
        assert caps.scene_info is True
        assert caps.snapshot is True
        assert caps.undo_redo is False

    def test_repr(self) -> None:
        caps = dcc_mcp_core.DccCapabilities(script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON], scene_info=True)
        r = repr(caps)
        assert "1" in r  # languages count
        assert "true" in r.lower() or "True" in r


# ── ScriptResult ──


class TestScriptResult:
    def test_success_result(self) -> None:
        result = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=42, output="result_value")
        assert result.success is True
        assert result.execution_time_ms == 42
        assert result.output == "result_value"
        assert result.error is None
        assert result.context == {}

    def test_failure_result(self) -> None:
        result = dcc_mcp_core.ScriptResult(
            success=False,
            execution_time_ms=100,
            error="NameError: undefined variable",
        )
        assert result.success is False
        assert result.error == "NameError: undefined variable"
        assert result.output is None

    def test_with_context(self) -> None:
        result = dcc_mcp_core.ScriptResult(
            success=True,
            execution_time_ms=10,
            context={"nodes_created": "3"},
        )
        assert result.context == {"nodes_created": "3"}

    def test_to_dict_success(self) -> None:
        result = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=55, output="done")
        d = result.to_dict()
        assert d["success"] is True
        assert d["execution_time_ms"] == 55
        assert d["output"] == "done"
        assert d["error"] is None
        assert d["context"] == {}

    def test_to_dict_failure(self) -> None:
        result = dcc_mcp_core.ScriptResult(success=False, execution_time_ms=0, error="script failed")
        d = result.to_dict()
        assert d["success"] is False
        assert d["error"] == "script failed"
        assert d["output"] is None

    def test_repr(self) -> None:
        result = dcc_mcp_core.ScriptResult(success=True, execution_time_ms=42)
        r = repr(result)
        assert "42" in r
        assert "true" in r.lower() or "True" in r


# ── CaptureResult ──


class TestCaptureResult:
    def test_basic_construction(self) -> None:
        data = b"\x89PNG\r\n" + b"\x00" * 100
        cap = dcc_mcp_core.CaptureResult(data=data, width=1920, height=1080, format="PNG")
        assert cap.width == 1920
        assert cap.height == 1080
        assert cap.format == "PNG"
        assert cap.viewport is None

    def test_with_viewport(self) -> None:
        cap = dcc_mcp_core.CaptureResult(data=b"imgdata", width=800, height=600, format="JPEG", viewport="persp")
        assert cap.viewport == "persp"

    def test_data_size(self) -> None:
        data = b"A" * 256
        cap = dcc_mcp_core.CaptureResult(data=data, width=16, height=16, format="PNG")
        assert cap.data_size() == 256

    def test_data_preserved(self) -> None:
        raw = bytes(range(256))
        cap = dcc_mcp_core.CaptureResult(data=raw, width=16, height=16, format="RAW")
        assert bytes(cap.data) == raw

    def test_empty_data(self) -> None:
        cap = dcc_mcp_core.CaptureResult(data=b"", width=0, height=0, format="PNG")
        assert cap.data_size() == 0

    def test_repr(self) -> None:
        cap = dcc_mcp_core.CaptureResult(data=b"x" * 10, width=640, height=480, format="PNG")
        r = repr(cap)
        assert "640" in r
        assert "480" in r
        assert "PNG" in r
        assert "10" in r

    def test_repr_no_viewport(self) -> None:
        cap = dcc_mcp_core.CaptureResult(data=b"img", width=100, height=100, format="JPEG")
        r = repr(cap)
        assert "100" in r


# ── Integration: DccInfo + DccCapabilities ──


class TestAdaptersIntegration:
    def test_maya_profile(self) -> None:
        """Simulate a realistic Maya adapter info + capabilities."""
        info = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2024.2",
            platform="windows",
            pid=42000,
            python_version="3.10.11",
            metadata={"maya_location": "C:/Program Files/Autodesk/Maya2024"},
        )
        caps = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL],
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            progress_reporting=False,
            file_operations=True,
            selection=True,
        )

        assert info.dcc_type == "maya"
        assert info.python_version == "3.10.11"
        assert caps.scene_info is True
        assert dcc_mcp_core.ScriptLanguage.MEL in caps.script_languages

    def test_unreal_profile(self) -> None:
        """Simulate a realistic Unreal Engine adapter info."""
        info = dcc_mcp_core.DccInfo(
            dcc_type="unreal",
            version="5.3.2",
            platform="windows",
            pid=99999,
            metadata={"remote_execution_port": "9998"},
        )
        caps = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.BLUEPRINT, dcc_mcp_core.ScriptLanguage.PYTHON],
            scene_info=True,
            snapshot=True,
            extensions={"remote_control": True},
        )
        assert info.dcc_type == "unreal"
        assert caps.extensions["remote_control"] is True

    def test_script_result_error_handling(self) -> None:
        """Simulate failed script execution with DccError."""
        script_result = dcc_mcp_core.ScriptResult(
            success=False,
            execution_time_ms=250,
            error="RuntimeError: cmds.polySphere not found",
        )
        err = dcc_mcp_core.DccError(
            dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            "Script execution failed",
            details=script_result.error,
            recoverable=True,
        )
        assert script_result.success is False
        assert err.code == dcc_mcp_core.DccErrorCode.SCRIPT_ERROR
        assert err.recoverable is True
        assert "cmds.polySphere" in (err.details or "")

    def test_scene_info_with_statistics(self) -> None:
        """Simulate scene info from a complex Maya scene."""
        stats = dcc_mcp_core.SceneStatistics(
            object_count=500,
            vertex_count=1_000_000,
            polygon_count=500_000,
            material_count=50,
            texture_count=120,
            light_count=10,
            camera_count=3,
        )
        scene = dcc_mcp_core.SceneInfo(
            file_path="/projects/hero_scene.ma",
            name="hero_scene",
            modified=False,
            format="maya",
            frame_range=(1001.0, 1100.0),
            current_frame=1001.0,
            fps=24.0,
            up_axis="Y",
            units="cm",
            statistics=stats,
        )
        assert scene.statistics.object_count == 500
        assert scene.statistics.vertex_count == 1_000_000
        assert scene.fps == 24.0
        assert scene.frame_range == (1001.0, 1100.0)
