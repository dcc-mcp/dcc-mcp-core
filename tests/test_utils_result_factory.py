#!/usr/bin/env python
"""Tests for the result_factory module."""

# Import built-in modules
from unittest.mock import patch

# Import local modules
# Import internal modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.result_factory import ensure_dict_context
from dcc_mcp_core.utils.result_factory import error_result
from dcc_mcp_core.utils.result_factory import from_exception
from dcc_mcp_core.utils.result_factory import success_result
from dcc_mcp_core.utils.result_factory import validate_action_result


class TestSuccessResult:
    """Tests for the success_result function."""

    def test_basic_success(self):
        """Test basic success result creation."""
        result = success_result("Operation successful")
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert result.message == "Operation successful"
        assert result.prompt is None
        assert result.error is None
        assert result.context == {}

    def test_success_with_prompt(self):
        """Test success result with prompt."""
        result = success_result("Operation successful", prompt="You can now proceed to the next step")
        assert result.success is True
        assert result.message == "Operation successful"
        assert result.prompt == "You can now proceed to the next step"
        assert result.error is None
        assert result.context == {}

    def test_success_with_context(self):
        """Test success result with context data."""
        result = success_result("Created 3 objects", object_ids=[1, 2, 3], total_count=3)
        assert result.success is True
        assert result.message == "Created 3 objects"
        assert result.prompt is None
        assert result.error is None
        assert result.context == {"object_ids": [1, 2, 3], "total_count": 3}

    def test_success_with_all_params(self):
        """Test success result with all parameters."""
        result = success_result(
            "Operation successful", prompt="You can now proceed to the next step", object_ids=[1, 2, 3], total_count=3
        )
        assert result.success is True
        assert result.message == "Operation successful"
        assert result.prompt == "You can now proceed to the next step"
        assert result.error is None
        assert result.context == {"object_ids": [1, 2, 3], "total_count": 3}


class TestErrorResult:
    """Tests for the error_result function."""

    def test_basic_error(self):
        """Test basic error result creation."""
        result = error_result("Operation failed", "File not found")
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert result.message == "Operation failed"
        assert result.prompt is None
        assert result.error == "File not found"
        assert result.context == {}

    def test_error_with_prompt(self):
        """Test error result with prompt."""
        result = error_result("Operation failed", "File not found", prompt="Please check the file path and try again")
        assert result.success is False
        assert result.message == "Operation failed"
        assert result.prompt == "Please check the file path and try again"
        assert result.error == "File not found"
        assert result.context == {}

    def test_error_with_possible_solutions(self):
        """Test error result with possible solutions."""
        result = error_result(
            "Operation failed",
            "File not found",
            possible_solutions=[
                "Check if the file exists",
                "Verify the file path",
                "Ensure you have permission to access the file",
            ],
        )
        assert result.success is False
        assert result.message == "Operation failed"
        assert result.error == "File not found"
        assert "possible_solutions" in result.context
        assert len(result.context["possible_solutions"]) == 3
        assert "Check if the file exists" in result.context["possible_solutions"]

    def test_error_with_context(self):
        """Test error result with context data."""
        result = error_result(
            "Operation failed", "File not found", file_path="/path/to/file.txt", attempted_operations=["read", "write"]
        )
        assert result.success is False
        assert result.message == "Operation failed"
        assert result.error == "File not found"
        assert result.context == {"file_path": "/path/to/file.txt", "attempted_operations": ["read", "write"]}

    def test_error_with_all_params(self):
        """Test error result with all parameters."""
        result = error_result(
            "Operation failed",
            "File not found",
            prompt="Please check the file path and try again",
            possible_solutions=[
                "Check if the file exists",
                "Verify the file path",
                "Ensure you have permission to access the file",
            ],
            file_path="/path/to/file.txt",
            attempted_operations=["read", "write"],
        )
        assert result.success is False
        assert result.message == "Operation failed"
        assert result.prompt == "Please check the file path and try again"
        assert result.error == "File not found"
        assert "possible_solutions" in result.context
        assert "file_path" in result.context
        assert "attempted_operations" in result.context


class TestFromException:
    """Tests for the from_exception function."""

    def test_basic_exception(self):
        """Test basic exception result creation."""
        try:
            raise ValueError("Invalid value")
        except Exception as e:
            result = from_exception(e)

        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Error" in result.message
        assert "Please check error details and retry" in result.prompt
        assert result.error == "Invalid value"
        assert "error_type" in result.context
        assert result.context["error_type"] == "ValueError"
        assert "traceback" in result.context

    def test_exception_with_custom_message(self):
        """Test exception result with custom message."""
        try:
            raise ValueError("Invalid value")
        except Exception as e:
            result = from_exception(e, message="Custom error message")

        assert result.success is False
        assert result.message == "Custom error message"
        assert "Please check error details and retry" in result.prompt
        assert result.error == "Invalid value"

    def test_exception_with_custom_prompt(self):
        """Test exception result with custom prompt."""
        try:
            raise ValueError("Invalid value")
        except Exception as e:
            result = from_exception(e, prompt="Custom prompt message")

        assert result.success is False
        assert "Error" in result.message
        assert result.prompt == "Custom prompt message"
        assert result.error == "Invalid value"

    def test_exception_without_traceback(self):
        """Test exception result without traceback."""
        try:
            raise ValueError("Invalid value")
        except Exception as e:
            result = from_exception(e, include_traceback=False)

        assert result.success is False
        assert "Error" in result.message
        assert "traceback" not in result.context

    def test_exception_with_context(self):
        """Test exception result with context data."""
        try:
            raise ValueError("Invalid value")
        except Exception as e:
            result = from_exception(e, input_value="test", operation="validation")

        assert result.success is False
        assert "Error" in result.message
        assert result.context["input_value"] == "test"
        assert result.context["operation"] == "validation"


class TestEnsureDictContext:
    """Tests for the ensure_dict_context function."""

    def test_dict_context(self):
        """Test with dictionary context."""
        context = {"key": "value", "number": 42}
        result = ensure_dict_context(context)
        assert result == context

    def test_list_context(self):
        """Test with list context."""
        context = [1, 2, 3]
        result = ensure_dict_context(context)
        assert result == {"items": [1, 2, 3]}

    def test_tuple_context(self):
        """Test with tuple context."""
        context = (1, 2, 3)
        result = ensure_dict_context(context)
        assert result == {"items": (1, 2, 3)}

    def test_set_context(self):
        """Test with set context."""
        context = {1, 2, 3}
        result = ensure_dict_context(context)
        assert "items" in result
        assert set(result["items"]) == {1, 2, 3}

    def test_scalar_context(self):
        """Test with scalar context."""
        context = 42
        result = ensure_dict_context(context)
        assert result == {"value": 42}

        context = "string value"
        result = ensure_dict_context(context)
        assert result == {"value": "string value"}

        context = True
        result = ensure_dict_context(context)
        assert result == {"value": True}

    def test_none_context(self):
        """Test with None context."""
        context = None
        result = ensure_dict_context(context)
        assert result == {"value": None}


class TestValidateActionResult:
    """Tests for the validate_action_result function."""

    def test_with_action_result_model(self):
        """Test with ActionResultModel instance."""
        original = ActionResultModel(
            success=True, message="Test message", prompt="Test prompt", context={"key": "value"}
        )
        result = validate_action_result(original)
        assert result is original

    def test_with_action_result_model_non_dict_context(self):
        """Test with ActionResultModel instance with non-dict context."""
        # Since Pydantic validation, directly creating an ActionResultModel with non-dict context will fail
        # Therefore, we need to create a valid ActionResultModel first, then mock non-dict context
        original = ActionResultModel(
            success=True, message="Test message", prompt="Test prompt", context={"items": [1, 2, 3]}
        )

        with patch.object(original, "context", [1, 2, 3]):
            result = validate_action_result(original)
            assert result.success == original.success
            assert result.message == original.message
            assert result.prompt == original.prompt
            assert result.context == {"items": [1, 2, 3]}

    def test_with_dict(self):
        """Test with dictionary."""
        data = {"success": True, "message": "Test message", "prompt": "Test prompt", "context": {"key": "value"}}
        result = validate_action_result(data)
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert result.message == "Test message"
        assert result.prompt == "Test prompt"
        assert result.context == {"key": "value"}

    def test_with_dict_non_dict_context(self):
        """Test with dictionary with non-dict context."""
        data = {"success": True, "message": "Test message", "prompt": "Test prompt", "context": [1, 2, 3]}
        result = validate_action_result(data)
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert result.message == "Test message"
        assert result.prompt == "Test prompt"
        assert result.context == {"items": [1, 2, 3]}

    def test_with_invalid_dict(self):
        """Test with invalid dictionary."""
        data = {"invalid_key": "invalid_value"}
        result = validate_action_result(data)
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Unable to convert result" in result.message
        assert "original_result" in result.context

    def test_with_scalar(self):
        """Test with scalar value."""
        result = validate_action_result(42)
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert "Successfully processed result" in result.message
        assert result.context == {"value": 42}

    def test_with_list(self):
        """Test with list value."""
        result = validate_action_result([1, 2, 3])
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert "Successfully processed result" in result.message
        assert result.context == {"items": [1, 2, 3]}

    def test_with_none(self):
        """Test with None value."""
        result = validate_action_result(None)
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert "Successfully processed result" in result.message
        assert result.context == {"value": None}
