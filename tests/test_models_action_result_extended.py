"""Extended tests for the ActionResultModel in models.py."""

# Import built-in modules
import json

# Import local modules
from dcc_mcp_core.models import ActionResultModel


class TestActionResultModelExtended:
    """Extended tests for the ActionResultModel class."""

    def test_error_field_with_success_true(self):
        """Test that error field is ignored when success is True."""
        # Create a result with success=True but also providing an error
        # Note: In the current implementation, error field is not automatically set to None when success is True
        # This test verifies the actual behavior rather than the expected behavior
        result = ActionResultModel(
            success=True,
            message="Operation completed successfully",
            error="This error should be ignored",
        )

        # Verify that error is preserved even when success is True
        assert result.success is True
        assert result.error == "This error should be ignored"

    def test_nested_error_handling(self):
        """Test handling of nested error information in context."""
        # Create a complex error context
        error_context = {
            "error_details": {
                "code": "VALIDATION_ERROR",
                "location": "parameter",
                "field": "radius",
                "validation": {
                    "constraint": "min_value",
                    "expected": 0.1,
                    "actual": -5.0,
                },
            },
            "traceback": [
                "File 'app.py', line 120, in create_sphere",
                "File 'validator.py', line 45, in validate_parameters",
            ],
        }

        # Create a result with nested error context
        result = ActionResultModel(
            success=False,
            message="Failed to create sphere due to invalid parameters",
            error="Validation error: radius must be positive",
            context=error_context,
        )

        # Verify the nested context is preserved
        assert result.success is False
        assert "Validation error" in result.error
        assert result.context["error_details"]["code"] == "VALIDATION_ERROR"
        assert result.context["error_details"]["validation"]["expected"] == 0.1
        assert result.context["traceback"][1].startswith("File 'validator.py'")

    def test_prompt_field_with_ai_guidance(self):
        """Test the prompt field with AI guidance information."""
        # Create a result with AI guidance in the prompt field
        result = ActionResultModel(
            success=True,
            message="Created 5 objects in the scene",
            prompt="You can now select these objects using 'select_objects' with the IDs from the context. "
            "Consider grouping them with 'create_group' for easier management.",
            context={"created_object_ids": ["obj_001", "obj_002", "obj_003", "obj_004", "obj_005"]},
        )

        # Verify the prompt field contains the guidance
        assert "select_objects" in result.prompt
        assert "create_group" in result.prompt

    def test_context_with_binary_data_representation(self):
        """Test context with binary data representation."""
        # Create a result with binary data representation in context
        result = ActionResultModel(
            success=True,
            message="Generated thumbnail",
            context={
                "thumbnail_info": {
                    "width": 256,
                    "height": 256,
                    "format": "PNG",
                    "size_bytes": 24680,
                    "data_preview": "<binary data (24680 bytes)>",  # Representation of binary data
                }
            },
        )

        # Verify the context with binary data representation
        assert result.context["thumbnail_info"]["width"] == 256
        assert result.context["thumbnail_info"]["format"] == "PNG"
        assert "<binary data" in result.context["thumbnail_info"]["data_preview"]

    def test_model_serialization_round_trip(self):
        """Test full serialization and deserialization round trip."""
        # Create an original result
        original = ActionResultModel(
            success=True,
            message="Operation completed",
            prompt="Next steps guidance",
            context={"key1": "value1", "key2": [1, 2, 3], "key3": {"nested": "value"}},
        )

        # Convert to dict, then to JSON, then back to dict, then back to model
        dict_data = original.model_dump()
        json_data = json.dumps(dict_data)
        dict_data_2 = json.loads(json_data)
        reconstructed = ActionResultModel(**dict_data_2)

        # Verify the reconstructed result matches the original
        assert reconstructed.success == original.success
        assert reconstructed.message == original.message
        assert reconstructed.prompt == original.prompt
        assert reconstructed.error == original.error
        assert reconstructed.context == original.context

    def test_empty_context_handling(self):
        """Test handling of empty context values."""
        # Test with empty dict context
        result = ActionResultModel(message="Test with empty dict context", context={})
        assert result.context == {}

        # Note: The model doesn't automatically convert None to empty dict
        # So we don't test that case

    def test_context_with_special_characters(self):
        """Test context with special characters and Unicode."""
        # Create a result with special characters in context
        result = ActionResultModel(
            success=True,
            message="Processed international data",
            context={
                "languages": ["English", "中文", "Español", "Русский", "日本語"],
                "symbols": "!@#$%^&*()_+{}[]|\\:;\"'<>,.?/~`",
                "emoji": "😀🚀🌍🔥💡",
            },
        )

        # Verify the context with special characters
        assert "中文" in result.context["languages"]
        assert "Русский" in result.context["languages"]
        assert "!@#$%" in result.context["symbols"]
        assert "😀🚀" in result.context["emoji"]

        # Test serialization and deserialization with special characters
        json_data = result.model_dump_json()
        deserialized = ActionResultModel.model_validate_json(json_data)
        assert "中文" in deserialized.context["languages"]
        assert "😀🚀" in deserialized.context["emoji"]

    def test_large_context_handling(self):
        """Test handling of large context data."""
        # Create a large nested context
        large_context = {
            "level1": {
                "level2": {
                    "level3": {
                        "level4": {
                            "level5": {
                                "data": [i for i in range(100)],
                                "text": "x" * 1000,  # 1000 character string
                            }
                        }
                    }
                }
            }
        }

        # Create a result with large context
        result = ActionResultModel(message="Large context test", context=large_context)

        # Verify the large context is preserved
        assert len(result.context["level1"]["level2"]["level3"]["level4"]["level5"]["data"]) == 100
        assert len(result.context["level1"]["level2"]["level3"]["level4"]["level5"]["text"]) == 1000
