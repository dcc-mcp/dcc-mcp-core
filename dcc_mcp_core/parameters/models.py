"""Pydantic models for parameter validation and conversion in DCC-MCP-Core.

This module defines structured data models for parameters used in action functions,
providing automatic validation, conversion, and documentation.
"""

# Import built-in modules
import functools
import inspect
import logging
from typing import Any
from typing import Callable
from typing import Dict
from typing import List
from typing import TypeVar
from typing import Union
from typing import get_type_hints

# Import third-party modules
from pydantic import Field
from pydantic import ValidationError
from pydantic import create_model

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.exceptions import ParameterValidationError

logger = logging.getLogger(__name__)

# Type variable for function return type
T = TypeVar('T')


def create_parameter_model_from_function(func: Callable) -> Any:
    """Create a Pydantic model from a function's signature.

    This function analyzes the function's signature, including type hints and default values,
    and creates a Pydantic model that can be used to validate and convert parameters.

    Args:
        func: The function to create a model for

    Returns:
        A Pydantic model class for the function's parameters

    """
    # Get function signature
    sig = inspect.signature(func)

    # Get type hints - use get_type_hints to resolve forward references
    try:
        type_hints = get_type_hints(func)
    except (NameError, TypeError):
        # Fall back to annotations if get_type_hints fails
        type_hints = getattr(func, '__annotations__', {})

    # Create field definitions for the model
    fields = {}
    for name, param in sig.parameters.items():
        # Skip 'self' parameter for methods
        if name == 'self':
            continue

        # Get type hint
        type_hint = type_hints.get(name, Any)

        # Skip return type hint
        if name == 'return':
            continue

        # Check if parameter has a default value
        if param.default is not inspect.Parameter.empty:
            # Parameter has a default value
            fields[name] = (type_hint, Field(default=param.default, description=f"Parameter {name}"))
        elif param.kind == inspect.Parameter.VAR_POSITIONAL:
            # *args parameter
            fields[name] = (
                List[Any],
                Field(
                    default_factory=list,
                    description=f"Variable positional arguments (*{name})"
                )
            )
        elif param.kind == inspect.Parameter.VAR_KEYWORD:
            # **kwargs parameter
            fields[name] = (
                Dict[str, Any],
                Field(
                    default_factory=dict,
                    description=f"Variable keyword arguments (**{name})"
                )
            )
        else:
            # Required parameter
            fields[name] = (type_hint, Field(..., description=f"Required parameter {name}"))

    # Create the model
    model_name = f"{func.__name__}Parameters"
    model = create_model(model_name, **fields)

    # Add function reference to the model for later use
    model.__function__ = func

    return model


def validate_function_parameters(func: Callable, *args, **kwargs) -> Dict[str, Any]:
    """Validate and convert function parameters using a Pydantic model.

    Args:
        func: The function whose parameters to validate
        *args: Positional arguments
        **kwargs: Keyword arguments

    Returns:
        Dictionary of validated and converted parameters

    Raises:
        ParameterValidationError: If parameter validation fails

    """
    # Create parameter model if it doesn't exist
    if not hasattr(func, "__parameter_model__"):
        try:
            func.__parameter_model__ = create_parameter_model_from_function(func)
            logger.debug(f"Created parameter model for {func.__name__}")
        except Exception as e:
            logger.error(f"Failed to create parameter model for {func.__name__}: {e!s}")
            raise ParameterValidationError(f"Failed to create parameter model: {e!s}")

    # Get the parameter model
    param_model = func.__parameter_model__

    # Get function signature
    sig = inspect.signature(func)
    param_names = list(sig.parameters.keys())

    # Skip 'self' parameter for methods
    if param_names and param_names[0] == 'self':
        param_names = param_names[1:]

    # Convert args to kwargs
    args_dict = {}
    for i, arg in enumerate(args):
        if i < len(param_names):
            args_dict[param_names[i]] = arg
        else:
            logger.warning(f"Extra positional argument {arg} provided to {func.__name__}")

    # Merge args and kwargs
    all_kwargs = {**args_dict, **kwargs}

    # Validate and convert parameters using the Pydantic model
    try:
        validated_params = param_model(**all_kwargs)
        return validated_params.model_dump()
    except ValidationError as e:
        # Convert Pydantic validation error to a more user-friendly message
        error_messages = []
        for error in e.errors():
            loc = '.'.join(str(location_part) for location_part in error['loc'])
            msg = error['msg']
            error_messages.append(f"{loc}: {msg}")

        error_str = "; ".join(error_messages)
        logger.error(f"Parameter validation failed for {func.__name__}: {error_str}")
        raise ParameterValidationError(f"Parameter validation failed: {error_str}")


def with_parameter_validation(func: Callable[..., T]) -> Callable[..., Union[T, ActionResultModel]]:
    """Add parameter validation to a function.

    This decorator creates a Pydantic model for the function's parameters
    and validates all inputs against this model before calling the function.

    Args:
        func: The function to add parameter validation to

    Returns:
        Decorated function with parameter validation

    """
    # Create parameter model
    try:
        func.__parameter_model__ = create_parameter_model_from_function(func)
    except Exception as e:
        logger.error(f"Failed to create parameter model for {func.__name__}: {e!s}")
        # We'll create it on first call if it fails here

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        # Validate parameters
        try:
            validated_params = validate_function_parameters(func, *args, **kwargs)

            # Call the function with validated parameters
            # Note: We need to handle 'self' for methods
            if args and inspect.ismethod(func):
                return func(args[0], **validated_params)
            else:
                return func(**validated_params)
        except ParameterValidationError as e:
            # Return a structured error response
            return ActionResultModel(
                success=False,
                message="Parameter validation failed",
                error=str(e),
                prompt="Please check the parameter values and try again.",
                context={"validation_error": str(e)}
            )
        except Exception as e:
            # Catch any other exceptions during function execution
            logger.exception(f"Error executing {func.__name__}: {e!s}")
            return ActionResultModel(
                success=False,
                message=f"Error executing {func.__name__}",
                error=str(e),
                prompt="An unexpected error occurred. Please check the error details.",
                context={"error_type": type(e).__name__, "error_details": str(e)}
            )

    return wrapper
