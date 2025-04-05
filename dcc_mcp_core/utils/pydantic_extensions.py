#!/usr/bin/env python
"""Pydantic extensions module.

This module provides extensions for Pydantic v2, adding support for generating JSON Schema for specific types
such as UUID.

Usage:
    # The patch is automatically applied when the module is imported
    from dcc_mcp_core.utils import pydantic_extensions

    # To manually apply the patch (if auto-patching is disabled):
    from dcc_mcp_core.utils.pydantic_extensions import apply_patches
    apply_patches()

    # To check if patches are applied:
    from dcc_mcp_core.utils.pydantic_extensions import is_patched
    if is_patched():
        print("Pydantic patches are applied")
"""

# Import built-in modules
from typing import Any
from typing import Dict
import uuid

# Import third-party modules
from pydantic import TypeAdapter

# Import Pydantic v2 modules
from pydantic.json_schema import GenerateJsonSchema
from pydantic.json_schema import JsonSchemaMode

# Flag to track if the patch has been applied
_is_patched = False


def generate_uuid_schema(schema: Any) -> Dict[str, Any]:
    """Generate JSON Schema for UUID type.

    Args:
        schema: The schema to process

    Returns:
        Dict with JSON Schema for UUID type

    """
    schema_copy = dict(schema) if schema else {}
    schema_copy["type"] = "string"
    schema_copy["format"] = "uuid"
    return schema_copy


def _generate_uuid_schema_method(self, schema: Any) -> Dict[str, Any]:
    """Generate JSON Schema for UUID type (instance method version).

    Args:
        self: The GenerateJsonSchema instance
        schema: The schema to process

    Returns:
        Dict with JSON Schema for UUID type

    """
    return generate_uuid_schema(schema)


def build_patched_schema_type_method(original_method):
    """Create a patched version of build_schema_type_to_method.

    Args:
        original_method: The original method to patch

    Returns:
        The patched method

    """

    def patched_build_schema_type_to_method(self):
        """Patched method that adds UUID support to the schema mapping.

        Returns:
            Dict mapping types to schema generator methods

        """
        mapping = original_method(self)
        mapping["uuid"] = self.uuid_schema
        return mapping

    return patched_build_schema_type_to_method


def apply_uuid_patch() -> bool:
    """Apply the UUID patch to Pydantic's JSON schema generation.

    Returns:
        bool: True if the patch was applied, False if it was already applied

    """
    global _is_patched

    # Skip if already patched
    if _is_patched and hasattr(GenerateJsonSchema, "uuid_schema"):
        return False

    # Add uuid_schema method to GenerateJsonSchema class
    setattr(GenerateJsonSchema, "uuid_schema", _generate_uuid_schema_method)

    # Update schema_type_to_method mapping
    if hasattr(GenerateJsonSchema, "build_schema_type_to_method"):
        # Patch the build_schema_type_to_method method
        original_method = GenerateJsonSchema.build_schema_type_to_method
        GenerateJsonSchema.build_schema_type_to_method = build_patched_schema_type_method(original_method)

    # Mark as patched
    _is_patched = True
    return True


def apply_patches(auto_apply: bool = True) -> Dict[str, bool]:
    """Apply all Pydantic patches.

    Args:
        auto_apply: Whether to automatically apply patches (default: True)

    Returns:
        Dict mapping patch names to application results (True if applied, False otherwise)

    """
    # Skip if auto-apply is disabled
    if not auto_apply:
        return {"uuid": False}

    # Apply patches
    results = {"uuid": apply_uuid_patch()}

    # Register the UUID type adapter for correct schema generation
    try:
        uuid_adapter = TypeAdapter(uuid.UUID)
        uuid_schema = uuid_adapter.json_schema(mode=JsonSchemaMode.SERIALIZATION)
        if uuid_schema.get("type") != "string" or uuid_schema.get("format") != "uuid":
            uuid_schema["type"] = "string"
            uuid_schema["format"] = "uuid"
    except (ImportError, AttributeError):
        pass

    return results


def is_patched() -> bool:
    """Check if Pydantic patches have been applied.

    Returns:
        bool: True if patches have been applied, False otherwise

    """
    return _is_patched


def _register_uuid_serialization() -> bool:
    """Register UUID serialization to ensure UUIDs are serialized as strings.

    Returns:
        bool: True if registration was successful, False otherwise

    """
    try:
        # Import third-party modules
        from pydantic.json import pydantic_encoder

        original_encoder = pydantic_encoder

        def patched_encoder(obj):
            if isinstance(obj, uuid.UUID):
                return str(obj)
            return original_encoder(obj)

        # Import third-party modules
        import pydantic.json

        pydantic.json.pydantic_encoder = patched_encoder
        return True
    except (ImportError, AttributeError):
        return False


# Apply patches when module is imported
apply_patches()

# Register UUID serialization
_register_uuid_serialization()
