"""Decorators for DCC-MCP-Core.

This module provides decorators for common patterns in DCC-MCP-Core, such as
error handling and result formatting for AI-friendly communication.
"""

import functools
import inspect
import traceback
from typing import Any, Callable, Dict, Optional, TypeVar, cast, Union

from dcc_mcp_core.models import ActionResultModel


F = TypeVar('F', bound=Callable[..., Any])


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
        message=f"Error executing {function_name}: {str(e)}",
        error=str(e),
        prompt="An error occurred during execution. Please review the error details and try again with different parameters if needed.",
        context={
            "error_type": type(e).__name__,
            "error_details": error_traceback,
            "function_args": args,
            "function_kwargs": kwargs
        }
    )


def format_result(result: Any, function_name: str) -> ActionResultModel:
    """Format a function result into an ActionResultModel.
    
    Args:
        result: The result to format
        function_name: Name of the function that produced the result
        
    Returns:
        ActionResultModel with formatted result
    """
    # If the result is already an ActionResultModel, return it as is
    if isinstance(result, ActionResultModel):
        return result
        
    # If the result is a dictionary, convert it to an ActionResultModel
    if isinstance(result, dict):
        # Check if the result has a success or status field
        if "success" in result:
            success = result["success"]
            message = result.get("message", "Operation completed")
            error = None if success else result.get("error", "Unknown error")
            prompt = result.get("prompt", None)
            # Remove success, message, error, and prompt from context to avoid duplication
            context_dict = {k: v for k, v in result.items() 
                           if k not in ["success", "message", "error", "prompt"]}
            return ActionResultModel(
                success=success,
                message=message,
                error=error,
                prompt=prompt,
                context=context_dict
            )
        elif "status" in result:
            success = result["status"] == "success"
            message = result.get("message", "Operation completed")
            error = None if success else result.get("message", "Unknown error")
            # Remove status and message from context to avoid duplication
            context_dict = {k: v for k, v in result.items() if k not in ["status", "message"]}
            return ActionResultModel(
                success=success,
                message=message,
                error=error,
                context=context_dict
            )
        # If no success or status field, assume success
        return ActionResultModel(
            success=True,
            message=f"{function_name} completed successfully",
            context=result
        )
    
    # For any other return type, wrap it in an ActionResultModel
    return ActionResultModel(
        success=True,
        message=f"{function_name} completed successfully",
        context={"result": result}
    )


def error_handler(func: F) -> F:
    """Decorator to handle errors and format results into structured ActionResultModel.
    
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
    """Decorator for class methods to handle errors and format results.
    
    Similar to error_handler, but designed for class methods where the first argument is 'self'.
    
    Args:
        method: The class method to decorate
        
    Returns:
        Decorated method that returns an ActionResultModel
    """
    @functools.wraps(method)
    def wrapper(self: Any, *args: Any, **kwargs: Any) -> ActionResultModel:
        try:
            result = method(self, *args, **kwargs)
            return format_result(result, f"{self.__class__.__name__}.{method.__name__}")
        except Exception as e:
            return format_exception(e, f"{self.__class__.__name__}.{method.__name__}", args, kwargs)
    
    return cast(F, wrapper)


def with_context(context_param: str = "context"):
    """Decorator factory to ensure a function has a context parameter.
    
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
