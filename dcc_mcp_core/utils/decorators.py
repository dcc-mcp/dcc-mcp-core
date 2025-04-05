"""Decorators for DCC-MCP-Core.

This module provides decorators for common patterns in DCC-MCP-Core, such as
error handling and result formatting for AI-friendly communication.
"""

# Import built-in modules
import functools
import inspect
from typing import Any
from typing import Callable
from typing import TypeVar

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.result_factory import from_exception
from dcc_mcp_core.utils.result_factory import success_result

# Define more specific type variables for better type checking
T = TypeVar("T")  # Return type of the original function
F = TypeVar("F", bound=Callable[..., Any])  # General callable


def format_exception(e: Exception, function_name: str, args: tuple, kwargs: dict) -> ActionResultModel:
    """Format an exception into an ActionResultModel.

    Args:
        e: The exception to format
        function_name: Name of the function that raised the exception
        args: Positional arguments passed to the function
        kwargs: Keyword arguments passed to the function

    Returns:
        ActionResultModel with formatted exception details

    """
    # Create context with function call information
    context = {
        "function_name": function_name,
        "function_args": args,
        "function_kwargs": kwargs,
        "error_details": str(e),
    }

    # Use the factory function for consistent error handling
    return from_exception(
        e,
        message=f"Error executing {function_name}: {e!s}",
        prompt="""An error occurred during execution.
Please review the error details and try again.""",
        **context,
    )


def format_result(result: Any, source: str) -> ActionResultModel:
    """Format a result as an ActionResultModel.

    This function ensures that all results are properly wrapped in an ActionResultModel:
    - If result is already an ActionResultModel, it is returned as is
    - Otherwise, it is wrapped in a new ActionResultModel with success=True and the
      original result stored in context['result']

    Args:
        result: The result to format
        source: Source of the result (for logging)

    Returns:
        ActionResultModel containing the result

    """
    # If result is already an ActionResultModel, return it as is
    if isinstance(result, ActionResultModel):
        return result

    # Create a successful result with the original result in context
    return success_result(message=f"{source} completed successfully", result=result)


def error_handler(func: Callable[..., T]) -> Callable[..., ActionResultModel]:
    """Handle errors and format results into structured ActionResultModel.

    This decorator wraps a function or method to catch any exceptions and format the result
    into an ActionResultModel, which provides a structured format for AI to understand
    the outcome of the function call.

    This works with both regular functions and class methods. For class methods, it automatically
    detects the 'self' parameter and includes the class name in error messages.

    Important: Functions/methods decorated with this will ALWAYS return ActionResultModel,
    regardless of their declared return type. The original return value will be
    available in the context['result'] field of the ActionResultModel if successful.

    Example for function:
        @error_handler
        def get_data(param: str) -> Dict[str, Any]:
            # This function declares it returns Dict[str, Any]
            # But due to the decorator, it actually returns ActionResultModel
            # with the Dict in context['result']
            return {"data": param}

    Example for method:
        @error_handler
        def get_action_info(self, action_name: str) -> ActionModel:
            # This method declares it returns ActionModel
            # But due to the decorator, it actually returns ActionResultModel
            # with the ActionModel in context['result']
            return create_action_model(...)

    Args:
        func: The function or method to decorate

    Returns:
        Decorated function/method that returns ActionResultModel

    """
    is_method = inspect.ismethod(func) or (
        inspect.isfunction(func) and func.__code__.co_varnames and func.__code__.co_varnames[0] == "self"
    )

    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> ActionResultModel:
        try:
            result = func(*args, **kwargs)

            # Determine the source name for better error messages
            if is_method and args:
                # For methods, include class name
                instance = args[0]
                source = f"{instance.__class__.__name__}.{func.__name__}"
            else:
                # For regular functions
                source = func.__name__

            return format_result(result, source)
        except Exception as e:
            # Determine function name for error messages
            if is_method and args:
                instance = args[0]
                func_name = f"{instance.__class__.__name__}.{func.__name__}"
            else:
                func_name = func.__name__

            return format_exception(e, func_name, args, kwargs)

    return wrapper  # Type checking will be handled by the return type annotation


def with_context(context_param: str = "context"):
    """Ensure a function has a context parameter.

    If the function is called without a context, this decorator will add an empty context.
    This simplifies functions that need an optional context parameter without requiring
    default values in every function definition.

    Example:
        @with_context()  # Default parameter name is 'context'
        def process_data(data: Dict[str, Any], context: Dict[str, Any]) -> Any:
            # context will always be available, even if not provided by the caller
            context['processed'] = True
            return data

        # Can be called without providing context
        process_data({"key": "value"})  # context will be {} automatically

        # Or with an explicit context
        process_data({"key": "value"}, {"user_id": 123})

    Args:
        context_param: Name of the context parameter (default: "context")

    Returns:
        Decorator function that ensures the context parameter is available

    """

    def decorator(func: Callable[..., T]) -> Callable[..., T]:
        # Get the function signature to check parameters
        sig = inspect.signature(func)

        # Check if the function has the context parameter
        if context_param not in sig.parameters:
            # If the function doesn't have the context parameter, just return it unchanged
            return func

        @functools.wraps(func)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            # If context is already provided in kwargs, use it as is
            if context_param in kwargs:
                return func(*args, **kwargs)

            # Get the position of the context parameter
            param_names = list(sig.parameters.keys())
            context_position = param_names.index(context_param)

            # Check if context was provided as a positional argument
            if len(args) > context_position:
                # Context is already provided as positional argument
                return func(*args, **kwargs)

            # Add empty context as a keyword argument
            kwargs[context_param] = {}
            return func(*args, **kwargs)

        return wrapper  # Type checking handled by return type annotation

    return decorator
