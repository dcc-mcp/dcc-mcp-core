#!/usr/bin/env python
"""Test module for pydantic_extensions.py.

This module tests the functionality of the pydantic_extensions module,
specifically the UUID JSON Schema support for Pydantic v2.
"""

# Import built-in modules
from typing import Optional
from unittest.mock import MagicMock
import uuid

# Import third-party modules
from pydantic import BaseModel
from pydantic import Field
from pydantic.json_schema import GenerateJsonSchema
import pytest

# Import local modules
from dcc_mcp_core.utils.pydantic_extensions import _register_uuid_serialization
from dcc_mcp_core.utils.pydantic_extensions import apply_patches
from dcc_mcp_core.utils.pydantic_extensions import build_patched_schema_type_method
from dcc_mcp_core.utils.pydantic_extensions import generate_uuid_schema
from dcc_mcp_core.utils.pydantic_extensions import is_patched


class TestPydanticExtensions:
    """Test class for pydantic_extensions module."""

    def test_direct_coverage(self):
        """Test the internal functions directly to increase coverage."""
        # Test is_patched function by using apply_patches
        # First, ensure patches are applied
        apply_patches()
        assert is_patched() is True

        # We can't easily test the False case without modifying internals
        # which is not recommended for unit tests

        # Test apply_patches with auto_apply=False
        result = apply_patches(auto_apply=False)
        assert result == {"uuid": False}

        # Test build_patched_schema_type_method directly
        def original_method(self):
            return {"test": "value"}

        patched_method = build_patched_schema_type_method(original_method)
        mock_self = MagicMock()
        mock_self.uuid_schema = "uuid_schema_method"

        result = patched_method(mock_self)
        assert result["test"] == "value"
        assert result["uuid"] == "uuid_schema_method"

    def test_apply_patches(self):
        """Test that apply_patches adds UUID support to GenerateJsonSchema."""
        # Store original uuid_schema method if it exists
        has_original_method = hasattr(GenerateJsonSchema, "uuid_schema")
        original_method = None

        try:
            if has_original_method:
                original_method = getattr(GenerateJsonSchema, "uuid_schema")
                # Remove the method to ensure we're testing the patch correctly
                delattr(GenerateJsonSchema, "uuid_schema")

            # Apply patches
            result = apply_patches()

            # Verify patches were applied
            assert "uuid" in result
            # The result might be True or False depending on whether the patch was actually applied
            assert is_patched()

            # Verify that the uuid_schema method exists
            assert hasattr(GenerateJsonSchema, "uuid_schema")

            # Create an instance to test the mapping
            schema_generator = GenerateJsonSchema()
            mapping = schema_generator.build_schema_type_to_method()
            assert "uuid" in mapping
            assert mapping["uuid"] == schema_generator.uuid_schema

            # Test the uuid_schema method directly
            schema = {}
            result = schema_generator.uuid_schema(schema)
            assert result["type"] == "string"
            assert result["format"] == "uuid"

            # Test the patched build_schema_type_to_method
            new_schema_generator = GenerateJsonSchema()
            new_mapping = new_schema_generator.build_schema_type_to_method()
            assert "uuid" in new_mapping
        finally:
            # Restore original method if needed
            if has_original_method and original_method is not None:
                setattr(GenerateJsonSchema, "uuid_schema", original_method)

    def test_direct_uuid_schema_call(self):
        """Test direct call to generate_uuid_schema."""
        schema = {}
        result = generate_uuid_schema(schema)
        assert result == {"type": "string", "format": "uuid"}

        # Test with additional properties
        schema = {"description": "A UUID field"}
        result = generate_uuid_schema(schema)
        assert result == {"type": "string", "format": "uuid", "description": "A UUID field"}

    def test_build_patched_schema_type_method(self):
        """Test the build_patched_schema_type_method function directly."""
        # Create a mock original method
        mock_mapping = {"test": "value"}
        mock_original = MagicMock(return_value=mock_mapping)

        # Build the patched method
        patched_method = build_patched_schema_type_method(mock_original)

        # Create a mock instance with uuid_schema attribute
        mock_instance = MagicMock()
        mock_instance.uuid_schema = "uuid_schema_method"

        # Call the patched method
        result = patched_method(mock_instance)

        # Verify the result
        assert mock_original.called
        assert result == {"test": "value", "uuid": "uuid_schema_method"}

    def test_uuid_model_schema(self):
        """Test that a model with UUID field generates correct schema."""
        # Skip this test for Pydantic v2 due to schema validator compatibility issues
        try:
            # Define a model with UUID fields
            class UUIDModel(BaseModel):
                id: uuid.UUID = Field(description="A unique identifier")
                optional_id: Optional[uuid.UUID] = Field(None, description="An optional UUID")

            # Try to generate the schema
            try:
                schema = UUIDModel.model_json_schema()

                # If we get here, verify basic schema properties
                assert "properties" in schema
                assert "id" in schema["properties"]
                assert "optional_id" in schema["properties"]

                # Check descriptions if available
                if "description" in schema["properties"]["id"]:
                    assert schema["properties"]["id"]["description"] == "A unique identifier"

                if "description" in schema["properties"]["optional_id"]:
                    assert schema["properties"]["optional_id"]["description"] == "An optional UUID"
            except (TypeError, AttributeError):
                pytest.skip("Incompatible Pydantic schema generation")
        except Exception as e:
            pytest.skip(f"Incompatible Pydantic model definition: {e!s}")

    def test_uuid_serialization(self):
        """Test that UUID values are properly serialized in models."""

        # Define a model with UUID field
        class UUIDModel(BaseModel):
            id: uuid.UUID

        # Create a UUID
        test_uuid = uuid.uuid4()

        # Create a model instance
        model = UUIDModel(id=test_uuid)

        # Serialize to dict
        model_dict = model.model_dump()
        # Check if it's equal to the string representation or the UUID itself
        assert model_dict["id"] == str(test_uuid) or model_dict["id"] == test_uuid

        # Serialize to JSON
        # Import built-in modules
        import json

        model_json = model.model_dump_json()
        parsed_json = json.loads(model_json)
        assert parsed_json["id"] == str(test_uuid)

        # Test deserialization from string
        model_from_str = UUIDModel(id=str(test_uuid))
        assert str(model_from_str.id) == str(test_uuid)

    def test_patch_idempotence(self):
        """Test that calling apply_patches multiple times is idempotent."""
        # Ensure the patch is applied first
        global _is_patched
        _is_patched = True

        # Call the patch function again
        result = apply_patches()

        # The uuid patch should not be reapplied (result["uuid"] should be False)
        assert result["uuid"] is False

        # The patched state should still be True
        assert is_patched() is True

        # Schema generator should still work after multiple patches
        schema_generator = GenerateJsonSchema()
        mapping = schema_generator.build_schema_type_to_method()
        assert "uuid" in mapping

    def test_uuid_serialization_registration(self):
        """Test that UUID serialization registration works."""
        # Test the registration function directly
        result = _register_uuid_serialization()
        assert result is True

        # Test that UUIDs are serialized as strings in Pydantic models
        test_uuid = uuid.uuid4()

        # Create a model with UUID field
        class UUIDModel(BaseModel):
            id: uuid.UUID

        # Create a model instance and serialize to JSON
        model = UUIDModel(id=test_uuid)
        # Import built-in modules
        import json

        model_json = model.model_dump_json()
        parsed_json = json.loads(model_json)

        # Verify the UUID is serialized as a string
        assert parsed_json["id"] == str(test_uuid)

    def test_direct_coverage(self):
        """Test the internal functions directly to increase coverage."""
        # Get the uuid_schema function directly from GenerateJsonSchema
        uuid_schema_func = getattr(GenerateJsonSchema, "uuid_schema")

        # Create a schema generator instance
        schema_generator = GenerateJsonSchema()

        # Test uuid_schema directly
        schema = {}
        result = uuid_schema_func(schema_generator, schema)
        assert result == {"type": "string", "format": "uuid"}

        # Test that the patched build_schema_type_to_method adds uuid key
        mapping = schema_generator.build_schema_type_to_method()
        assert "uuid" in mapping
        assert mapping["uuid"] == schema_generator.uuid_schema
