"""Tests for the ActionResultModel methods."""

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.result_factory import error_result
from dcc_mcp_core.utils.result_factory import from_exception
from dcc_mcp_core.utils.result_factory import success_result


class TestActionResultModelAndFactories:
    """Tests for the ActionResultModel methods."""

    def test_success_result_factory(self):
        """Test success_result factory function."""
        # Create a success result
        result = success_result(message="Success message", prompt="Next steps prompt", key1="value1", key2=123)

        # Verify the result
        result_dict = result.to_dict()
        assert result_dict["success"] is True
        assert result_dict["message"] == "Success message"
        assert result_dict["prompt"] == "Next steps prompt"
        assert result_dict["error"] is None
        assert result_dict["context"] == {"key1": "value1", "key2": 123}

    def test_error_result_factory(self):
        """Test error_result factory function."""
        # Create a failure result
        result = error_result(
            message="Failure message",
            error="Error message",
            prompt="Error prompt",
            error_code=404,
            details={"reason": "Not found"},
        )

        # Verify the result
        result_dict = result.to_dict()
        assert result_dict["success"] is False
        assert result_dict["message"] == "Failure message"
        assert result_dict["prompt"] == "Error prompt"
        assert result_dict["error"] == "Error message"
        assert result_dict["context"] == {"error_code": 404, "details": {"reason": "Not found"}}

    def test_from_exception_factory_with_traceback(self):
        """Test from_exception factory function with traceback."""
        # Create an exception
        try:
            # Deliberately cause an exception
            1 / 0
        except Exception as e:
            # Create a result from the exception
            result = from_exception(
                e,
                message="Division error",
                prompt="Check your math",
                include_traceback=True,
                additional_info="This was a test",
            )

            # Verify the result
            result_dict = result.to_dict()
            assert result_dict["success"] is False
            assert result_dict["message"] == "Division error"
            assert result_dict["prompt"] == "Check your math"
            assert result_dict["error"] == "division by zero"
            assert result_dict["context"]["error_type"] == "ZeroDivisionError"
            assert "traceback" in result_dict["context"]
            assert "additional_info" in result_dict["context"]
            assert result_dict["context"]["additional_info"] == "This was a test"

    def test_from_exception_factory_without_traceback(self):
        """Test from_exception factory function without traceback."""
        # Create an exception
        try:
            # Deliberately cause an exception
            int("not a number")
        except Exception as e:
            # Create a result from the exception
            result = from_exception(e, include_traceback=False, custom_field="Custom value")

            # Verify the result
            result_dict = result.to_dict()
            assert result_dict["success"] is False
            assert "Error:" in result_dict["message"]
            assert result_dict["prompt"] == "Please check error details and retry"
            assert "invalid literal for int" in result_dict["error"]
            assert result_dict["context"]["error_type"] == "ValueError"
            assert "traceback" not in result_dict["context"]
            assert result_dict["context"]["custom_field"] == "Custom value"

    def test_with_error(self):
        """Test with_error instance method."""
        # Create a success result
        original = ActionResultModel(
            success=True, message="Original message", prompt="Original prompt", context={"original": "data"}
        )

        # Create a new result with error
        result = original.with_error("New error")

        # Verify the result
        result_dict = result.to_dict()
        assert result_dict["success"] is False  # Changed to False
        assert result_dict["message"] == "Original message"  # Unchanged
        assert result_dict["prompt"] == "Original prompt"  # Unchanged
        assert result_dict["error"] == "New error"  # New error
        assert result_dict["context"] == {"original": "data"}  # Unchanged

    def test_with_context(self):
        """Test with_context instance method."""
        # Create a result
        original = ActionResultModel(
            success=True,
            message="Original message",
            prompt="Original prompt",
            context={"original": "data", "shared": "old value"},
        )

        # Create a new result with updated context
        result = original.with_context(new_key="new value", shared="updated value")

        # Verify the result
        result_dict = result.to_dict()
        assert result_dict["success"] is True  # Unchanged
        assert result_dict["message"] == "Original message"  # Unchanged
        assert result_dict["prompt"] == "Original prompt"  # Unchanged
        assert result_dict["error"] is None  # Unchanged

        # Context should be updated
        assert result_dict["context"]["original"] == "data"  # Original key preserved
        assert result_dict["context"]["new_key"] == "new value"  # New key added
        assert result_dict["context"]["shared"] == "updated value"  # Existing key updated

    def test_with_context_empty_original(self):
        """Test with_context when original context is empty."""
        # Create a result with empty context
        original = ActionResultModel(
            success=True,
            message="Original message",
            # No context provided, should use default empty dict
        )

        # Create a new result with context
        result = original.with_context(new_key="new value")

        # Verify the result
        result_dict = result.to_dict()
        assert result_dict["context"] == {"new_key": "new value"}
