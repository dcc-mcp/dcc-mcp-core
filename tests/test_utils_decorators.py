"""Tests for the utils.decorators module."""

# Import built-in modules

# Import third-party modules

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.decorators import error_handler
from dcc_mcp_core.utils.decorators import format_exception
from dcc_mcp_core.utils.decorators import format_result
from dcc_mcp_core.utils.decorators import method_error_handler
from dcc_mcp_core.utils.decorators import with_context


def test_format_exception():
    """Test formatting exceptions into ActionResultModel."""
    # Create a test exception
    exception = ValueError("Test error message")
    function_name = "test_function"
    args = (1, 2, 3)
    kwargs = {"key": "value"}

    # Format the exception
    result = format_exception(exception, function_name, args, kwargs)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert function_name in result.message
    assert "Test error message" in result.message
    assert "Test error message" in result.error
    assert "error occurred" in result.prompt.lower()
    assert result.context["error_type"] == "ValueError"
    assert "error_details" in result.context
    assert result.context["function_args"] == args
    assert result.context["function_kwargs"] == kwargs


def test_format_result_with_action_result_model():
    """Test formatting an existing ActionResultModel."""
    # Create an ActionResultModel
    original = ActionResultModel(
        success=True, message="Original message", prompt="Original prompt", context={"original": "data"}
    )

    # Format the result
    result = format_result(original, "test_source")

    # Verify the result is unchanged
    assert result is original
    assert result.success is True
    assert result.message == "Original message"
    assert result.prompt == "Original prompt"
    assert result.context == {"original": "data"}


def test_format_result_with_other_types():
    """Test formatting non-ActionResultModel results."""
    # Test with different result types
    test_cases = [("string result", str), (123, int), ({"key": "value"}, dict), ([1, 2, 3], list), (None, type(None))]

    for result_value, result_type in test_cases:
        # Format the result
        source = "test_function"
        result = format_result(result_value, source)

        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert source in result.message
        assert "completed successfully" in result.message
        assert result.context["result"] == result_value
        assert isinstance(result.context["result"], result_type)


def test_error_handler_success():
    """Test error_handler decorator with successful function execution."""

    # Define a test function
    @error_handler
    def test_function(x, y):
        return x + y

    # Call the function
    result = test_function(2, 3)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert "test_function completed successfully" in result.message
    assert result.context["result"] == 5


def test_error_handler_with_action_result_model():
    """Test error_handler with a function that returns ActionResultModel."""

    # Define a test function that returns ActionResultModel
    @error_handler
    def test_function():
        return ActionResultModel(
            success=True, message="Custom message", prompt="Custom prompt", context={"custom": "data"}
        )

    # Call the function
    result = test_function()

    # Verify the result is the original ActionResultModel
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert result.message == "Custom message"
    assert result.prompt == "Custom prompt"
    assert result.context == {"custom": "data"}


def test_error_handler_exception():
    """Test error_handler decorator with function that raises an exception."""

    # Define a test function that raises an exception
    @error_handler
    def test_function():
        raise ValueError("Test error")

    # Call the function
    result = test_function()

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Error executing test_function" in result.message
    assert "Test error" in result.error
    assert "error occurred" in result.prompt.lower()
    assert result.context["error_type"] == "ValueError"
    assert "error_details" in result.context


def test_method_error_handler_success():
    """Test method_error_handler decorator with successful method execution."""

    # Define a test class with a decorated method
    class TestClass:
        @method_error_handler
        def test_method(self, x, y):
            return x + y

    # Create an instance and call the method
    instance = TestClass()
    result = instance.test_method(2, 3)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert "TestClass.test_method completed successfully" in result.message
    assert result.context["result"] == 5


def test_method_error_handler_exception():
    """Test method_error_handler decorator with method that raises an exception."""

    # Define a test class with a decorated method that raises an exception
    class TestClass:
        @method_error_handler
        def test_method(self):
            raise ValueError("Test error")

    # Create an instance and call the method
    instance = TestClass()
    result = instance.test_method()

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Error executing TestClass.test_method" in result.message
    assert "Test error" in result.error
    assert "error occurred" in result.prompt.lower()
    assert result.context["error_type"] == "ValueError"
    assert "error_details" in result.context


def test_with_context_decorator_explicit_context():
    """Test with_context decorator when context is explicitly provided."""

    # Define a test function with context parameter
    @with_context()
    def test_function(x, y, context):
        return x + y, context

    # Call the function with explicit context
    test_context = {"key": "value"}
    result_value, result_context = test_function(2, 3, context=test_context)

    # Verify the result
    assert result_value == 5
    assert result_context is test_context


def test_with_context_decorator_default_context():
    """Test with_context decorator when context is not provided."""

    # Define a test function with context parameter
    @with_context()
    def test_function(x, y, context):
        return x + y, context

    # Call the function without context
    result_value, result_context = test_function(2, 3)

    # Verify the result
    assert result_value == 5
    assert result_context == {}


def test_with_context_decorator_positional_args():
    """Test with_context decorator with positional arguments."""

    # Define a test function with context parameter
    @with_context()
    def test_function(x, y, context):
        return x + y, context

    # Call the function with positional arguments
    test_context = {"key": "value"}
    result_value, result_context = test_function(2, 3, test_context)

    # Verify the result
    assert result_value == 5
    assert result_context is test_context


def test_with_context_decorator_custom_param_name():
    """Test with_context decorator with custom parameter name."""

    # Define a test function with custom context parameter name
    @with_context(context_param="ctx")
    def test_function(x, y, ctx):
        return x + y, ctx

    # Call the function without context
    result_value, result_context = test_function(2, 3)

    # Verify the result
    assert result_value == 5
    assert result_context == {}

    # Call the function with explicit context
    test_context = {"key": "value"}
    result_value, result_context = test_function(2, 3, ctx=test_context)

    # Verify the result
    assert result_value == 5
    assert result_context is test_context


def test_with_context_decorator_no_context_param():
    """Test with_context decorator with function that has no context parameter."""

    # Define a test function without context parameter
    @with_context()
    def test_function(x, y):
        return x + y

    # Call the function
    result = test_function(2, 3)

    # Verify the result
    assert result == 5


# Integration tests
def test_integration_error_handler_with_context():
    """Test integration of error_handler and with_context decorators."""

    # Define a test function with both decorators
    @error_handler
    @with_context()
    def test_function(x, y, context=None):
        context["computed"] = x + y
        return context

    # Call the function without context
    result = test_function(2, 3)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert result.context["result"] == {"computed": 5}

    # Call the function with explicit context
    test_context = {"initial": "value"}
    result = test_function(2, 3, context=test_context)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert result.context["result"] == {"initial": "value", "computed": 5}


def test_integration_method_error_handler_with_context():
    """Test integration of method_error_handler and with_context decorators."""

    # Define a test class with a decorated method
    class TestClass:
        @method_error_handler
        @with_context()
        def test_method(self, x, y, context=None):
            context["computed"] = x + y
            return context

    # Create an instance and call the method
    instance = TestClass()
    result = instance.test_method(2, 3)

    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert result.context["result"] == {"computed": 5}
