"""Deep tests for ResourceDefinition, ResourceTemplateDefinition, PromptDefinition.

PySceneDataKind enum, and ScriptLanguage/DccErrorCode enum depth.

Covers:
- ResourceDefinition: uri, name, description, mime_type, annotations fields
- ResourceDefinition default mime_type is "text/plain"
- ResourceTemplateDefinition: uri_template field vs uri
- PromptDefinition: name, description, arguments list
- PromptArgument: name, description, required flag
- PySceneDataKind: all 4 enum values, repr, equality
- ScriptLanguage: all 8 enum values, repr, equality
- DccErrorCode: all 9 enum values, repr, equality
- ResourceAnnotations: audience, priority fields
- CaptureResult: data, width, height, format, viewport, data_size()
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import CaptureResult
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import PromptArgument
from dcc_mcp_core import PromptDefinition
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import ResourceAnnotations
from dcc_mcp_core import ResourceDefinition
from dcc_mcp_core import ResourceTemplateDefinition
from dcc_mcp_core import ScriptLanguage

# ---------------------------------------------------------------------------
# ResourceAnnotations
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    def test_default_audience_is_empty(self):
        ann = ResourceAnnotations()
        assert ann.audience == []

    def test_default_priority_is_none(self):
        ann = ResourceAnnotations()
        assert ann.priority is None

    def test_set_audience(self):
        ann = ResourceAnnotations(audience=["user", "assistant"])
        assert "user" in ann.audience
        assert "assistant" in ann.audience

    def test_set_priority_float(self):
        ann = ResourceAnnotations(priority=0.75)
        assert abs(ann.priority - 0.75) < 1e-6

    def test_repr_contains_class_name(self):
        ann = ResourceAnnotations(audience=["user"])
        r = repr(ann)
        assert "ResourceAnnotations" in r or "resource" in r.lower()


# ---------------------------------------------------------------------------
# ResourceDefinition
# ---------------------------------------------------------------------------


class TestResourceDefinition:
    def test_basic_construction(self):
        rd = ResourceDefinition(
            uri="file:///scene.usda",
            name="scene",
            description="USD scene file",
        )
        assert rd.uri == "file:///scene.usda"
        assert rd.name == "scene"
        assert rd.description == "USD scene file"

    def test_default_mime_type_is_text_plain(self):
        rd = ResourceDefinition(uri="x://y", name="n", description="d")
        assert rd.mime_type == "text/plain"

    def test_custom_mime_type(self):
        rd = ResourceDefinition(
            uri="x://y",
            name="n",
            description="d",
            mime_type="application/json",
        )
        assert rd.mime_type == "application/json"

    def test_annotations_none_by_default(self):
        rd = ResourceDefinition(uri="x://y", name="n", description="d")
        assert rd.annotations is None

    def test_with_annotations(self):
        ann = ResourceAnnotations(audience=["user"], priority=1.0)
        rd = ResourceDefinition(
            uri="x://y",
            name="n",
            description="d",
            annotations=ann,
        )
        assert rd.annotations is not None
        assert "user" in rd.annotations.audience

    def test_repr_contains_uri(self):
        rd = ResourceDefinition(uri="test://resource", name="r", description="d")
        r = repr(rd)
        assert "ResourceDefinition" in r or "test://resource" in r

    def test_usd_mime_type(self):
        rd = ResourceDefinition(
            uri="usda:///scene.usda",
            name="scene",
            description="A USD scene",
            mime_type="model/vnd.usd",
        )
        assert "usd" in rd.mime_type.lower()

    def test_image_mime_type(self):
        rd = ResourceDefinition(
            uri="viewport://main",
            name="viewport",
            description="Viewport screenshot",
            mime_type="image/png",
        )
        assert rd.mime_type == "image/png"


# ---------------------------------------------------------------------------
# ResourceTemplateDefinition
# ---------------------------------------------------------------------------


class TestResourceTemplateDefinition:
    def test_uri_template_field(self):
        rtd = ResourceTemplateDefinition(
            uri_template="file:///scenes/{name}.usda",
            name="scene-template",
            description="Scene template",
        )
        assert rtd.uri_template == "file:///scenes/{name}.usda"

    def test_name_and_description(self):
        rtd = ResourceTemplateDefinition(
            uri_template="x://{id}",
            name="my-template",
            description="A template for resources",
        )
        assert rtd.name == "my-template"
        assert rtd.description == "A template for resources"

    def test_default_mime_type(self):
        rtd = ResourceTemplateDefinition(
            uri_template="x://{id}",
            name="t",
            description="d",
        )
        assert rtd.mime_type == "text/plain"

    def test_custom_mime_type(self):
        rtd = ResourceTemplateDefinition(
            uri_template="x://{id}",
            name="t",
            description="d",
            mime_type="application/octet-stream",
        )
        assert rtd.mime_type == "application/octet-stream"

    def test_annotations_none_by_default(self):
        rtd = ResourceTemplateDefinition(uri_template="x://{id}", name="t", description="d")
        assert rtd.annotations is None

    def test_with_annotations(self):
        ann = ResourceAnnotations(priority=0.5)
        rtd = ResourceTemplateDefinition(
            uri_template="x://{id}",
            name="t",
            description="d",
            annotations=ann,
        )
        assert rtd.annotations is not None
        assert abs(rtd.annotations.priority - 0.5) < 1e-6

    def test_repr_contains_class(self):
        rtd = ResourceTemplateDefinition(uri_template="x://{id}", name="t", description="d")
        r = repr(rtd)
        assert "ResourceTemplateDefinition" in r or "Template" in r

    def test_uri_template_uses_curly_braces(self):
        rtd = ResourceTemplateDefinition(
            uri_template="dcc://maya/{scene_name}/objects/{object_id}",
            name="maya-object",
            description="Maya scene object template",
        )
        assert "{scene_name}" in rtd.uri_template
        assert "{object_id}" in rtd.uri_template


# ---------------------------------------------------------------------------
# PromptArgument
# ---------------------------------------------------------------------------


class TestPromptArgument:
    def test_basic_construction(self):
        arg = PromptArgument(name="scene_path", description="Path to the scene file")
        assert arg.name == "scene_path"
        assert arg.description == "Path to the scene file"

    def test_required_default_false(self):
        arg = PromptArgument(name="opt", description="optional arg")
        assert arg.required is False

    def test_required_true(self):
        arg = PromptArgument(name="mandatory", description="must provide", required=True)
        assert arg.required is True

    def test_repr(self):
        arg = PromptArgument(name="x", description="d")
        r = repr(arg)
        assert "PromptArgument" in r or "x" in r

    def test_equality(self):
        a1 = PromptArgument(name="x", description="d", required=True)
        a2 = PromptArgument(name="x", description="d", required=True)
        assert a1 == a2

    def test_inequality_different_name(self):
        a1 = PromptArgument(name="a", description="d")
        a2 = PromptArgument(name="b", description="d")
        assert a1 != a2


# ---------------------------------------------------------------------------
# PromptDefinition
# ---------------------------------------------------------------------------


class TestPromptDefinition:
    def test_basic_construction_no_args(self):
        pd = PromptDefinition(name="summarize", description="Summarize a scene")
        assert pd.name == "summarize"
        assert pd.description == "Summarize a scene"
        assert pd.arguments == []

    def test_with_arguments(self):
        args = [
            PromptArgument(name="scene_path", description="Scene path", required=True),
            PromptArgument(name="detail_level", description="Detail level"),
        ]
        pd = PromptDefinition(name="describe_scene", description="Describe a DCC scene", arguments=args)
        assert len(pd.arguments) == 2
        assert pd.arguments[0].name == "scene_path"
        assert pd.arguments[0].required is True

    def test_repr_contains_name(self):
        pd = PromptDefinition(name="my_prompt", description="d")
        r = repr(pd)
        assert "PromptDefinition" in r or "my_prompt" in r

    def test_equality(self):
        p1 = PromptDefinition(name="x", description="d")
        p2 = PromptDefinition(name="x", description="d")
        assert p1 == p2

    def test_inequality(self):
        p1 = PromptDefinition(name="a", description="d")
        p2 = PromptDefinition(name="b", description="d")
        assert p1 != p2

    def test_all_required_arguments(self):
        args = [PromptArgument(name=f"arg{i}", description=f"Arg {i}", required=True) for i in range(3)]
        pd = PromptDefinition(name="all_required", description="All args required", arguments=args)
        assert all(a.required for a in pd.arguments)


# ---------------------------------------------------------------------------
# PySceneDataKind enum depth
# ---------------------------------------------------------------------------


class TestPySceneDataKind:
    def test_geometry_exists(self):
        k = PySceneDataKind.Geometry
        assert k is not None

    def test_animation_cache_exists(self):
        k = PySceneDataKind.AnimationCache
        assert k is not None

    def test_screenshot_exists(self):
        k = PySceneDataKind.Screenshot
        assert k is not None

    def test_arbitrary_exists(self):
        k = PySceneDataKind.Arbitrary
        assert k is not None

    def test_all_four_are_distinct(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]
        # Each must be different from the others
        for i, a in enumerate(kinds):
            for j, b in enumerate(kinds):
                if i != j:
                    assert a != b, f"kinds[{i}] == kinds[{j}] unexpectedly"

    def test_equality_self(self):
        assert PySceneDataKind.Geometry == PySceneDataKind.Geometry
        assert PySceneDataKind.AnimationCache == PySceneDataKind.AnimationCache
        assert PySceneDataKind.Screenshot == PySceneDataKind.Screenshot
        assert PySceneDataKind.Arbitrary == PySceneDataKind.Arbitrary

    def test_inequality_between_variants(self):
        assert PySceneDataKind.Geometry != PySceneDataKind.Screenshot
        assert PySceneDataKind.AnimationCache != PySceneDataKind.Arbitrary

    def test_repr_not_empty(self):
        r = repr(PySceneDataKind.Geometry)
        assert len(r) > 0


# ---------------------------------------------------------------------------
# ScriptLanguage enum depth
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    def test_python_exists(self):
        assert ScriptLanguage.PYTHON is not None

    def test_mel_exists(self):
        assert ScriptLanguage.MEL is not None

    def test_maxscript_exists(self):
        assert ScriptLanguage.MAXSCRIPT is not None

    def test_hscript_exists(self):
        assert ScriptLanguage.HSCRIPT is not None

    def test_vex_exists(self):
        assert ScriptLanguage.VEX is not None

    def test_lua_exists(self):
        assert ScriptLanguage.LUA is not None

    def test_csharp_exists(self):
        assert ScriptLanguage.CSHARP is not None

    def test_blueprint_exists(self):
        assert ScriptLanguage.BLUEPRINT is not None

    def test_all_eight_are_distinct(self):
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
        for i, a in enumerate(langs):
            for j, b in enumerate(langs):
                if i != j:
                    assert a != b, f"langs[{i}] == langs[{j}] unexpectedly"

    def test_self_equality(self):
        assert ScriptLanguage.PYTHON == ScriptLanguage.PYTHON
        assert ScriptLanguage.MEL == ScriptLanguage.MEL

    def test_inequality_across_variants(self):
        assert ScriptLanguage.PYTHON != ScriptLanguage.MEL
        assert ScriptLanguage.LUA != ScriptLanguage.CSHARP

    def test_repr_non_empty(self):
        r = repr(ScriptLanguage.PYTHON)
        assert len(r) > 0

    def test_str_non_empty(self):
        s = str(ScriptLanguage.PYTHON)
        assert len(s) > 0


# ---------------------------------------------------------------------------
# DccErrorCode enum depth
# ---------------------------------------------------------------------------


class TestDccErrorCode:
    def test_connection_failed_exists(self):
        assert DccErrorCode.CONNECTION_FAILED is not None

    def test_timeout_exists(self):
        assert DccErrorCode.TIMEOUT is not None

    def test_script_error_exists(self):
        assert DccErrorCode.SCRIPT_ERROR is not None

    def test_not_responding_exists(self):
        assert DccErrorCode.NOT_RESPONDING is not None

    def test_unsupported_exists(self):
        assert DccErrorCode.UNSUPPORTED is not None

    def test_permission_denied_exists(self):
        assert DccErrorCode.PERMISSION_DENIED is not None

    def test_invalid_input_exists(self):
        assert DccErrorCode.INVALID_INPUT is not None

    def test_scene_error_exists(self):
        assert DccErrorCode.SCENE_ERROR is not None

    def test_internal_exists(self):
        assert DccErrorCode.INTERNAL is not None

    def test_all_nine_are_distinct(self):
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
        for i, a in enumerate(codes):
            for j, b in enumerate(codes):
                if i != j:
                    assert a != b, f"codes[{i}] == codes[{j}] unexpectedly"

    def test_self_equality(self):
        assert DccErrorCode.TIMEOUT == DccErrorCode.TIMEOUT
        assert DccErrorCode.INTERNAL == DccErrorCode.INTERNAL

    def test_inequality_across_variants(self):
        assert DccErrorCode.TIMEOUT != DccErrorCode.INTERNAL
        assert DccErrorCode.CONNECTION_FAILED != DccErrorCode.SCRIPT_ERROR

    def test_repr_non_empty(self):
        r = repr(DccErrorCode.CONNECTION_FAILED)
        assert len(r) > 0

    def test_str_non_empty(self):
        s = str(DccErrorCode.TIMEOUT)
        assert len(s) > 0


# ---------------------------------------------------------------------------
# CaptureResult depth
# ---------------------------------------------------------------------------


class TestCaptureResult:
    def _make_result(
        self,
        data: bytes = b"\x89PNG\r\n\x1a\nfakedata",
        width: int = 1920,
        height: int = 1080,
        fmt: str = "png",
        viewport: str | None = None,
    ) -> CaptureResult:
        return CaptureResult(
            data=data,
            width=width,
            height=height,
            format=fmt,
            viewport=viewport,
        )

    def test_data_attribute(self):
        data = b"image_bytes"
        cr = self._make_result(data=data)
        assert cr.data == data

    def test_width_attribute(self):
        cr = self._make_result(width=3840)
        assert cr.width == 3840

    def test_height_attribute(self):
        cr = self._make_result(height=2160)
        assert cr.height == 2160

    def test_format_attribute(self):
        cr = self._make_result(fmt="jpeg")
        assert cr.format == "jpeg"

    def test_viewport_none_by_default(self):
        cr = self._make_result()
        assert cr.viewport is None

    def test_viewport_set(self):
        cr = self._make_result(viewport="persp")
        assert cr.viewport == "persp"

    def test_data_size_equals_len_data(self):
        data = b"abcdefghij"
        cr = self._make_result(data=data)
        assert cr.data_size() == len(data)

    def test_data_size_zero_for_empty(self):
        cr = self._make_result(data=b"")
        assert cr.data_size() == 0

    def test_repr_non_empty(self):
        cr = self._make_result()
        r = repr(cr)
        assert len(r) > 0
        assert "CaptureResult" in r or "1920" in r

    def test_png_format_round_trip(self):
        data = bytes(range(256))
        cr = CaptureResult(data=data, width=16, height=16, format="png")
        assert cr.format == "png"
        assert cr.data == data
        assert cr.data_size() == 256
