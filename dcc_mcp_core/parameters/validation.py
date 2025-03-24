"""Parameter validation and conversion using Pydantic models and parameter constraints.

This module provides utilities for validating and converting function parameters
using Pydantic models and parameter constraints, making it easier to handle different
input formats and types while enforcing logical relationships between parameters.
"""

# Import built-in modules
import inspect
import logging
from typing import Any
from typing import Callable
from typing import Dict
from typing import Tuple
from typing import Union

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.parameters.groups import validate_parameter_constraints
from dcc_mcp_core.parameters.groups import with_parameter_validation
from dcc_mcp_core.parameters.models import validate_function_parameters

logger = logging.getLogger(__name__)


def validate_and_convert_parameters(func: Callable, args: Tuple[Any, ...], kwargs: Dict[str, Any]) -> Dict[str, Any]:
    """Validate and convert function parameters using Pydantic.

    This is a wrapper around validate_function_parameters from models.py.

    Args:
        func: The function to validate parameters for
        args: Positional arguments
        kwargs: Keyword arguments

    Returns:
        Dictionary of validated and converted parameters

    Raises:
        ValueError: If parameter validation fails

    """
    try:
        return validate_function_parameters(func, *args, **kwargs)
    except Exception as e:
        logger.exception(f"Parameter validation failed for {func.__name__}: {e!s}")
        raise ValueError(f"Parameter validation failed: {e!s}")


def validate_parameters_with_constraints(
    func: Callable, args: Tuple[Any, ...], kwargs: Dict[str, Any]
) -> Union[Dict[str, Any], ActionResultModel]:
    """Validate and convert function parameters using both Pydantic and parameter constraints.

    This function combines type validation from Pydantic with logical validation from parameter
    constraints (groups and dependencies).

    Args:
        func: The function to validate parameters for
        args: Positional arguments
        kwargs: Keyword arguments

    Returns:
        Either a dictionary of validated parameters or an ActionResultModel with error details

    """
    try:
        # First, validate and convert using Pydantic
        validated_params = validate_function_parameters(func, *args, **kwargs)

        # Then, validate parameter constraints
        is_valid, errors = validate_parameter_constraints(func, args, validated_params)

        if not is_valid:
            return ActionResultModel(
                success=False,
                message="Parameter validation failed",
                error="\n".join(errors),
                prompt="Please check the parameter values and try again.",
                context={"validation_errors": errors},
            )

        return validated_params
    except Exception as e:
        logger.exception(f"Parameter validation failed for {func.__name__}: {e!s}")
        return ActionResultModel(
            success=False,
            message="Parameter validation failed",
            error=str(e),
            prompt="Please check the parameter values and try again.",
            context={"exception": str(e)},
        )


def create_validation_decorator(with_constraints: bool = True) -> Callable:
    """Create a parameter validation decorator with optional constraint checking.

    Args:
        with_constraints: Whether to include parameter constraint validation

    Returns:
        A decorator function that adds parameter validation to a function

    """
    if with_constraints:
        return with_parameter_validation
    else:

        def decorator(func):
            def wrapper(*args, **kwargs):
                try:
                    # Validate and convert parameters
                    validated_params = validate_function_parameters(func, *args, **kwargs)

                    # Get function signature
                    sig = inspect.signature(func)
                    param_names = list(sig.parameters.keys())

                    # Skip 'self' parameter for methods
                    if param_names and param_names[0] == "self":
                        # Call with self and validated parameters
                        return func(args[0], **validated_params)
                    else:
                        # Call with validated parameters
                        return func(**validated_params)
                except Exception as e:
                    logger.exception(f"Parameter validation failed for {func.__name__}: {e!s}")
                    return ActionResultModel(
                        success=False,
                        message="Parameter validation failed",
                        error=str(e),
                        prompt="Please check the parameter values and try again.",
                        context={"exception": str(e)},
                    )

            # Copy function metadata
            wrapper.__name__ = func.__name__
            wrapper.__doc__ = func.__doc__
            wrapper.__module__ = func.__module__
            wrapper.__qualname__ = func.__qualname__
            wrapper.__annotations__ = func.__annotations__

            return wrapper

        return decorator


# Convenience exports
validate_parameters = validate_parameter_constraints
