"""Decorators for DCC-MCP-Core.

This module provides decorators for common patterns in DCC-MCP-Core, such as
error handling and result formatting for AI-friendly communication.
"""

# Import built-in modules
import functools
import inspect
import traceback
from typing import Any
from typing import Callable
from typing import TypeVar
from typing import cast

# Import local modules
from dcc_mcp_core.models import ActionResultModel

F = TypeVar("F", bound=Callable[..., Any])


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
    error_traceback = traceback.format_exc()

    return ActionResultModel(
        success=False,
        message=f"Error executing {function_name}: {e!s}",
        error=str(e),
        prompt=(
            "An error occurred during execution. Please review the error details "
            "and try again with different parameters if needed."
        ),
        context={
            "error_type": type(e).__name__,
            "error_details": error_traceback,
            "function_args": args,
            "function_kwargs": kwargs,
        },
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

    # Otherwise, wrap it in an ActionResultModel
    return ActionResultModel(success=True, message=f"{source} completed successfully", context={"result": result})


def error_handler(func: F) -> F:
    """Handle errors and format results into structured ActionResultModel.

    This decorator wraps a function to catch any exceptions and format the result
    into an ActionResultModel, which provides a structured format for AI to understand
    the outcome of the function call.

    Args:
        func: The function to decorate

    Returns:
        Decorated function that returns an ActionResultModel

    """

    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> ActionResultModel:
        try:
            result = func(*args, **kwargs)
            return format_result(result, func.__name__)
        except Exception as e:
            return format_exception(e, func.__name__, args, kwargs)

    return cast(F, wrapper)


def method_error_handler(method: F) -> F:
    """Handle method errors and return structured results.

    This decorator wraps the method's return value in an ActionResultModel:
    - If the method returns normally, the result is placed in context['result']
    - If the method raises an exception, error details are captured in the ActionResultModel

    Important: Methods decorated with this will ALWAYS return ActionResultModel,
    regardless of their declared return type. The original return value will be
    available in the context['result'] field of the ActionResultModel if successful.

    Example:
        @method_error_handler
        def get_action_info(self, action_name: str) -> ActionModel:
            # This method declares it returns ActionModel
            # But due to the decorator, it actually returns ActionResultModel
            # with the ActionModel in context['result']
            return create_action_model(...)

    Args:
        method: The method to decorate

    Returns:
        Decorated method that returns ActionResultModel

    """

    @functools.wraps(method)
    def wrapper(self, *args, **kwargs) -> ActionResultModel:
        try:
            result = method(self, *args, **kwargs)
            return format_result(result, f"{self.__class__.__name__}.{method.__name__}")
        except Exception as e:
            return format_exception(e, f"{self.__class__.__name__}.{method.__name__}", args, kwargs)

    return cast(F, wrapper)


def with_context(context_param: str = "context"):
    """Ensure a function has a context parameter.

    If the function is called without a context, this decorator will add an empty context.

    Args:
        context_param: Name of the context parameter (default: "context")

    Returns:
        Decorator function

    """

    def decorator(func: F) -> F:
        sig = inspect.signature(func)
        has_context_param = context_param in sig.parameters

        @functools.wraps(func)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            if has_context_param and context_param not in kwargs:
                # Check if it was passed as a positional argument
                context_pos = list(sig.parameters.keys()).index(context_param)
                if len(args) <= context_pos:
                    # Not passed as positional, add it as a keyword argument
                    kwargs[context_param] = {}

            return func(*args, **kwargs)

        return cast(F, wrapper)

    return decorator
