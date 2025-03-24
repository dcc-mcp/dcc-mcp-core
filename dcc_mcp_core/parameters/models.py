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
import weakref

# Import third-party modules
from pydantic import Field
from pydantic import ValidationError
from pydantic import create_model

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.exceptions import ParameterValidationError

logger = logging.getLogger(__name__)

# Type variable for function return type
T = TypeVar("T")

# Dictionary to store parameter models for methods
_method_parameter_models = weakref.WeakKeyDictionary()


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
        type_hints = getattr(func, "__annotations__", {})

    # Create field definitions for the model
    fields = {}
    for name, param in sig.parameters.items():
        # Skip 'self' parameter for methods
        if name == "self":
            continue

        # Get type hint
        type_hint = type_hints.get(name, Any)

        # Skip return type hint
        if name == "return":
            continue

        # Check if parameter has a default value
        if param.default is not inspect.Parameter.empty:
            # Parameter has a default value
            fields[name] = (type_hint, Field(default=param.default, description=f"Parameter {name}"))
        elif param.kind == inspect.Parameter.VAR_POSITIONAL:
            # *args parameter
            fields[name] = (
                List[Any],
                Field(default_factory=list, description=f"Variable positional arguments (*{name})"),
            )
        elif param.kind == inspect.Parameter.VAR_KEYWORD:
            # **kwargs parameter
            fields[name] = (
                Dict[str, Any],
                Field(default_factory=dict, description=f"Variable keyword arguments (**{name})"),
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
    # For methods, we need special handling
    is_method = inspect.ismethod(func)

    # Create parameter model if it doesn't exist
    if is_method:
        # For methods, use the function object ID as key in our dictionary
        if func not in _method_parameter_models:
            try:
                # Store the model in our dictionary instead of as an attribute
                _method_parameter_models[func] = create_parameter_model_from_function(func)
                logger.debug(f"Created parameter model for method {func.__name__}")
            except Exception as e:
                logger.error(f"Failed to create parameter model for method {func.__name__}: {e!s}")
                raise ParameterValidationError(f"Failed to create parameter model: {e!s}")
        param_model = _method_parameter_models[func]
    else:
        # For regular functions, continue with the original approach
        if not hasattr(func, "__parameter_model__"):
            try:
                func.__parameter_model__ = create_parameter_model_from_function(func)
                logger.debug(f"Created parameter model for {func.__name__}")
            except Exception as e:
                logger.error(f"Failed to create parameter model for {func.__name__}: {e!s}")
                raise ParameterValidationError(f"Failed to create parameter model: {e!s}")
        param_model = func.__parameter_model__

    # Get function signature
    sig = inspect.signature(func)
    param_names = list(sig.parameters.keys())

    # Skip 'self' parameter for methods
    if param_names and param_names[0] == "self":
        param_names = param_names[1:]

    # Convert args to kwargs
    args_dict = {}

    # For methods, skip the first argument (self/instance)
    args_to_process = args[1:] if is_method else args

    for i, arg in enumerate(args_to_process):
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
            loc = ".".join(str(location_part) for location_part in error["loc"])
            msg = error["msg"]
            error_messages.append(f"{loc}: {msg}")

        error_str = "; ".join(error_messages)
        logger.error(f"Parameter validation failed for {func.__name__}: {error_str}")
        raise ParameterValidationError(f"Parameter validation failed: {error_str}")


def with_parameter_validation(func: Callable[..., T]) -> Callable[..., Union[T, ActionResultModel]]:
    """Add parameter validation to a function.

    This decorator validates parameters using Pydantic models and parameter constraints
    before calling the function. If validation fails, it returns an ActionResultModel
    with error details instead of calling the function.

    Args:
        func: The function to add parameter validation to

    Returns:
        Decorated function with parameter validation

    """
    # Get function signature
    sig = inspect.signature(func)
    parameters = list(sig.parameters.values())

    # Create Pydantic model for parameters
    model_fields = {}
    for param in parameters:
        # Skip 'self' parameter for methods
        if param.name == "self":
            continue

        # Get annotation (type hint) for the parameter
        annotation = param.annotation
        if annotation is inspect.Parameter.empty:
            annotation = Any

        # Get default value for the parameter
        default = ...
        if param.default is not inspect.Parameter.empty:
            default = param.default

        # Add field to model
        model_fields[param.name] = (annotation, Field(default=default))

    # Create model class dynamically
    model_name = f"{func.__name__}Parameters"
    model_class = create_model(model_name, **model_fields)

    # Define wrapper function
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        # Check if this is a method call (first arg is self)
        is_method = False
        if args and parameters and parameters[0].name == "self":
            is_method = True
            self_arg = args[0]
            args = args[1:]

        try:
            # Convert args to kwargs
            args_as_kwargs = {}
            for i, arg in enumerate(args):
                param_idx = i
                if is_method:
                    param_idx += 1
                if param_idx < len(parameters):
                    args_as_kwargs[parameters[param_idx].name] = arg

            # Merge args and kwargs
            all_kwargs = {**args_as_kwargs, **kwargs}

            # Validate parameters using Pydantic model
            try:
                validated_params = model_class(**all_kwargs).model_dump()
                logger.debug(f"Parameters validated for {func.__name__}: {validated_params}")
            except ValidationError as e:
                logger.error(f"Parameter validation failed for {func.__name__}: {e}")
                return ActionResultModel(
                    success=False,
                    message="Parameter validation failed",
                    error=str(e),
                    prompt="Please check the parameter values and try again.",
                )

            # Validate parameter constraints (groups and dependencies)
            # Import local modules
            from dcc_mcp_core.parameters.groups import validate_parameter_constraints

            # Use empty tuple as args parameter, and validated_params as kwargs parameter
            constraints_valid, constraint_errors = validate_parameter_constraints(func, (), validated_params)
            if not constraints_valid:
                error_str = "; ".join(constraint_errors)
                logger.error(f"Parameter constraint validation failed for {func.__name__}: {error_str}")
                return ActionResultModel(
                    success=False,
                    message="Parameter validation failed",
                    error=error_str,
                    prompt="Please check the parameter values and try again.",
                )

            # Call function with validated parameters
            if is_method:
                result = func(self_arg, **validated_params)
            else:
                result = func(**validated_params)

            # If the result is already an ActionResultModel, return it as is
            if isinstance(result, ActionResultModel):
                return result

            # Otherwise, wrap the result in an ActionResultModel
            return ActionResultModel(
                success=True, message=f"Successfully executed {func.__name__}", context={"result": result}
            )

        except Exception as e:
            logger.exception(f"Error in parameter validation for {func.__name__}: {e}")
            return ActionResultModel(
                success=False,
                message="Error in parameter validation",
                error=str(e),
                prompt="An unexpected error occurred during parameter validation.",
            )

    return wrapper
