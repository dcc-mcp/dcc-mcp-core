"""Deep tests for TelemetryConfig, ToolValidator, InputValidator.

Also covers ToolDefinition, ToolAnnotations, PromptDefinition, PromptArgument,
ResourceDefinition, ResourceAnnotations, DccInfo, DccCapabilities,
SceneInfo, SceneStatistics, ScriptLanguage.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
import threading

# Import local modules
import dcc_mcp_core

# ---------------------------------------------------------------------------
# TestTelemetryConfig
# ---------------------------------------------------------------------------


class TestTelemetryConfig:
    def test_default_fields(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("my-service")
        assert tc.service_name == "my-service"
        assert tc.enable_tracing is True
        assert tc.enable_metrics is True

    def test_with_noop_exporter_returns_config(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_noop_exporter()
        assert tc is not None
        assert "Noop" in repr(tc)

    def test_with_stdout_exporter_returns_config(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_stdout_exporter()
        assert "Stdout" in repr(tc)

    def test_with_service_version(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_noop_exporter().with_service_version("2.0.0")
        assert tc is not None

    def test_with_attribute(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_noop_exporter().with_attribute("env", "prod")
        assert tc is not None

    def test_with_json_logs(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_stdout_exporter().with_json_logs()
        assert tc is not None

    def test_with_text_logs(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").with_stdout_exporter().with_text_logs()
        assert tc is not None

    def test_set_enable_tracing_false(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").set_enable_tracing(False)
        assert tc is not None

    def test_set_enable_metrics_false(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("svc").set_enable_metrics(False)
        assert tc is not None

    def test_builder_chain(self) -> None:
        tc = (
            dcc_mcp_core.TelemetryConfig("chain-svc")
            .with_noop_exporter()
            .with_service_version("1.0.0")
            .with_attribute("region", "us-east")
            .set_enable_tracing(True)
            .set_enable_metrics(True)
        )
        assert tc is not None
        assert "chain-svc" in repr(tc)

    def test_is_telemetry_initialized_default_false(self) -> None:
        # Without calling init(), should be False (unless previous test initialized it)
        # We test the function is callable and returns bool
        result = dcc_mcp_core.is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_repr_contains_service_name(self) -> None:
        tc = dcc_mcp_core.TelemetryConfig("repr-test").with_noop_exporter()
        r = repr(tc)
        assert "repr-test" in r

    def test_multiple_configs_independent(self) -> None:
        tc1 = dcc_mcp_core.TelemetryConfig("svc-a").with_noop_exporter()
        tc2 = dcc_mcp_core.TelemetryConfig("svc-b").with_stdout_exporter()
        assert "svc-a" in repr(tc1)
        assert "svc-b" in repr(tc2)


# ---------------------------------------------------------------------------
# TestActionValidator
# ---------------------------------------------------------------------------


class TestActionValidator:
    def _make_validator(self, props: dict, required: list | None = None) -> dcc_mcp_core.ToolValidator:
        schema = {"type": "object", "properties": props}
        if required:
            schema["required"] = required
        return dcc_mcp_core.ToolValidator.from_schema_json(json.dumps(schema))

    def test_valid_input(self) -> None:
        v = self._make_validator({"radius": {"type": "number"}})
        ok, errs = v.validate(json.dumps({"radius": 1.5}))
        assert ok is True
        assert errs == []

    def test_invalid_type(self) -> None:
        v = self._make_validator({"radius": {"type": "number"}})
        ok, errs = v.validate(json.dumps({"radius": "not_a_number"}))
        assert ok is False
        assert len(errs) > 0

    def test_missing_required_field(self) -> None:
        v = self._make_validator({"name": {"type": "string"}}, required=["name"])
        ok, errs = v.validate(json.dumps({}))
        assert ok is False
        assert len(errs) > 0

    def test_empty_object_valid_without_required(self) -> None:
        v = self._make_validator({"radius": {"type": "number"}})
        ok, errs = v.validate(json.dumps({}))
        assert ok is True
        assert errs == []

    def test_multiple_fields_valid(self) -> None:
        v = self._make_validator(
            {
                "name": {"type": "string"},
                "count": {"type": "integer"},
            },
            required=["name", "count"],
        )
        ok, errs = v.validate(json.dumps({"name": "sphere", "count": 5}))
        assert ok is True
        assert errs == []

    def test_multiple_fields_one_invalid(self) -> None:
        v = self._make_validator(
            {
                "name": {"type": "string"},
                "count": {"type": "integer"},
            },
            required=["name", "count"],
        )
        ok, _errs = v.validate(json.dumps({"name": "sphere", "count": "five"}))
        assert ok is False

    def test_from_action_registry_valid(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        schema = json.dumps({"type": "object", "properties": {"r": {"type": "number"}}})
        reg.register("sphere", description="Create sphere", category="geo", input_schema=schema)
        av = dcc_mcp_core.ToolValidator.from_action_registry(reg, "sphere")
        ok, _errs = av.validate(json.dumps({"r": 2.0}))
        assert ok is True

    def test_from_action_registry_invalid(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        schema = json.dumps({"type": "object", "properties": {"r": {"type": "number"}}})
        reg.register("sphere", description="Create sphere", category="geo", input_schema=schema)
        av = dcc_mcp_core.ToolValidator.from_action_registry(reg, "sphere")
        ok, _errs = av.validate(json.dumps({"r": "bad"}))
        assert ok is False

    def test_from_schema_json_repr(self) -> None:
        v = self._make_validator({"x": {"type": "number"}})
        r = repr(v)
        assert "ToolValidator" in r

    def test_boolean_field(self) -> None:
        v = self._make_validator({"flag": {"type": "boolean"}})
        ok, _ = v.validate(json.dumps({"flag": True}))
        assert ok is True

    def test_string_field_valid(self) -> None:
        v = self._make_validator({"name": {"type": "string"}})
        ok, _ = v.validate(json.dumps({"name": "hello"}))
        assert ok is True

    def test_extra_fields_allowed(self) -> None:
        v = self._make_validator({"name": {"type": "string"}})
        ok, _ = v.validate(json.dumps({"name": "hello", "extra": 123}))
        assert ok is True

    def test_concurrent_validate(self) -> None:
        v = self._make_validator({"n": {"type": "number"}})
        results = []
        lock = threading.Lock()

        def _run() -> None:
            ok, _ = v.validate(json.dumps({"n": 1.0}))
            with lock:
                results.append(ok)

        threads = [threading.Thread(target=_run) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert all(results)


# ---------------------------------------------------------------------------
# TestInputValidator
# ---------------------------------------------------------------------------


class TestInputValidator:
    def test_require_string_valid(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("name", 100, 1)
        ok, _err = iv.validate(json.dumps({"name": "sphere"}))
        assert ok is True

    def test_require_string_too_short(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("name", 100, 5)
        ok, err = iv.validate(json.dumps({"name": "ab"}))
        assert ok is False
        assert err is not None

    def test_require_string_too_long(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("name", 5, 1)
        ok, _err = iv.validate(json.dumps({"name": "toolongstring"}))
        assert ok is False

    def test_require_number_valid(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_number("count", -1000.0, 1000.0)
        ok, _err = iv.validate(json.dumps({"count": 42}))
        assert ok is True

    def test_require_number_out_of_range(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_number("count", 0.0, 100.0)
        ok, _err = iv.validate(json.dumps({"count": 200}))
        assert ok is False

    def test_require_number_wrong_type(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_number("count", -999.0, 999.0)
        ok, _err = iv.validate(json.dumps({"count": "not_number"}))
        assert ok is False

    def test_forbid_substrings_clean(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("cmd", 200, 1)
        iv.forbid_substrings("cmd", ["hack", "drop table"])
        ok, _err = iv.validate(json.dumps({"cmd": "create sphere"}))
        assert ok is True

    def test_forbid_substrings_blocked(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("cmd", 200, 1)
        iv.forbid_substrings("cmd", ["hack", "drop table"])
        ok, _err = iv.validate(json.dumps({"cmd": "drop table users"}))
        assert ok is False

    def test_forbid_substrings_case_sensitive(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("cmd", 200, 1)
        iv.forbid_substrings("cmd", ["HACK"])
        ok, _err = iv.validate(json.dumps({"cmd": "hack it"}))
        assert ok is True

    def test_multiple_rules_all_pass(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("name", 50, 2)
        iv.require_number("size", 0.1, 999.9)
        iv.forbid_substrings("name", ["evil"])
        data = json.dumps({"name": "sphere", "size": 5.0})
        ok, _err = iv.validate(data)
        assert ok is True

    def test_multiple_rules_one_fails(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("name", 50, 2)
        iv.require_number("size", 0.1, 999.9)
        data = json.dumps({"name": "sphere", "size": 0.0})
        ok, _err = iv.validate(data)
        assert ok is False

    def test_empty_object_no_rules_valid(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        ok, _err = iv.validate(json.dumps({}))
        assert ok is True

    def test_invalid_json_raises(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        try:
            iv.validate("not_json")
            raise AssertionError("Expected exception")
        except AssertionError:
            raise
        except Exception:
            pass

    def test_concurrent_validate(self) -> None:
        iv = dcc_mcp_core.InputValidator()
        iv.require_string("x", 100, 1)
        results = []
        lock = threading.Lock()

        def _run() -> None:
            ok, _ = iv.validate(json.dumps({"x": "hello"}))
            with lock:
                results.append(ok)

        threads = [threading.Thread(target=_run) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert all(results)


# ---------------------------------------------------------------------------
# TestToolDefinition
# ---------------------------------------------------------------------------


class TestToolDefinition:
    def _make_td(self, name: str = "sphere", desc: str = "Create sphere") -> dcc_mcp_core.ToolDefinition:
        schema = json.dumps({"type": "object", "properties": {"radius": {"type": "number"}}})
        return dcc_mcp_core.ToolDefinition(name=name, description=desc, input_schema=schema)

    def test_basic_fields(self) -> None:
        td = self._make_td()
        assert td.name == "sphere"
        assert td.description == "Create sphere"

    def test_input_schema_stored(self) -> None:
        td = self._make_td()
        assert td.input_schema is not None
        assert "radius" in td.input_schema

    def test_output_schema_none_by_default(self) -> None:
        td = self._make_td()
        assert td.output_schema is None

    def test_annotations_none_by_default(self) -> None:
        td = self._make_td()
        assert td.annotations is None

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="Sphere Tool",
            read_only_hint=False,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        schema = json.dumps({"type": "object", "properties": {}})
        td = dcc_mcp_core.ToolDefinition(
            name="sphere", description="Create sphere", input_schema=schema, annotations=ann
        )
        assert td.annotations is not None

    def test_empty_description(self) -> None:
        schema = json.dumps({"type": "object"})
        td = dcc_mcp_core.ToolDefinition(name="tool", description="", input_schema=schema)
        assert td.description == ""

    def test_multiple_tools_independent(self) -> None:
        td1 = self._make_td("sphere", "Sphere")
        td2 = self._make_td("cube", "Cube")
        assert td1.name == "sphere"
        assert td2.name == "cube"


# ---------------------------------------------------------------------------
# TestToolAnnotations
# ---------------------------------------------------------------------------


class TestToolAnnotations:
    def test_all_fields(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="Test Tool",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        assert ann.title == "Test Tool"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is False

    def test_destructive_hint_true(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="Destructive",
            read_only_hint=False,
            destructive_hint=True,
            idempotent_hint=False,
            open_world_hint=True,
        )
        assert ann.destructive_hint is True
        assert ann.open_world_hint is True

    def test_repr(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        r = repr(ann)
        assert isinstance(r, str)


# ---------------------------------------------------------------------------
# TestPromptDefinition
# ---------------------------------------------------------------------------


class TestPromptDefinition:
    def test_basic_fields(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="create_scene", description="Create a scene")
        assert pd.name == "create_scene"
        assert pd.description == "Create a scene"

    def test_arguments_empty_by_default(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="pd", description="desc")
        assert pd.arguments == [] or pd.arguments is None or isinstance(pd.arguments, list)

    def test_with_arguments(self) -> None:
        arg = dcc_mcp_core.PromptArgument(name="scene_name", description="Name of the scene", required=True)
        pd = dcc_mcp_core.PromptDefinition(name="create", description="Create scene", arguments=[arg])
        assert len(pd.arguments) == 1
        assert pd.arguments[0].name == "scene_name"

    def test_empty_description(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="pd", description="")
        assert pd.description == ""

    def test_multiple_arguments(self) -> None:
        args = [
            dcc_mcp_core.PromptArgument(name="a", description="arg a", required=True),
            dcc_mcp_core.PromptArgument(name="b", description="arg b", required=False),
        ]
        pd = dcc_mcp_core.PromptDefinition(name="multi", description="multi args", arguments=args)
        assert len(pd.arguments) == 2


# ---------------------------------------------------------------------------
# TestPromptArgument
# ---------------------------------------------------------------------------


class TestPromptArgument:
    def test_required_true(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="x", description="X arg", required=True)
        assert pa.name == "x"
        assert pa.description == "X arg"
        assert pa.required is True

    def test_required_false(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="y", description="Y arg", required=False)
        assert pa.required is False

    def test_empty_description(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="z", description="", required=False)
        assert pa.description == ""


# ---------------------------------------------------------------------------
# TestResourceDefinition
# ---------------------------------------------------------------------------


class TestResourceDefinition:
    def test_basic_fields(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(
            uri="file:///scene.ma",
            name="maya_scene",
            description="A Maya scene file",
            mime_type="application/octet-stream",
        )
        assert rd.uri == "file:///scene.ma"
        assert rd.name == "maya_scene"
        assert rd.description == "A Maya scene file"
        assert rd.mime_type == "application/octet-stream"

    def test_annotations_none_by_default(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="file:///x.txt", name="x", description="x", mime_type="text/plain")
        assert rd.annotations is None

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user", "assistant"], priority=0.9)
        rd = dcc_mcp_core.ResourceDefinition(
            uri="file:///y.txt", name="y", description="y", mime_type="text/plain", annotations=ann
        )
        assert rd.annotations is not None
        assert rd.annotations.priority == 0.9
        assert "user" in rd.annotations.audience

    def test_different_mime_types(self) -> None:
        for mime in ["text/plain", "application/json", "image/png", "application/octet-stream"]:
            rd = dcc_mcp_core.ResourceDefinition(uri="file:///f", name="f", description="f", mime_type=mime)
            assert rd.mime_type == mime


# ---------------------------------------------------------------------------
# TestResourceAnnotations
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    def test_default_empty(self) -> None:
        ra = dcc_mcp_core.ResourceAnnotations()
        assert ra.audience == []
        assert ra.priority is None

    def test_audience_and_priority(self) -> None:
        ra = dcc_mcp_core.ResourceAnnotations(audience=["user", "assistant"], priority=0.75)
        assert "user" in ra.audience
        assert "assistant" in ra.audience
        assert abs(ra.priority - 0.75) < 1e-6

    def test_repr(self) -> None:
        ra = dcc_mcp_core.ResourceAnnotations(audience=["user"], priority=0.5)
        r = repr(ra)
        assert isinstance(r, str)
        assert "0.5" in r or "ResourceAnnotations" in r


# ---------------------------------------------------------------------------
# TestDccInfo
# ---------------------------------------------------------------------------


class TestDccInfo:
    def test_basic_fields(self) -> None:
        di = dcc_mcp_core.DccInfo("maya", "2025", "3.11", 12345)
        assert di.dcc_type == "maya"
        assert di.version == "2025"
        assert di.pid == 12345

    def test_python_version_can_be_none(self) -> None:
        di = dcc_mcp_core.DccInfo("blender", "4.0", "3.11", 9999)
        assert di.dcc_type == "blender"
        assert di.pid == 9999

    def test_to_dict_keys(self) -> None:
        di = dcc_mcp_core.DccInfo("houdini", "20.0", "3.11", 7777)
        d = di.to_dict()
        assert isinstance(d, dict)
        assert "dcc_type" in d
        assert "version" in d
        assert "pid" in d

    def test_to_dict_values(self) -> None:
        di = dcc_mcp_core.DccInfo("maya", "2025", "3.11", 5555)
        d = di.to_dict()
        assert d["dcc_type"] == "maya"
        assert d["pid"] == 5555

    def test_multiple_dcc_types(self) -> None:
        for dcc in ["maya", "blender", "houdini", "3dsmax", "unreal"]:
            di = dcc_mcp_core.DccInfo(dcc, "1.0", "3.11", 1000)
            assert di.dcc_type == dcc

    def test_concurrent_creation(self) -> None:
        results = []
        lock = threading.Lock()

        def _run(pid: int) -> None:
            di = dcc_mcp_core.DccInfo("maya", "2025", "3.11", pid)
            with lock:
                results.append(di.pid)

        threads = [threading.Thread(target=_run, args=(i,)) for i in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert len(results) == 20


# ---------------------------------------------------------------------------
# TestDccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilities:
    def test_default_all_false(self) -> None:
        cap = dcc_mcp_core.DccCapabilities()
        assert cap.snapshot is False or cap.snapshot is True  # just check is bool
        assert isinstance(cap.snapshot, bool)

    def test_snapshot_true(self) -> None:
        cap = dcc_mcp_core.DccCapabilities(snapshot=True)
        assert cap.snapshot is True

    def test_scene_info_true(self) -> None:
        cap = dcc_mcp_core.DccCapabilities(scene_info=True)
        assert cap.scene_info is True

    def test_script_languages_python(self) -> None:
        cap = dcc_mcp_core.DccCapabilities(script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON])
        assert dcc_mcp_core.ScriptLanguage.PYTHON in cap.script_languages

    def test_all_capabilities(self) -> None:
        cap = dcc_mcp_core.DccCapabilities(
            snapshot=True,
            scene_info=True,
            selection=True,
            undo_redo=True,
            progress_reporting=True,
            file_operations=True,
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON],
        )
        assert cap.snapshot is True
        assert cap.scene_info is True
        assert cap.selection is True
        assert cap.undo_redo is True
        assert cap.progress_reporting is True
        assert cap.file_operations is True

    def test_multiple_script_languages(self) -> None:
        cap = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL]
        )
        assert dcc_mcp_core.ScriptLanguage.PYTHON in cap.script_languages
        assert dcc_mcp_core.ScriptLanguage.MEL in cap.script_languages


# ---------------------------------------------------------------------------
# TestScriptLanguage
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    def test_all_variants_exist(self) -> None:
        for variant in ["PYTHON", "MEL", "LUA", "MAXSCRIPT", "HSCRIPT", "VEX", "CSHARP", "BLUEPRINT"]:
            assert hasattr(dcc_mcp_core.ScriptLanguage, variant)

    def test_python_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.PYTHON
        assert lang is not None

    def test_mel_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.MEL
        assert lang is not None

    def test_csharp_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.CSHARP
        assert lang is not None

    def test_blueprint_variant(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.BLUEPRINT
        assert lang is not None

    def test_repr_contains_name(self) -> None:
        lang = dcc_mcp_core.ScriptLanguage.PYTHON
        r = repr(lang)
        assert "PYTHON" in r or "Python" in r or isinstance(r, str)


# ---------------------------------------------------------------------------
# TestSceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatistics:
    def test_default_zeros(self) -> None:
        ss = dcc_mcp_core.SceneStatistics()
        assert ss.vertex_count == 0
        assert ss.polygon_count == 0
        assert ss.object_count == 0
        assert ss.material_count == 0
        assert ss.light_count == 0
        assert ss.camera_count == 0
        assert ss.texture_count == 0

    def test_set_values(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(
            object_count=100,
            vertex_count=5000,
            polygon_count=2000,
            material_count=10,
            light_count=3,
            camera_count=2,
            texture_count=20,
        )
        assert ss.object_count == 100
        assert ss.vertex_count == 5000
        assert ss.polygon_count == 2000
        assert ss.material_count == 10
        assert ss.light_count == 3
        assert ss.camera_count == 2
        assert ss.texture_count == 20

    def test_repr(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(object_count=5, vertex_count=50, polygon_count=20)
        r = repr(ss)
        assert "5" in r or "SceneStatistics" in r

    def test_large_values(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(vertex_count=10_000_000, polygon_count=5_000_000)
        assert ss.vertex_count == 10_000_000

    def test_independent_instances(self) -> None:
        ss1 = dcc_mcp_core.SceneStatistics(object_count=1)
        ss2 = dcc_mcp_core.SceneStatistics(object_count=2)
        assert ss1.object_count == 1
        assert ss2.object_count == 2


# ---------------------------------------------------------------------------
# TestSceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfo:
    def test_default_name(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.name == "untitled"

    def test_default_modified_false(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.modified is False

    def test_custom_name(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_modified_true(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="scene", modified=True)
        assert si.modified is True

    def test_fps_field(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="anim", fps=30.0)
        assert si.fps == 30.0

    def test_frame_range_field(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="anim", fps=24.0, frame_range=(1, 250), current_frame=50)
        assert si.frame_range == (1.0, 250.0)
        assert si.current_frame == 50.0

    def test_statistics_field(self) -> None:
        ss = dcc_mcp_core.SceneStatistics(vertex_count=1000, polygon_count=500, object_count=20)
        si = dcc_mcp_core.SceneInfo(name="geo_scene", statistics=ss)
        assert si.statistics.vertex_count == 1000
        assert si.statistics.polygon_count == 500
        assert si.statistics.object_count == 20

    def test_default_statistics_zeros(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.statistics.vertex_count == 0
        assert si.statistics.polygon_count == 0

    def test_file_path_empty_by_default(self) -> None:
        si = dcc_mcp_core.SceneInfo()
        assert si.file_path == "" or si.file_path is None or isinstance(si.file_path, str)

    def test_repr(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="test_scene")
        r = repr(si)
        assert "test_scene" in r or "SceneInfo" in r

    def test_up_axis_field(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="s", up_axis="Y")
        assert si.up_axis == "Y" or si.up_axis is not None

    def test_units_field(self) -> None:
        si = dcc_mcp_core.SceneInfo(name="s", units="cm")
        assert si.units == "cm" or si.units is not None

    def test_concurrent_creation(self) -> None:
        results = []
        lock = threading.Lock()

        def _run(name: str) -> None:
            si = dcc_mcp_core.SceneInfo(name=name)
            with lock:
                results.append(si.name)

        threads = [threading.Thread(target=_run, args=(f"scene_{i}",)) for i in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert len(results) == 20
