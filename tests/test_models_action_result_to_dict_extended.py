"""Extended tests for the ActionResultModel.to_dict method in models.py."""

# Import built-in modules
import json

# Import local modules
from dcc_mcp_core.models import ActionResultModel


class TestActionResultModelToDictExtended:
    """Extended tests for the ActionResultModel.to_dict method."""

    def test_to_dict_basic_functionality(self):
        """Test the basic functionality of to_dict method."""
        # Create a result with various fields
        result = ActionResultModel(
            success=True,
            message="Operation completed successfully",
            prompt="You can now proceed to the next step",
            error=None,
            context={"key": "value", "nested": {"inner": 123}},
        )

        # Get the result dictionary
        result_dict = result.to_dict()

        # Verify the result contains all expected fields
        assert result_dict["success"] is True
        assert result_dict["message"] == "Operation completed successfully"
        assert result_dict["prompt"] == "You can now proceed to the next step"
        assert result_dict["error"] is None
        assert "key" in result_dict["context"]
        assert result_dict["context"]["key"] == "value"
        assert "nested" in result_dict["context"]
        assert result_dict["context"]["nested"]["inner"] == 123

    def test_to_dict_with_error(self):
        """Test to_dict method with error field set."""
        # Create a result with error field
        result = ActionResultModel(
            success=False,
            message="Operation failed",
            error="An error occurred",
            context={"error_code": 500},
        )

        # Get the result dictionary
        result_dict = result.to_dict()

        # Verify the result contains all expected fields
        assert result_dict["success"] is False
        assert result_dict["message"] == "Operation failed"
        assert result_dict["error"] == "An error occurred"
        assert result_dict["context"]["error_code"] == 500

    def test_to_dict_with_empty_context(self):
        """Test to_dict method with empty context."""
        # Create a result with empty context
        result = ActionResultModel(
            success=True,
            message="Operation completed successfully",
        )

        # Get the result dictionary
        result_dict = result.to_dict()

        # Verify the result contains all expected fields
        assert result_dict["success"] is True
        assert result_dict["message"] == "Operation completed successfully"
        assert result_dict["prompt"] is None
        assert result_dict["error"] is None
        assert isinstance(result_dict["context"], dict)
        assert len(result_dict["context"]) == 0

    def test_to_dict_serialization(self):
        """Test that to_dict output can be serialized to JSON."""
        # Create a result with various fields
        result = ActionResultModel(
            success=True,
            message="Operation completed successfully",
            prompt="You can now proceed to the next step",
            context={"key": "value", "nested": {"inner": 123}},
        )

        # Get the result dictionary and serialize to JSON
        result_dict = result.to_dict()
        json_str = json.dumps(result_dict)

        # Deserialize and verify
        deserialized = json.loads(json_str)
        assert deserialized["success"] is True
        assert deserialized["message"] == "Operation completed successfully"
        assert deserialized["prompt"] == "You can now proceed to the next step"
        assert deserialized["error"] is None
        assert deserialized["context"]["key"] == "value"
        assert deserialized["context"]["nested"]["inner"] == 123

    def test_to_dict_with_complex_context(self):
        """Test to_dict method with complex nested context."""
        # Create a complex context with various types
        complex_context = {
            "string": "text",
            "integer": 42,
            "float": 3.14,
            "boolean": True,
            "null": None,
            "list": [1, 2, 3, "four", 5.0],
            "nested_dict": {
                "key1": "value1",
                "key2": 2,
                "deeper": {
                    "deepest": "bottom",
                    "numbers": [1, 2, 3],
                },
            },
        }

        # Create a result with complex context
        result = ActionResultModel(
            success=True,
            message="Complex context test",
            context=complex_context,
        )

        # Get dictionary representation
        result_dict = result.to_dict()

        # Verify the context is preserved correctly
        assert result_dict["context"] == complex_context

        # Verify it can be JSON serialized (important for API responses)
        json_str = json.dumps(result_dict)
        json_dict = json.loads(json_str)
        assert json_dict["context"] == complex_context

    def test_to_dict_with_empty_context(self):
        """Test to_dict method with empty context."""
        # Create a result with empty context
        result = ActionResultModel(
            success=True,
            message="Empty context test",
        )

        # Get dictionary representation
        result_dict = result.to_dict()

        # Verify the context is an empty dict
        assert result_dict["context"] == {}

    def test_to_dict_with_all_none_values(self):
        """Test to_dict method with all optional fields set to None."""
        # Create a result with only required fields
        result = ActionResultModel(
            success=True,
            message="Minimal test",
            prompt=None,
            error=None,
            context={},
        )

        # Get dictionary representation
        result_dict = result.to_dict()

        # Verify the structure
        assert result_dict["success"] is True
        assert result_dict["message"] == "Minimal test"
        assert result_dict["prompt"] is None
        assert result_dict["error"] is None
        assert result_dict["context"] == {}
