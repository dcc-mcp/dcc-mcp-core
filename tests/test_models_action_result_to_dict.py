"""Tests for the to_dict method in ActionResultModel."""

# Import built-in modules
import json
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.models import ActionResultModel


class TestActionResultModelToDict:
    """Tests for the to_dict method in ActionResultModel."""

    def test_to_dict_basic(self):
        """Test basic to_dict functionality."""
        # Create a simple result model
        result = ActionResultModel(success=True, message="Test message", prompt="Test prompt", context={"key": "value"})

        # Convert to dictionary
        result_dict = result.to_dict()

        # Verify the dictionary
        assert isinstance(result_dict, dict)
        assert result_dict["success"] is True
        assert result_dict["message"] == "Test message"
        assert result_dict["prompt"] == "Test prompt"
        assert result_dict["error"] is None
        assert result_dict["context"] == {"key": "value"}

    def test_to_dict_with_error(self):
        """Test to_dict with error field."""
        # Create a result model with error
        result = ActionResultModel(
            success=False, message="Error message", error="Test error", context={"error_code": 123}
        )

        # Convert to dictionary
        result_dict = result.to_dict()

        # Verify the dictionary
        assert result_dict["success"] is False
        assert result_dict["message"] == "Error message"
        assert result_dict["prompt"] is None
        assert result_dict["error"] == "Test error"
        assert result_dict["context"] == {"error_code": 123}

    def test_to_dict_with_complex_context(self):
        """Test to_dict with complex nested context."""
        # Create a complex context
        complex_context = {
            "nested": {"level1": {"level2": [1, 2, 3], "data": {"a": 1, "b": 2}}},
            "list": ["item1", "item2", {"subitem": "value"}],
        }

        # Create a result model with complex context
        result = ActionResultModel(message="Complex context", context=complex_context)

        # Convert to dictionary
        result_dict = result.to_dict()

        # Verify the dictionary
        assert result_dict["context"] == complex_context
        assert result_dict["context"]["nested"]["level1"]["level2"] == [1, 2, 3]
        assert result_dict["context"]["list"][2]["subitem"] == "value"

    def test_to_dict_json_serializable(self):
        """Test that to_dict returns JSON serializable data."""
        # Create a result model
        result = ActionResultModel(
            success=True, message="JSON test", context={"numbers": [1, 2, 3], "nested": {"key": "value"}}
        )

        # Convert to dictionary
        result_dict = result.to_dict()

        # Try to serialize to JSON
        try:
            json_str = json.dumps(result_dict)
            # Parse back to verify
            parsed = json.loads(json_str)
            assert parsed["success"] is True
            assert parsed["message"] == "JSON test"
            assert parsed["context"]["numbers"] == [1, 2, 3]
        except Exception as e:
            pytest.fail(f"Failed to JSON serialize dictionary: {e}")

    def test_to_dict_with_empty_context(self):
        """Test to_dict with empty context."""
        # Create a result model with empty context
        result = ActionResultModel(message="Empty context", context={})

        # Convert to dictionary
        result_dict = result.to_dict()

        # Verify the dictionary
        assert result_dict["context"] == {}

    def test_to_dict_basic_functionality(self):
        """Test basic to_dict functionality without mocking."""
        # Create a result model
        result = ActionResultModel(message="Compatibility test")

        # Call to_dict method directly
        result_dict = result.to_dict()

        # Verify we got a valid dictionary
        assert isinstance(result_dict, dict)
        assert result_dict["message"] == "Compatibility test"
        assert result_dict["success"] is True
        assert result_dict["prompt"] is None
        assert result_dict["error"] is None
        assert result_dict["context"] == {}

    def test_to_dict_fallback_mechanism(self):
        """Test to_dict fallback mechanism using patch."""
        # Create a result model
        result = ActionResultModel(message="Fallback test", context={"test": "value"})

        # Test fallback when model_dump raises exception
        with patch.object(ActionResultModel, "model_dump", side_effect=Exception("Test exception")):
            # The to_dict method should fall back to dict() or manual dictionary creation
            result_dict = result.to_dict()

            # Verify we got a valid dictionary
            assert isinstance(result_dict, dict)
            assert result_dict["message"] == "Fallback test"
            assert result_dict["context"] == {"test": "value"}

        # Test fallback when both model_dump and dict raise exceptions
        with patch.object(ActionResultModel, "model_dump", side_effect=Exception("Test exception")), patch.object(
            ActionResultModel, "dict", side_effect=Exception("Test exception")
        ):
            # The to_dict method should fall back to manual dictionary creation
            result_dict = result.to_dict()

            # Verify we got a valid dictionary
            assert isinstance(result_dict, dict)
            assert result_dict["message"] == "Fallback test"
            assert result_dict["context"] == {"test": "value"}
