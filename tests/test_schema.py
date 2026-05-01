"""Tests for dcc_mcp_core.schema — zero-dep type → JSON Schema derivation (#242)."""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import datetime
import enum
from pathlib import Path
from typing import Any
from typing import Literal
from typing import TypedDict
from typing import Union
import uuid

import pytest

from dcc_mcp_core.schema import derive_parameters_schema
from dcc_mcp_core.schema import derive_schema
from dcc_mcp_core.schema import schema_from_doc

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
        assert derive_schema(list[str]) == {
            "type": "array",
            "items": {"type": "string"},
        }

    def test_homogeneous_tuple(self) -> None:
        assert derive_schema(tuple[int, ...]) == {
            "type": "array",
            "items": {"type": "integer"},
        }

    def test_fixed_tuple(self) -> None:
        assert derive_schema(tuple[int, str, bool]) == {
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
        assert derive_schema(dict[str, int]) == {
            "type": "object",
            "additionalProperties": {"type": "integer"},
        }

    def test_nested_list_of_list(self) -> None:
        assert derive_schema(list[list[int]]) == {
            "type": "array",
            "items": {
                "type": "array",
                "items": {"type": "integer"},
            },
        }


# ── Optional / Union / Literal / Enum ──────────────────────────────────────


class TestUnionAndLiterals:
    def test_optional_unwraps_to_anyof_with_null(self) -> None:
        assert derive_schema(int | None) == {
            "anyOf": [{"type": "integer"}, {"type": "null"}],
        }

    def test_pep604_union_none(self) -> None:
        # Same as above, kept as a separate assertion to spell out the intent.
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
    label: str | None = None


@dataclass
class Polygon:
    name: str
    vertices: list[Point]
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
            tags: list[str] = field(default_factory=list)

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
        def fn(x: int, y: int | None = None) -> None: ...

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
        schema = derive_schema(Point | None)
        assert schema["anyOf"][1] == {"type": "null"}
        # First branch is the Point object schema.
        first = schema["anyOf"][0]
        assert first.get("$ref") == "#/$defs/Point" or first.get("title") == "Point"
