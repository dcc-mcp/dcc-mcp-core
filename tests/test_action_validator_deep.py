"""Tests for ActionValidator deep validation coverage.

Covers: from_schema_json / from_action_registry / validate for required fields,
type checking, multi-field schemas, and error messages.
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import ActionValidator


def _schema(**properties) -> str:
    """Build a simple JSON schema string."""
    return json.dumps({"type": "object", "properties": properties})


def _schema_required(*required, **properties) -> str:
    """Build a JSON schema string with required fields."""
    return json.dumps({"type": "object", "properties": properties, "required": list(required)})


class TestActionValidatorFromSchemaJson:
    """Tests for ActionValidator.from_schema_json constructor."""

    def test_creates_validator_from_minimal_schema(self):
        schema = json.dumps({"type": "object"})
        av = ActionValidator.from_schema_json(schema)
        assert av is not None

    def test_creates_validator_from_properties_schema(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        assert av is not None

    def test_invalid_json_schema_raises(self):
        with pytest.raises((RuntimeError, ValueError)):
            ActionValidator.from_schema_json("not valid json")

    def test_empty_object_schema(self):
        schema = json.dumps({"type": "object", "properties": {}})
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({}))
        assert ok is True

    def test_number_property_schema(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        assert av is not None

    def test_string_property_schema(self):
        schema = _schema(name={"type": "string"})
        av = ActionValidator.from_schema_json(schema)
        assert av is not None

    def test_boolean_property_schema(self):
        schema = _schema(enabled={"type": "boolean"})
        av = ActionValidator.from_schema_json(schema)
        assert av is not None


class TestActionValidatorFromActionRegistry:
    """Tests for ActionValidator.from_action_registry constructor."""

    def test_creates_validator_for_registered_action(self):
        reg = ActionRegistry()
        schema = _schema_required("radius", radius={"type": "number"})
        reg.register("create_sphere", description="", category="geo", input_schema=schema)
        av = ActionValidator.from_action_registry(reg, "create_sphere")
        assert av is not None

    def test_validator_from_registry_validates_correctly(self):
        reg = ActionRegistry()
        schema = _schema_required("radius", radius={"type": "number"})
        reg.register("create_sphere", description="", category="geo", input_schema=schema)
        av = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, _errors = av.validate(json.dumps({"radius": 1.5}))
        assert ok is True

    def test_validator_from_registry_catches_missing(self):
        reg = ActionRegistry()
        schema = _schema_required("radius", radius={"type": "number"})
        reg.register("create_sphere", description="", category="geo", input_schema=schema)
        av = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, _errors = av.validate(json.dumps({}))
        assert ok is False

    def test_from_registry_nonexistent_action_raises(self):
        reg = ActionRegistry()
        with pytest.raises((RuntimeError, KeyError, ValueError)):
            ActionValidator.from_action_registry(reg, "nonexistent_action")

    def test_from_registry_with_dcc_name(self):
        reg = ActionRegistry()
        schema = _schema_required("radius", radius={"type": "number"})
        reg.register("create_sphere", description="", category="geo", input_schema=schema, dcc="maya")
        av = ActionValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
        ok, _ = av.validate(json.dumps({"radius": 2.0}))
        assert ok is True

    def test_from_registry_no_schema_action(self):
        """Action registered without schema: validate should accept anything."""
        reg = ActionRegistry()
        reg.register("no_schema_action", description="", category="misc")
        av = ActionValidator.from_action_registry(reg, "no_schema_action")
        ok, _ = av.validate(json.dumps({"anything": "value"}))
        assert ok is True


class TestActionValidatorValidateHappyPath:
    """Tests for ActionValidator.validate with valid inputs."""

    def test_valid_number_field(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({"radius": 1.5}))
        assert ok is True
        assert errors == []

    def test_valid_string_field(self):
        schema = _schema_required("name", name={"type": "string"})
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"name": "sphere"}))
        assert ok is True

    def test_valid_boolean_field(self):
        schema = _schema_required("visible", visible={"type": "boolean"})
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"visible": True}))
        assert ok is True

    def test_valid_integer_as_number(self):
        schema = _schema_required("count", count={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"count": 5}))
        assert ok is True

    def test_valid_multiple_fields(self):
        schema = _schema_required(
            "radius",
            "name",
            radius={"type": "number"},
            name={"type": "string"},
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"radius": 1.0, "name": "sphere"}))
        assert ok is True

    def test_extra_field_allowed(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"radius": 1.0, "extra_field": "ignored"}))
        assert ok is True

    def test_optional_field_absent_still_valid(self):
        schema = _schema(
            radius={"type": "number"},
            name={"type": "string"},
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"radius": 1.0}))
        assert ok is True

    def test_validate_returns_tuple(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        result = av.validate(json.dumps({"radius": 1.0}))
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_validate_success_errors_is_empty_list(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({"radius": 1.0}))
        assert ok is True
        assert isinstance(errors, list)
        assert len(errors) == 0

    def test_validate_empty_input_when_no_required(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({}))
        assert ok is True


class TestActionValidatorValidateErrorPaths:
    """Tests for ActionValidator.validate with invalid inputs."""

    def test_missing_required_field_fails(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({}))
        assert ok is False
        assert len(errors) > 0

    def test_wrong_type_string_for_number_fails(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({"radius": "not_a_number"}))
        assert ok is False
        assert len(errors) > 0

    def test_wrong_type_number_for_string_fails(self):
        schema = _schema_required("name", name={"type": "string"})
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"name": 42}))
        assert ok is False

    def test_wrong_type_string_for_boolean_fails(self):
        schema = _schema_required("visible", visible={"type": "boolean"})
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"visible": "true"}))
        assert ok is False

    def test_multiple_fields_one_missing_fails(self):
        schema = _schema_required(
            "radius",
            "name",
            radius={"type": "number"},
            name={"type": "string"},
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"radius": 1.0}))
        assert ok is False

    def test_multiple_fields_both_wrong_type_fails(self):
        schema = _schema_required(
            "radius",
            "name",
            radius={"type": "number"},
            name={"type": "string"},
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _errors = av.validate(json.dumps({"radius": "bad", "name": 123}))
        assert ok is False

    def test_error_messages_are_strings(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({}))
        assert ok is False
        for err in errors:
            assert isinstance(err, str)

    def test_error_message_mentions_field_name(self):
        schema = _schema_required("radius", radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({}))
        assert ok is False
        combined = " ".join(errors)
        assert "radius" in combined

    def test_type_mismatch_error_mentions_field(self):
        schema = _schema_required("count", count={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, errors = av.validate(json.dumps({"count": "five"}))
        assert ok is False
        combined = " ".join(errors)
        assert "count" in combined

    def test_invalid_json_string_raises(self):
        schema = _schema(radius={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        with pytest.raises((RuntimeError, ValueError)):
            av.validate("not json {{{")


class TestActionValidatorSchemaTypes:
    """Tests for various JSON Schema type combinations."""

    def test_array_type_schema(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"items": {"type": "array"}},
                "required": ["items"],
            }
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"items": [1, 2, 3]}))
        assert ok is True

    def test_object_type_schema(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"config": {"type": "object"}},
                "required": ["config"],
            }
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"config": {"key": "value"}}))
        assert ok is True

    def test_nested_required_fields(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {
                    "x": {"type": "number"},
                    "y": {"type": "number"},
                    "z": {"type": "number"},
                },
                "required": ["x", "y", "z"],
            }
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"x": 1.0, "y": 2.0, "z": 3.0}))
        assert ok is True
        ok2, _ = av.validate(json.dumps({"x": 1.0, "y": 2.0}))
        assert ok2 is False

    def test_null_type_schema(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"value": {"type": "null"}},
            }
        )
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"value": None}))
        assert ok is True

    def test_multi_type_schema(self):
        """Schema with a field that accepts multiple types."""
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"value": {"type": ["number", "string"]}},
                "required": ["value"],
            }
        )
        av = ActionValidator.from_schema_json(schema)
        ok1, _ = av.validate(json.dumps({"value": 42}))
        assert ok1 is True
        ok2, _ = av.validate(json.dumps({"value": "hello"}))
        assert ok2 is True

    def test_zero_is_valid_number(self):
        schema = _schema_required("count", count={"type": "number"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"count": 0}))
        assert ok is True

    def test_empty_string_is_valid_string(self):
        schema = _schema_required("name", name={"type": "string"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"name": ""}))
        assert ok is True

    def test_false_is_valid_boolean(self):
        schema = _schema_required("flag", flag={"type": "boolean"})
        av = ActionValidator.from_schema_json(schema)
        ok, _ = av.validate(json.dumps({"flag": False}))
        assert ok is True
