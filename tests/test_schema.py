"""Tests for dcc_mcp_core.schema — zero-dep type → JSON Schema derivation (#242)."""

# ruff: noqa: UP006, UP045

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import datetime
import enum
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Literal
from typing import Optional
from typing import Tuple
from typing import TypedDict
from typing import Union
import uuid

import pytest

from dcc_mcp_core.schema import derive_parameters_schema
from dcc_mcp_core.schema import derive_schema
from dcc_mcp_core.schema import schema_from_doc
from dcc_mcp_core.schema import tool_spec_from_callable

# ── Primitives ─────────────────────────────────────────────────────────────


class TestPrimitiveTypes:
    def test_bool(self) -> None:
        assert derive_schema(bool) == {"type": "boolean"}

    def test_int(self) -> None:
        assert derive_schema(int) == {"type": "integer"}

    def test_float(self) -> None:
        assert derive_schema(float) == {"type": "number"}

    def test_str(self) -> None:
        assert derive_schema(str) == {"type": "string"}

    def test_bytes_becomes_base64_string(self) -> None:
        assert derive_schema(bytes) == {
            "type": "string",
            "contentEncoding": "base64",
        }

    def test_none_type(self) -> None:
        assert derive_schema(type(None)) == {"type": "null"}

    def test_datetime(self) -> None:
        assert derive_schema(datetime.datetime) == {
            "type": "string",
            "format": "date-time",
        }

    def test_date(self) -> None:
        assert derive_schema(datetime.date) == {"type": "string", "format": "date"}

    def test_pathlib_path(self) -> None:
        assert derive_schema(Path) == {"type": "string"}

    def test_uuid(self) -> None:
        assert derive_schema(uuid.UUID) == {"type": "string", "format": "uuid"}

    def test_any(self) -> None:
        assert derive_schema(Any) == {}


# ── Containers ─────────────────────────────────────────────────────────────


class TestContainers:
    def test_list_of_str(self) -> None:
        assert derive_schema(List[str]) == {
            "type": "array",
            "items": {"type": "string"},
        }

    def test_homogeneous_tuple(self) -> None:
        assert derive_schema(Tuple[int, ...]) == {
            "type": "array",
            "items": {"type": "integer"},
        }

    def test_fixed_tuple(self) -> None:
        assert derive_schema(Tuple[int, str, bool]) == {
            "type": "array",
            "prefixItems": [
                {"type": "integer"},
                {"type": "string"},
                {"type": "boolean"},
            ],
            "minItems": 3,
            "maxItems": 3,
        }

    def test_dict_str_v(self) -> None:
        assert derive_schema(Dict[str, int]) == {
            "type": "object",
            "additionalProperties": {"type": "integer"},
        }

    def test_nested_list_of_list(self) -> None:
        assert derive_schema(List[List[int]]) == {
            "type": "array",
            "items": {
                "type": "array",
                "items": {"type": "integer"},
            },
        }


# ── Optional / Union / Literal / Enum ──────────────────────────────────────


class TestUnionAndLiterals:
    def test_optional_unwraps_to_anyof_with_null(self) -> None:
        assert derive_schema(Optional[int]) == {
            "anyOf": [{"type": "integer"}, {"type": "null"}],
        }

    def test_pep604_union_none(self) -> None:
        if sys.version_info < (3, 10):
            pytest.skip("PEP 604 unions require Python 3.10+")
        assert derive_schema(int | None) == {
            "anyOf": [{"type": "integer"}, {"type": "null"}],
        }

    def test_union_non_optional(self) -> None:
        assert derive_schema(Union[int, str]) == {
            "anyOf": [{"type": "integer"}, {"type": "string"}],
        }

    def test_literal_str(self) -> None:
        assert derive_schema(Literal["fbx", "abc", "usd"]) == {
            "enum": ["fbx", "abc", "usd"],
            "type": "string",
        }

    def test_literal_int(self) -> None:
        assert derive_schema(Literal[1, 2, 3]) == {
            "enum": [1, 2, 3],
            "type": "integer",
        }

    def test_literal_mixed_types(self) -> None:
        result = derive_schema(Literal["on", 1, True])
        assert result["enum"] == ["on", 1, True]
        assert "type" not in result  # mixed types → no `type` pin

    def test_string_enum(self) -> None:
        class Colour(enum.Enum):
            RED = "red"
            GREEN = "green"
            BLUE = "blue"

        schema = derive_schema(Colour)
        assert schema["enum"] == ["red", "green", "blue"]
        assert schema["type"] == "string"
        assert schema["title"] == "Colour"

    def test_int_enum(self) -> None:
        class Status(enum.IntEnum):
            OK = 0
            ERROR = 1

        schema = derive_schema(Status)
        assert schema["enum"] == [0, 1]
        assert schema["type"] == "integer"


# ── Dataclasses ────────────────────────────────────────────────────────────


@dataclass
class Point:
    x: float
    y: float
    label: Optional[str] = None


@dataclass
class Polygon:
    name: str
    vertices: List[Point]
    closed: bool = True


class TestDataclasses:
    def test_simple_dataclass(self) -> None:
        schema = derive_schema(Point)
        # Top-level is flattened (no $ref).
        assert schema["type"] == "object"
        assert schema["title"] == "Point"
        assert schema["additionalProperties"] is False
        assert schema["$schema"].endswith("draft/2020-12/schema")

        # Required fields: only those without defaults.
        assert set(schema["required"]) == {"x", "y"}

        # Field types.
        props = schema["properties"]
        assert props["x"] == {"type": "number"}
        assert props["y"] == {"type": "number"}
        # Optional[str] → anyOf([string, null])
        assert props["label"] == {"anyOf": [{"type": "string"}, {"type": "null"}]}

    def test_nested_dataclass_uses_defs(self) -> None:
        schema = derive_schema(Polygon)
        assert schema["title"] == "Polygon"
        # The nested Point lives in $defs and is $ref'd.
        assert "$defs" in schema
        assert "Point" in schema["$defs"]
        assert schema["properties"]["vertices"] == {
            "type": "array",
            "items": {"$ref": "#/$defs/Point"},
        }
        assert set(schema["required"]) == {"name", "vertices"}

    def test_field_metadata_description(self) -> None:
        @dataclass
        class Input:
            radius: float = field(metadata={"description": "Sphere radius in cm"})

        schema = derive_schema(Input)
        assert schema["properties"]["radius"] == {
            "type": "number",
            "description": "Sphere radius in cm",
        }

    def test_dataclass_with_default_factory_is_optional(self) -> None:
        @dataclass
        class WithList:
            name: str
            tags: List[str] = field(default_factory=list)

        schema = derive_schema(WithList)
        assert schema["required"] == ["name"]

    def test_additional_properties_opt_in(self) -> None:
        schema_strict = derive_schema(Point)
        assert schema_strict["additionalProperties"] is False

        schema_open = derive_schema(Point, allow_additional=True)
        assert schema_open["additionalProperties"] is True


# ── TypedDict ──────────────────────────────────────────────────────────────


class MovieTD(TypedDict):
    title: str
    year: int


class MovieTDWithOptional(TypedDict, total=False):
    title: str  # overridden below
    director: str


class TestTypedDict:
    def test_all_required(self) -> None:
        schema = derive_schema(MovieTD)
        assert schema["type"] == "object"
        assert schema["title"] == "MovieTD"
        assert schema["properties"] == {
            "title": {"type": "string"},
            "year": {"type": "integer"},
        }
        assert set(schema["required"]) == {"title", "year"}

    def test_total_false_means_none_required(self) -> None:
        schema = derive_schema(MovieTDWithOptional)
        assert "required" not in schema


# ── Unsupported types raise ────────────────────────────────────────────────


class TestUnsupported:
    def test_plain_class_raises(self) -> None:
        class Plain:
            pass

        with pytest.raises(TypeError, match="unsupported type"):
            derive_schema(Plain)

    def test_error_mentions_escape_hatch(self) -> None:
        class Plain:
            pass

        with pytest.raises(TypeError, match="explicit input_schema"):
            derive_schema(Plain)


# ── derive_parameters_schema ───────────────────────────────────────────────


class TestDeriveParametersSchema:
    def test_single_typed_param(self) -> None:
        def fn(name: str) -> None: ...

        schema = derive_parameters_schema(fn)
        assert schema["type"] == "object"
        assert schema["properties"] == {"name": {"type": "string"}}
        assert schema["required"] == ["name"]
        assert schema["additionalProperties"] is False

    def test_multi_params_with_defaults(self) -> None:
        def fn(radius: float, segments: int = 16, label: str = "sphere") -> None: ...

        schema = derive_parameters_schema(fn)
        assert set(schema["properties"].keys()) == {"radius", "segments", "label"}
        assert schema["required"] == ["radius"]

    def test_optional_param_is_not_required(self) -> None:
        def fn(x: int, y: Optional[int] = None) -> None: ...

        schema = derive_parameters_schema(fn)
        assert schema["required"] == ["x"]
        assert schema["properties"]["y"] == {
            "anyOf": [{"type": "integer"}, {"type": "null"}],
        }

    def test_skips_var_args(self) -> None:
        def fn(x: int, *args: int, **kwargs: str) -> None: ...

        schema = derive_parameters_schema(fn)
        assert set(schema["properties"].keys()) == {"x"}

    def test_skips_self(self) -> None:
        class _Holder:
            def method(self, x: int) -> None: ...

        schema = derive_parameters_schema(_Holder.method)
        assert set(schema["properties"].keys()) == {"x"}

    def test_untyped_param_raises(self) -> None:
        def fn(x, y: int) -> None: ...

        with pytest.raises(TypeError, match="untyped parameters"):
            derive_parameters_schema(fn)

    def test_numpy_style_docstring_attaches_descriptions(self) -> None:
        def fn(radius: float, segments: int = 16) -> None:
            """Build a sphere.

            Parameters
            ----------
            radius : float
                Radius of the sphere in cm.
            segments : int
                Number of latitude segments.

            """

        schema = derive_parameters_schema(fn)
        assert schema["properties"]["radius"]["description"] == "Radius of the sphere in cm."
        assert schema["properties"]["segments"]["description"] == "Number of latitude segments."

    def test_google_style_docstring(self) -> None:
        def fn(scene_path: str, format: str = "fbx") -> None:
            """Export a scene.

            Args:
                scene_path: Absolute path to the scene file.
                format: One of fbx/abc/usd.

            """

        schema = derive_parameters_schema(fn)
        assert schema["properties"]["scene_path"]["description"] == ("Absolute path to the scene file.")
        assert schema["properties"]["format"]["description"] == "One of fbx/abc/usd."


# ── schema_from_doc ────────────────────────────────────────────────────────


class TestSchemaFromDoc:
    def test_empty_docstring(self) -> None:
        def fn() -> None: ...

        assert schema_from_doc(fn) == {}

    def test_no_parameters_section(self) -> None:
        def fn() -> None:
            """Just a description."""

        assert schema_from_doc(fn) == {}

    def test_numpy_wrapped_description(self) -> None:
        def fn() -> None:
            """X.

            Parameters
            ----------
            foo : int
                description that
                continues on the next line

            """

        result = schema_from_doc(fn)
        assert result["foo"] == "description that continues on the next line"


# ── Forward/cyclic ─────────────────────────────────────────────────────────


class TestEdgeCases:
    def test_optional_dataclass(self) -> None:
        schema = derive_schema(Optional[Point])
        assert schema["anyOf"][1] == {"type": "null"}
        # First branch is the Point object schema.
        first = schema["anyOf"][0]
        assert first.get("$ref") == "#/$defs/Point" or first.get("title") == "Point"


# ── tool_spec_from_callable ────────────────────────────────────────────────


@dataclass
class _ExportInput:
    scene_path: str
    format: Literal["fbx", "abc", "usd"] = "fbx"


@dataclass
class _ExportResult:
    path: str
    size_bytes: int


class _NotAThing: ...


class TestToolSpecFromCallable:
    def test_single_dataclass_param(self) -> None:
        def export_scene(args: _ExportInput) -> _ExportResult:
            """Export a scene to an interchange format."""
            return _ExportResult(path=args.scene_path, size_bytes=0)

        spec = tool_spec_from_callable(export_scene)

        assert spec.name == "export_scene"
        assert spec.description == "Export a scene to an interchange format."
        # Single-dataclass style: inputSchema IS the dataclass schema.
        assert spec.input_schema["type"] == "object"
        assert spec.input_schema["title"] == "_ExportInput"
        assert "scene_path" in spec.input_schema["properties"]
        # outputSchema derived from return annotation.
        assert spec.output_schema is not None
        assert spec.output_schema["title"] == "_ExportResult"

    def test_multi_primitive_params(self) -> None:
        def make_sphere(radius: float, segments: int = 16) -> Dict[str, int]:
            return {"radius": int(radius), "segments": segments}

        spec = tool_spec_from_callable(make_sphere)

        # Multi-primitive style: inputSchema is an object with each param as a property.
        assert spec.input_schema["type"] == "object"
        assert set(spec.input_schema["properties"]) == {"radius", "segments"}
        assert spec.input_schema["required"] == ["radius"]
        # Return type Dict[str, int] → outputSchema is an object with additionalProperties.
        assert spec.output_schema == {
            "type": "object",
            "additionalProperties": {"type": "integer"},
        }

    def test_refuses_untyped_handler(self) -> None:
        def fn(x, y): ...  # no annotations

        with pytest.raises(TypeError, match="untyped parameters"):
            tool_spec_from_callable(fn)

    def test_no_return_annotation_leaves_output_schema_unset(self) -> None:
        def fn(x: int) -> None: ...

        spec = tool_spec_from_callable(fn)
        assert spec.output_schema is None

    def test_unsupported_return_type_silently_drops_output_schema(self) -> None:
        def fn(x: int) -> _NotAThing:
            raise NotImplementedError

        spec = tool_spec_from_callable(fn)
        # Input schema still derived from the typed parameter.
        assert spec.input_schema["properties"] == {"x": {"type": "integer"}}
        # Unsupported return → outputSchema left out, but registration still works.
        assert spec.output_schema is None

    def test_name_and_description_overrides(self) -> None:
        def fn(x: int) -> None: ...

        spec = tool_spec_from_callable(
            fn,
            name="custom.name",
            description="custom desc",
            category="custom",
            version="2.0.0",
        )
        assert spec.name == "custom.name"
        assert spec.description == "custom desc"
        assert spec.category == "custom"
        assert spec.version == "2.0.0"


def test_typed_schema_demo_example_imports_cleanly() -> None:
    """Protect examples/skills/typed-schema-demo from bitrot.

    We import the demo module by path (directory name has a hyphen so it's
    not a valid Python package name) and assert that its derived ``spec``
    has the expected schema fields.
    """
    import importlib.util
    from pathlib import Path
    import sys

    repo_root = Path(__file__).resolve().parent.parent
    demo_py = repo_root / "examples" / "skills" / "typed-schema-demo" / "scripts" / "demo.py"
    assert demo_py.is_file(), f"demo script missing at {demo_py}"

    mod_name = "_typed_schema_demo"
    spec = importlib.util.spec_from_file_location(mod_name, demo_py)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # dataclasses with ``from __future__ import annotations`` need the module
    # present in sys.modules so ``typing.get_type_hints`` can resolve string
    # annotations against the module's namespace.
    sys.modules[mod_name] = module
    try:
        spec.loader.exec_module(module)
    finally:
        sys.modules.pop(mod_name, None)

    tool_spec = module.spec
    assert tool_spec.name == "typed_schema_demo__export"
    assert tool_spec.input_schema["type"] == "object"
    assert "scene_path" in tool_spec.input_schema["properties"]
    assert tool_spec.output_schema is not None
    assert tool_spec.output_schema["title"] == "ExportResult"
