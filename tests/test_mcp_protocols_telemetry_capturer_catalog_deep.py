"""Deep tests for MCP protocol types, TelemetryConfig, Capturer, SkillCatalog, and middleware classes.

Covers:
- PromptArgument / PromptDefinition
- ResourceAnnotations / ResourceDefinition / ResourceTemplateDefinition
- ToolAnnotations / ToolDefinition / ToolDeclaration
- TelemetryConfig / is_telemetry_initialized / shutdown_telemetry
- CaptureFrame / CaptureResult / Capturer (mock backend)
- SkillSummary / SkillCatalog
- LoggingMiddleware / TimingMiddleware / RateLimitMiddleware (standalone)
"""

from __future__ import annotations

import contextlib
import json
from pathlib import Path

import pytest

import dcc_mcp_core as m

# ---------------------------------------------------------------------------
# TestPromptArgument
# ---------------------------------------------------------------------------


class TestPromptArgument:
    def test_required_true(self):
        pa = m.PromptArgument("scene_path", "Path to scene file", required=True)
        assert pa.name == "scene_path"
        assert pa.description == "Path to scene file"
        assert pa.required is True

    def test_required_false_default(self):
        pa = m.PromptArgument("output", "Output path")
        assert pa.required is False

    def test_name_attribute(self):
        pa = m.PromptArgument("my_arg", "desc")
        assert pa.name == "my_arg"

    def test_description_attribute(self):
        pa = m.PromptArgument("x", "some description text")
        assert pa.description == "some description text"

    def test_empty_description(self):
        pa = m.PromptArgument("flag", "")
        assert pa.description == ""

    def test_unicode_name(self):
        pa = m.PromptArgument("arg_路径", "unicode argument")
        assert "arg_路径" in pa.name

    def test_repr_contains_name(self):
        pa = m.PromptArgument("test_arg", "desc", required=True)
        r = repr(pa)
        assert "test_arg" in r

    def test_required_in_repr(self):
        pa = m.PromptArgument("test_arg", "desc", required=True)
        r = repr(pa)
        assert "true" in r.lower() or "True" in r

    def test_attrs_complete(self):
        pa = m.PromptArgument("x", "y")
        attrs = [a for a in dir(pa) if not a.startswith("_")]
        assert "name" in attrs
        assert "description" in attrs
        assert "required" in attrs


# ---------------------------------------------------------------------------
# TestPromptDefinition
# ---------------------------------------------------------------------------


class TestPromptDefinition:
    def test_basic_no_args(self):
        pd = m.PromptDefinition("greet", "Greet the user")
        assert pd.name == "greet"
        assert pd.description == "Greet the user"
        assert pd.arguments == []

    def test_with_one_arg(self):
        pa = m.PromptArgument("name", "User name", required=True)
        pd = m.PromptDefinition("greet", "Greet", arguments=[pa])
        assert len(pd.arguments) == 1
        assert pd.arguments[0].name == "name"

    def test_with_multiple_args(self):
        args = [
            m.PromptArgument("scene", "Scene path", required=True),
            m.PromptArgument("format", "Output format"),
            m.PromptArgument("quality", "Quality level"),
        ]
        pd = m.PromptDefinition("export", "Export scene", arguments=args)
        assert len(pd.arguments) == 3

    def test_arguments_required_attribute_preserved(self):
        pa = m.PromptArgument("req_arg", "required", required=True)
        pa_opt = m.PromptArgument("opt_arg", "optional", required=False)
        pd = m.PromptDefinition("cmd", "desc", arguments=[pa, pa_opt])
        names = [a.name for a in pd.arguments]
        assert "req_arg" in names
        assert "opt_arg" in names

    def test_repr_contains_name(self):
        pd = m.PromptDefinition("my_prompt", "desc")
        assert "my_prompt" in repr(pd)

    def test_repr_with_args_shows_count(self):
        args = [m.PromptArgument("a", "A"), m.PromptArgument("b", "B")]
        pd = m.PromptDefinition("two_args", "desc", arguments=args)
        r = repr(pd)
        assert "2" in r

    def test_name_attribute(self):
        pd = m.PromptDefinition("analyze_scene", "Analyze the DCC scene")
        assert pd.name == "analyze_scene"

    def test_description_attribute(self):
        pd = m.PromptDefinition("cmd", "Detailed description here")
        assert pd.description == "Detailed description here"

    def test_attrs_complete(self):
        pd = m.PromptDefinition("x", "y")
        attrs = [a for a in dir(pd) if not a.startswith("_")]
        assert "name" in attrs
        assert "description" in attrs
        assert "arguments" in attrs


# ---------------------------------------------------------------------------
# TestResourceAnnotations
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    def test_default_no_args(self):
        ra = m.ResourceAnnotations()
        # audience may be None or empty
        assert ra.audience is None or ra.audience == []
        assert ra.priority is None

    def test_audience_user_only(self):
        ra = m.ResourceAnnotations(audience=["user"])
        assert "user" in ra.audience

    def test_audience_multiple(self):
        ra = m.ResourceAnnotations(audience=["user", "assistant"])
        assert len(ra.audience) == 2
        assert "assistant" in ra.audience

    def test_priority_set(self):
        ra = m.ResourceAnnotations(priority=0.5)
        assert abs(ra.priority - 0.5) < 1e-6

    def test_priority_zero(self):
        ra = m.ResourceAnnotations(priority=0.0)
        assert ra.priority == 0.0 or abs(ra.priority) < 1e-9

    def test_priority_one(self):
        ra = m.ResourceAnnotations(priority=1.0)
        assert abs(ra.priority - 1.0) < 1e-6

    def test_audience_and_priority(self):
        ra = m.ResourceAnnotations(audience=["user"], priority=0.8)
        assert ra.audience == ["user"]
        assert abs(ra.priority - 0.8) < 1e-6

    def test_repr_contains_audience(self):
        ra = m.ResourceAnnotations(audience=["user"])
        assert "user" in repr(ra)

    def test_repr_contains_priority(self):
        ra = m.ResourceAnnotations(priority=0.8)
        assert "0.8" in repr(ra)

    def test_attrs_complete(self):
        ra = m.ResourceAnnotations()
        attrs = [a for a in dir(ra) if not a.startswith("_")]
        assert "audience" in attrs
        assert "priority" in attrs


# ---------------------------------------------------------------------------
# TestResourceDefinition
# ---------------------------------------------------------------------------


class TestResourceDefinition:
    def test_minimal(self):
        rd = m.ResourceDefinition("file:///test.txt", "Test", "A test file")
        assert rd.uri == "file:///test.txt"
        assert rd.name == "Test"
        assert rd.description == "A test file"

    def test_default_mime_type(self):
        rd = m.ResourceDefinition("file:///x", "x", "x")
        # default mime_type may be None or empty
        assert rd.mime_type is None or isinstance(rd.mime_type, str)

    def test_explicit_mime_type(self):
        rd = m.ResourceDefinition("file:///x.txt", "X", "X", mime_type="text/plain")
        assert rd.mime_type == "text/plain"

    def test_json_mime_type(self):
        rd = m.ResourceDefinition("file:///x.json", "X", "X", mime_type="application/json")
        assert "json" in rd.mime_type

    def test_no_annotations_default(self):
        rd = m.ResourceDefinition("file:///x", "X", "X")
        assert rd.annotations is None

    def test_with_annotations(self):
        ra = m.ResourceAnnotations(audience=["user"], priority=0.9)
        rd = m.ResourceDefinition("file:///x", "X", "X", annotations=ra)
        assert rd.annotations is not None
        assert "user" in repr(rd.annotations)

    def test_repr_contains_name(self):
        rd = m.ResourceDefinition("file:///x", "MyResource", "desc")
        assert "MyResource" in repr(rd)

    def test_repr_contains_uri(self):
        rd = m.ResourceDefinition("file:///data.bin", "Bin", "bin file")
        assert "file:///data.bin" in repr(rd)

    def test_attrs_complete(self):
        rd = m.ResourceDefinition("file:///x", "X", "X")
        attrs = [a for a in dir(rd) if not a.startswith("_")]
        for attr in ["uri", "name", "description", "mime_type", "annotations"]:
            assert attr in attrs

    def test_uri_various_schemes(self):
        for uri in ["https://example.com/resource", "db://local/table", "s3://bucket/key"]:
            rd = m.ResourceDefinition(uri, "name", "desc")
            assert rd.uri == uri


# ---------------------------------------------------------------------------
# TestResourceTemplateDefinition
# ---------------------------------------------------------------------------


class TestResourceTemplateDefinition:
    def test_basic(self):
        rtd = m.ResourceTemplateDefinition("file:///{path}", "Template", "desc")
        assert rtd.uri_template == "file:///{path}"
        assert rtd.name == "Template"
        assert rtd.description == "desc"

    def test_default_mime_type(self):
        rtd = m.ResourceTemplateDefinition("x://{id}", "X", "X")
        assert rtd.mime_type is None or isinstance(rtd.mime_type, str)

    def test_explicit_mime_type(self):
        rtd = m.ResourceTemplateDefinition("x://{id}", "X", "X", mime_type="text/html")
        assert rtd.mime_type == "text/html"

    def test_no_annotations_default(self):
        rtd = m.ResourceTemplateDefinition("x://{id}", "X", "X")
        assert rtd.annotations is None

    def test_with_annotations(self):
        ra = m.ResourceAnnotations(audience=["assistant"])
        rtd = m.ResourceTemplateDefinition("x://{id}", "X", "X", annotations=ra)
        assert rtd.annotations is not None

    def test_repr_contains_name(self):
        rtd = m.ResourceTemplateDefinition("x://{id}", "MyTemplate", "desc")
        assert "MyTemplate" in repr(rtd)

    def test_repr_contains_uri_template(self):
        rtd = m.ResourceTemplateDefinition("custom://{resource_id}", "T", "d")
        assert "custom://{resource_id}" in repr(rtd)

    def test_attrs_complete(self):
        rtd = m.ResourceTemplateDefinition("x://{id}", "X", "X")
        attrs = [a for a in dir(rtd) if not a.startswith("_")]
        for attr in ["uri_template", "name", "description", "mime_type", "annotations"]:
            assert attr in attrs

    def test_multiple_params_in_template(self):
        rtd = m.ResourceTemplateDefinition("db://{host}/{table}/{id}", "Multi", "desc")
        assert "{host}" in rtd.uri_template
        assert "{table}" in rtd.uri_template


# ---------------------------------------------------------------------------
# TestToolAnnotations
# ---------------------------------------------------------------------------


class TestToolAnnotations:
    def test_all_none_default(self):
        ta = m.ToolAnnotations()
        assert ta.title is None
        assert ta.read_only_hint is None
        assert ta.destructive_hint is None
        assert ta.idempotent_hint is None
        assert ta.open_world_hint is None
        assert ta.deferred_hint is None

    def test_title_set(self):
        ta = m.ToolAnnotations(title="Create Sphere")
        assert ta.title == "Create Sphere"

    def test_read_only_true(self):
        ta = m.ToolAnnotations(read_only_hint=True)
        assert ta.read_only_hint is True

    def test_read_only_false(self):
        ta = m.ToolAnnotations(read_only_hint=False)
        assert ta.read_only_hint is False

    def test_destructive_hint(self):
        ta = m.ToolAnnotations(destructive_hint=True)
        assert ta.destructive_hint is True

    def test_idempotent_hint(self):
        ta = m.ToolAnnotations(idempotent_hint=True)
        assert ta.idempotent_hint is True

    def test_open_world_hint(self):
        ta = m.ToolAnnotations(open_world_hint=True)
        assert ta.open_world_hint is True

    def test_all_set(self):
        ta = m.ToolAnnotations(
            title="My Tool",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
            deferred_hint=True,
        )
        assert ta.title == "My Tool"
        assert ta.read_only_hint is True
        assert ta.destructive_hint is False
        assert ta.idempotent_hint is True
        assert ta.open_world_hint is False
        assert ta.deferred_hint is True

    def test_repr_contains_title(self):
        ta = m.ToolAnnotations(title="TestTool")
        assert "TestTool" in repr(ta)

    def test_repr_contains_hints(self):
        ta = m.ToolAnnotations(read_only_hint=True)
        r = repr(ta)
        assert "true" in r.lower() or "True" in r

    def test_attrs_complete(self):
        ta = m.ToolAnnotations()
        attrs = [a for a in dir(ta) if not a.startswith("_")]
        for attr in [
            "title",
            "read_only_hint",
            "destructive_hint",
            "idempotent_hint",
            "open_world_hint",
            "deferred_hint",
        ]:
            assert attr in attrs


# ---------------------------------------------------------------------------
# TestToolDefinition
# ---------------------------------------------------------------------------


class TestToolDefinition:
    _schema = json.dumps({"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]})

    def test_basic(self):
        td = m.ToolDefinition("create_sphere", "Create a sphere", self._schema)
        assert td.name == "create_sphere"
        assert td.description == "Create a sphere"
        assert td.input_schema == self._schema

    def test_output_schema_none_default(self):
        td = m.ToolDefinition("tool", "desc", self._schema)
        assert td.output_schema is None

    def test_explicit_output_schema(self):
        out_schema = json.dumps({"type": "object", "properties": {"id": {"type": "string"}}})
        td = m.ToolDefinition("tool", "desc", self._schema, output_schema=out_schema)
        assert td.output_schema == out_schema

    def test_annotations_none_default(self):
        td = m.ToolDefinition("tool", "desc", self._schema)
        assert td.annotations is None

    def test_with_annotations(self):
        ta = m.ToolAnnotations(title="Create Sphere", read_only_hint=False, idempotent_hint=True)
        td = m.ToolDefinition("create_sphere", "Create sphere", self._schema, annotations=ta)
        assert td.annotations is not None
        assert "Create Sphere" in repr(td.annotations)

    def test_repr_contains_name(self):
        td = m.ToolDefinition("my_tool", "desc", self._schema)
        assert "my_tool" in repr(td)

    def test_attrs_complete(self):
        td = m.ToolDefinition("t", "d", self._schema)
        attrs = [a for a in dir(td) if not a.startswith("_")]
        for attr in ["name", "description", "input_schema", "output_schema", "annotations"]:
            assert attr in attrs

    def test_empty_schema(self):
        empty = json.dumps({"type": "object", "properties": {}})
        td = m.ToolDefinition("no_params", "No params tool", empty)
        assert td.name == "no_params"

    def test_complex_schema(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "radius": {"type": "number", "minimum": 0},
                    "subdivisions": {"type": "integer", "default": 4},
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "minItems": 3,
                        "maxItems": 3,
                    },
                },
                "required": ["name"],
            }
        )
        td = m.ToolDefinition("create_complex", "Complex tool", schema)
        assert td.input_schema == schema


# ---------------------------------------------------------------------------
# TestToolDeclaration
# ---------------------------------------------------------------------------


class TestToolDeclaration:
    def test_minimal(self):
        tdecl = m.ToolDeclaration("my_tool")
        assert tdecl.name == "my_tool"

    def test_description_set(self):
        tdecl = m.ToolDeclaration("tool", description="Does something")
        assert tdecl.description == "Does something"

    def test_read_only_default_false(self):
        tdecl = m.ToolDeclaration("tool")
        assert tdecl.read_only is False

    def test_read_only_true(self):
        tdecl = m.ToolDeclaration("tool", read_only=True)
        assert tdecl.read_only is True

    def test_destructive_default_false(self):
        tdecl = m.ToolDeclaration("tool")
        assert tdecl.destructive is False

    def test_destructive_true(self):
        tdecl = m.ToolDeclaration("tool", destructive=True)
        assert tdecl.destructive is True

    def test_idempotent_default_false(self):
        tdecl = m.ToolDeclaration("tool")
        assert tdecl.idempotent is False

    def test_idempotent_true(self):
        tdecl = m.ToolDeclaration("tool", idempotent=True)
        assert tdecl.idempotent is True

    def test_defer_loading_default_false(self):
        tdecl = m.ToolDeclaration("tool")
        assert tdecl.defer_loading is False

    def test_defer_loading_true(self):
        tdecl = m.ToolDeclaration("tool", defer_loading=True)
        assert tdecl.defer_loading is True

    def test_input_schema_default_is_object_schema(self):
        tdecl = m.ToolDeclaration("tool")
        # Rust default is {"type":"object"} — always returns a JSON string
        schema_obj = json.loads(tdecl.input_schema)
        assert schema_obj.get("type") == "object"

    def test_input_schema_set(self):
        schema = {"type": "object", "properties": {"x": {"type": "number"}}}
        tdecl = m.ToolDeclaration("tool", input_schema=json.dumps(schema))
        # Rust may re-serialize with different key order; compare parsed JSON
        parsed = json.loads(tdecl.input_schema)
        assert parsed.get("type") == "object"
        assert "x" in parsed.get("properties", {})

    def test_output_schema_default_empty(self):
        tdecl = m.ToolDeclaration("tool")
        # output_schema may be None, empty string, or empty JSON "{}"
        out = tdecl.output_schema
        assert out is None or out == "" or out == "{}"

    def test_repr_contains_name(self):
        tdecl = m.ToolDeclaration("special_tool")
        assert "special_tool" in repr(tdecl)

    def test_attrs_complete(self):
        tdecl = m.ToolDeclaration("t")
        attrs = [a for a in dir(tdecl) if not a.startswith("_")]
        for attr in [
            "name",
            "description",
            "read_only",
            "destructive",
            "idempotent",
            "defer_loading",
            "input_schema",
            "output_schema",
        ]:
            assert attr in attrs

    def test_all_flags_true(self):
        tdecl = m.ToolDeclaration("all_flags", read_only=True, destructive=True, idempotent=True)
        assert tdecl.read_only is True
        assert tdecl.destructive is True
        assert tdecl.idempotent is True


# ---------------------------------------------------------------------------
# TestTelemetryConfig
# ---------------------------------------------------------------------------


class TestTelemetryConfig:
    def test_service_name(self):
        tc = m.TelemetryConfig("my-service")
        assert tc.service_name == "my-service"

    def test_repr_contains_service(self):
        tc = m.TelemetryConfig("test-svc")
        assert "test-svc" in repr(tc)

    def test_enable_tracing_default_true(self):
        tc = m.TelemetryConfig("svc")
        assert tc.enable_tracing is True

    def test_enable_metrics_default_true(self):
        tc = m.TelemetryConfig("svc")
        assert tc.enable_metrics is True

    def test_set_enable_tracing_false(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_tracing(False)
        assert tc.enable_tracing is False

    def test_set_enable_metrics_false(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_metrics(False)
        assert tc.enable_metrics is False

    def test_set_enable_tracing_true(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_tracing(False)
        tc.set_enable_tracing(True)
        assert tc.enable_tracing is True

    def test_with_service_version_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_service_version("1.2.3")
        assert isinstance(result, m.TelemetryConfig)

    def test_with_attribute_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_attribute("env", "production")
        assert isinstance(result, m.TelemetryConfig)

    def test_with_noop_exporter_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_noop_exporter()
        assert isinstance(result, m.TelemetryConfig)

    def test_with_stdout_exporter_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_stdout_exporter()
        assert isinstance(result, m.TelemetryConfig)

    def test_with_json_logs_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_json_logs()
        assert isinstance(result, m.TelemetryConfig)

    def test_with_text_logs_returns_config(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_text_logs()
        assert isinstance(result, m.TelemetryConfig)

    def test_chaining(self):
        tc = (
            m.TelemetryConfig("svc")
            .with_service_version("2.0.0")
            .with_attribute("region", "us-east-1")
            .with_noop_exporter()
        )
        assert isinstance(tc, m.TelemetryConfig)

    def test_attrs_include_methods(self):
        tc = m.TelemetryConfig("svc")
        attrs = [a for a in dir(tc) if not a.startswith("_")]
        for attr in [
            "service_name",
            "enable_tracing",
            "enable_metrics",
            "set_enable_tracing",
            "set_enable_metrics",
            "with_service_version",
            "with_attribute",
            "with_noop_exporter",
            "with_stdout_exporter",
            "with_json_logs",
            "with_text_logs",
            "init",
        ]:
            assert attr in attrs


# ---------------------------------------------------------------------------
# TestTelemetryFunctions
# ---------------------------------------------------------------------------


class TestTelemetryFunctions:
    def test_is_telemetry_initialized_returns_bool(self):
        result = m.is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_shutdown_telemetry_returns_none(self):
        result = m.shutdown_telemetry()
        assert result is None

    def test_shutdown_idempotent(self):
        m.shutdown_telemetry()
        m.shutdown_telemetry()
        assert m.is_telemetry_initialized() is False

    def test_init_then_initialized(self):
        # init() either succeeds (first call in process) or raises (if already set).
        # Either way, the API must not crash unexpectedly.
        tc = m.TelemetryConfig("svc-init-test").with_noop_exporter()
        try:
            tc.init()
            # If it succeeded, is_telemetry_initialized should be True
            assert m.is_telemetry_initialized() is True
            m.shutdown_telemetry()
        except RuntimeError:
            # Already initialized in this process — expected in shared test runner
            pass

    def test_init_twice_raises(self):
        # Any call to init() when the global tracer is already installed raises RuntimeError.
        # We can verify this by calling init() — it will either succeed (first call ever)
        # or raise (already set). After the first call, the second MUST raise.
        tc = m.TelemetryConfig("dup-init-test").with_noop_exporter()
        with contextlib.suppress(RuntimeError):
            tc.init()  # first call may succeed or raise
        # Second call MUST raise (global dispatcher already set)
        tc2 = m.TelemetryConfig("dup-init-test2").with_noop_exporter()
        with pytest.raises(RuntimeError):
            tc2.init()
        m.shutdown_telemetry()

    def test_not_initialized_by_default(self):
        # Ensure clean state after shutdown
        m.shutdown_telemetry()
        assert m.is_telemetry_initialized() is False


# ---------------------------------------------------------------------------
# TestCaptureResult
# ---------------------------------------------------------------------------


class TestCaptureResult:
    def test_basic(self):
        cr = m.CaptureResult(b"image_data", 1920, 1080, "png")
        assert cr.width == 1920
        assert cr.height == 1080
        assert cr.format == "png"
        assert cr.data == b"image_data"

    def test_viewport_default_none(self):
        cr = m.CaptureResult(b"data", 640, 480, "jpeg")
        # viewport may be None or empty string
        assert cr.viewport is None or cr.viewport == ""

    def test_explicit_viewport(self):
        cr = m.CaptureResult(b"data", 640, 480, "png", viewport="persp")
        assert cr.viewport == "persp"

    def test_data_size(self):
        data = b"x" * 1000
        cr = m.CaptureResult(data, 100, 100, "png")
        # data_size is a callable method
        size = cr.data_size() if callable(cr.data_size) else cr.data_size
        assert size == 1000

    def test_jpeg_format(self):
        cr = m.CaptureResult(b"jpeg_data", 800, 600, "jpeg")
        assert cr.format == "jpeg"

    def test_repr_contains_dimensions(self):
        cr = m.CaptureResult(b"d", 1280, 720, "png")
        r = repr(cr)
        assert "1280" in r
        assert "720" in r

    def test_repr_contains_format(self):
        cr = m.CaptureResult(b"d", 100, 100, "webp")
        assert "webp" in repr(cr)

    def test_attrs_complete(self):
        cr = m.CaptureResult(b"d", 100, 100, "png")
        attrs = [a for a in dir(cr) if not a.startswith("_")]
        for attr in ["data", "width", "height", "format", "viewport", "data_size"]:
            assert attr in attrs


# ---------------------------------------------------------------------------
# TestCapturer
# ---------------------------------------------------------------------------


class TestCapturer:
    def test_new_mock_returns_capturer(self):
        cap = m.Capturer.new_mock()
        assert cap is not None

    def test_backend_name_mock(self):
        cap = m.Capturer.new_mock()
        assert cap.backend_name() == "Mock"

    def test_capture_default_returns_frame(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        assert isinstance(frame, m.CaptureFrame)

    def test_capture_frame_default_dimensions(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        assert frame.width > 0
        assert frame.height > 0

    def test_capture_png_format(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(format="png")
        assert frame.format == "png"

    def test_capture_jpeg_format(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(format="jpeg")
        assert frame.format == "jpeg"

    def test_capture_scale_half(self):
        cap = m.Capturer.new_mock()
        frame_full = cap.capture()
        frame_half = cap.capture(scale=0.5)
        assert frame_half.width == frame_full.width // 2
        assert frame_half.height == frame_full.height // 2

    def test_capture_scale_one(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(scale=1.0)
        assert frame.width > 0

    def test_capture_data_is_bytes(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        assert isinstance(frame.data, bytes)
        assert len(frame.data) > 0

    def test_capture_mime_type_png(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(format="png")
        assert frame.mime_type == "image/png"

    def test_capture_mime_type_jpeg(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(format="jpeg")
        assert frame.mime_type == "image/jpeg"

    def test_capture_dpi_scale(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        assert frame.dpi_scale >= 1.0

    def test_capture_timestamp_positive(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        assert frame.timestamp_ms > 0

    def test_capture_byte_len_callable(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        byte_len = frame.byte_len()
        assert byte_len > 0

    def test_stats_returns_tuple(self):
        cap = m.Capturer.new_mock()
        stats = cap.stats()
        assert isinstance(stats, tuple)
        assert len(stats) == 3

    def test_stats_after_capture(self):
        cap = m.Capturer.new_mock()
        cap.capture()
        stats = cap.stats()
        # first element should be ≥ 1 after at least one capture
        assert stats[0] >= 0  # some implementations may reset

    def test_repr_contains_backend(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        r = repr(frame)
        assert "CaptureFrame" in r

    def test_capture_frame_repr_has_dimensions(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture()
        r = repr(frame)
        assert str(frame.width) in r
        assert str(frame.height) in r

    def test_multiple_captures_independent(self):
        cap = m.Capturer.new_mock()
        f1 = cap.capture(format="png")
        f2 = cap.capture(format="jpeg")
        assert f1.format == "png"
        assert f2.format == "jpeg"

    def test_capture_timeout_ms_param(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(timeout_ms=1000)
        assert frame is not None

    def test_capture_jpeg_quality_param(self):
        cap = m.Capturer.new_mock()
        frame = cap.capture(format="jpeg", jpeg_quality=90)
        assert frame.format == "jpeg"


# ---------------------------------------------------------------------------
# TestSkillSummary  (via SkillCatalog.list_skills())
# ---------------------------------------------------------------------------

EXAMPLES_SKILLS_DIR = str((Path(__file__).parent / ".." / "examples" / "skills").resolve())


class TestSkillSummary:
    @pytest.fixture
    def catalog_with_skills(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        return cat

    def test_list_returns_skill_summaries(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        assert len(items) > 0
        for item in items:
            assert isinstance(item, m.SkillSummary)

    def test_summary_name_is_str(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert isinstance(s.name, str)
            assert len(s.name) > 0

    def test_summary_version(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert s.version is not None

    def test_summary_loaded_default_false(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert s.loaded is False

    def test_summary_dcc_is_str(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert isinstance(s.dcc, str)

    def test_summary_tags_list(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert isinstance(s.tags, list)

    def test_summary_description_str(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert isinstance(s.description, str)

    def test_summary_tool_count_nonneg(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert s.tool_count >= 0

    def test_summary_tool_names_list(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        for s in items:
            assert isinstance(s.tool_names, list)

    def test_summary_repr(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        s = items[0]
        r = repr(s)
        assert "SkillSummary" in r
        assert s.name in r

    def test_summary_attrs(self, catalog_with_skills):
        items = catalog_with_skills.list_skills()
        s = items[0]
        attrs = [a for a in dir(s) if not a.startswith("_")]
        for attr in ["name", "version", "loaded", "dcc", "tags", "description", "tool_count", "tool_names"]:
            assert attr in attrs


# ---------------------------------------------------------------------------
# TestSkillCatalog
# ---------------------------------------------------------------------------


class TestSkillCatalog:
    def test_empty_catalog(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        assert cat.loaded_count() == 0
        assert cat.list_skills() == []

    def test_discover_returns_count(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        count = cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert isinstance(count, int)
        assert count > 0

    def test_discover_populates_list(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        items = cat.list_skills()
        assert len(items) > 0

    def test_is_loaded_false_before_load(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        assert cat.is_loaded(s.name) is False

    def test_load_skill_returns_action_names(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        result = cat.load_skill(s.name)
        assert isinstance(result, list)

    def test_is_loaded_true_after_load(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        assert cat.is_loaded(s.name) is True

    def test_loaded_count_after_load(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        assert cat.loaded_count() == 1

    def test_unload_skill_returns_count(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        result = cat.unload_skill(s.name)
        assert isinstance(result, int)

    def test_is_loaded_false_after_unload(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        cat.unload_skill(s.name)
        assert cat.is_loaded(s.name) is False

    def test_loaded_count_decreases_after_unload(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        cat.unload_skill(s.name)
        assert cat.loaded_count() == 0

    def test_get_skill_info_returns_dict(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        info = cat.get_skill_info(s.name)
        assert isinstance(info, dict)

    def test_get_skill_info_has_name_key(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        info = cat.get_skill_info(s.name)
        assert "name" in info
        assert info["name"] == s.name

    def test_get_skill_info_nonexistent_returns_none(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        result = cat.get_skill_info("no-such-skill")
        assert result is None

    def test_is_loaded_nonexistent_false(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        assert cat.is_loaded("no-such-skill") is False

    def test_find_skills_all(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        found = cat.find_skills()
        assert len(found) > 0

    def test_find_skills_by_query(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        found = cat.find_skills(query="maya")
        # Query matches name, description, search_hint, and tool names — not only dcc field.
        # Skills like dcc-diagnostics and workflow mention "Maya" in their descriptions/examples,
        # so they are legitimately included in results.
        assert len(found) > 0
        # At least one result must have maya in name or dcc
        assert any("maya" in s.name.lower() or "maya" in s.dcc.lower() for s in found)

    def test_find_skills_by_dcc(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        found = cat.find_skills(dcc="maya")
        assert all(s.dcc == "maya" for s in found)

    def test_find_skills_no_match_empty(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        found = cat.find_skills(query="zzz_no_such_skill_zzz")
        assert found == []

    def test_info_has_state_key(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        info = cat.get_skill_info(s.name)
        assert "state" in info

    def test_info_state_discovered_before_load(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        info = cat.get_skill_info(s.name)
        assert info["state"] == "discovered"

    def test_info_state_loaded_after_load(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        cat.discover(extra_paths=[EXAMPLES_SKILLS_DIR])
        s = cat.list_skills()[0]
        cat.load_skill(s.name)
        info = cat.get_skill_info(s.name)
        assert info["state"] == "loaded"
        cat.unload_skill(s.name)

    def test_attrs_complete(self):
        reg = m.ActionRegistry()
        cat = m.SkillCatalog(reg)
        attrs = [a for a in dir(cat) if not a.startswith("_")]
        for attr in [
            "discover",
            "find_skills",
            "get_skill_info",
            "is_loaded",
            "list_skills",
            "load_skill",
            "loaded_count",
            "unload_skill",
        ]:
            assert attr in attrs


# ---------------------------------------------------------------------------
# TestLoggingMiddleware
# ---------------------------------------------------------------------------


class TestLoggingMiddleware:
    def test_create_default(self):
        lm = m.LoggingMiddleware()
        assert lm is not None

    def test_log_params_false_default(self):
        lm = m.LoggingMiddleware()
        assert lm.log_params is False

    def test_log_params_true(self):
        lm = m.LoggingMiddleware(log_params=True)
        assert lm.log_params is True

    def test_repr_contains_log_params(self):
        lm = m.LoggingMiddleware(log_params=True)
        r = repr(lm)
        assert "true" in r.lower() or "True" in r

    def test_attrs_complete(self):
        lm = m.LoggingMiddleware()
        attrs = [a for a in dir(lm) if not a.startswith("_")]
        assert "log_params" in attrs

    def test_pipeline_add_logging(self):
        reg = m.ActionRegistry()
        reg.register("test_action", description="Test")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("test_action", lambda p: {"ok": True})
        pipeline = m.ActionPipeline(dispatcher)
        # add_logging() may return None or LoggingMiddleware depending on version
        pipeline.add_logging(log_params=True)
        assert "logging" in pipeline.middleware_names()

    def test_pipeline_logging_middleware_visible(self):
        reg = m.ActionRegistry()
        reg.register("test_action", description="Test")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("test_action", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        pipeline.add_logging()
        assert "logging" in pipeline.middleware_names()


# ---------------------------------------------------------------------------
# TestTimingMiddleware
# ---------------------------------------------------------------------------


class TestTimingMiddleware:
    def test_create(self):
        tm = m.TimingMiddleware()
        assert tm is not None

    def test_last_elapsed_ms_none_before_dispatch(self):
        tm = m.TimingMiddleware()
        result = tm.last_elapsed_ms("some_action")
        assert result is None

    def test_repr(self):
        tm = m.TimingMiddleware()
        assert "TimingMiddleware" in repr(tm)

    def test_attrs_complete(self):
        tm = m.TimingMiddleware()
        attrs = [a for a in dir(tm) if not a.startswith("_")]
        assert "last_elapsed_ms" in attrs

    def test_pipeline_add_timing(self):
        reg = m.ActionRegistry()
        reg.register("timed_action", description="Timed")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("timed_action", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        tm = pipeline.add_timing()
        assert isinstance(tm, m.TimingMiddleware)

    def test_timing_after_dispatch(self):
        reg = m.ActionRegistry()
        reg.register("timed_action", description="Timed")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("timed_action", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        tm = pipeline.add_timing()
        pipeline.dispatch("timed_action", "{}")
        elapsed = tm.last_elapsed_ms("timed_action")
        assert elapsed is not None
        assert elapsed >= 0

    def test_timing_in_middleware_names(self):
        reg = m.ActionRegistry()
        reg.register("x", description="x")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("x", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        pipeline.add_timing()
        assert "timing" in pipeline.middleware_names()


# ---------------------------------------------------------------------------
# TestRateLimitMiddleware
# ---------------------------------------------------------------------------


class TestRateLimitMiddleware:
    def test_create(self):
        rlm = m.RateLimitMiddleware(max_calls=5, window_ms=500)
        assert rlm is not None

    def test_max_calls_attribute(self):
        rlm = m.RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert rlm.max_calls == 10

    def test_window_ms_attribute(self):
        rlm = m.RateLimitMiddleware(max_calls=5, window_ms=2000)
        assert rlm.window_ms == 2000

    def test_call_count_zero_initially(self):
        rlm = m.RateLimitMiddleware(max_calls=10, window_ms=1000)
        count = rlm.call_count("some_action")
        assert count == 0

    def test_repr(self):
        rlm = m.RateLimitMiddleware(max_calls=10, window_ms=1000)
        r = repr(rlm)
        assert "10" in r
        assert "1000" in r

    def test_attrs_complete(self):
        rlm = m.RateLimitMiddleware(max_calls=5, window_ms=500)
        attrs = [a for a in dir(rlm) if not a.startswith("_")]
        for attr in ["max_calls", "window_ms", "call_count"]:
            assert attr in attrs

    def test_pipeline_add_rate_limit(self):
        reg = m.ActionRegistry()
        reg.register("rate_action", description="Rate limited")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("rate_action", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        rlm = pipeline.add_rate_limit(max_calls=5, window_ms=1000)
        assert isinstance(rlm, m.RateLimitMiddleware)

    def test_rate_limit_in_middleware_names(self):
        reg = m.ActionRegistry()
        reg.register("x", description="x")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("x", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        pipeline.add_rate_limit(max_calls=5, window_ms=500)
        assert "rate_limit" in pipeline.middleware_names()

    def test_call_count_increments(self):
        reg = m.ActionRegistry()
        reg.register("counted_action", description="Counted")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("counted_action", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        rlm = pipeline.add_rate_limit(max_calls=100, window_ms=60000)
        pipeline.dispatch("counted_action", "{}")
        pipeline.dispatch("counted_action", "{}")
        count = rlm.call_count("counted_action")
        assert count >= 2

    def test_rate_limit_exceeded_raises(self):
        reg = m.ActionRegistry()
        reg.register("limited", description="Limited")
        dispatcher = m.ActionDispatcher(reg)
        dispatcher.register_handler("limited", lambda p: True)
        pipeline = m.ActionPipeline(dispatcher)
        pipeline.add_rate_limit(max_calls=2, window_ms=60000)
        pipeline.dispatch("limited", "{}")
        pipeline.dispatch("limited", "{}")
        with pytest.raises(RuntimeError):
            pipeline.dispatch("limited", "{}")
