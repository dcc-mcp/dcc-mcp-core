"""Pydantic models for parameter validation and conversion in DCC-MCP-Core.

This module defines structured data models for parameters used in action functions,
providing automatic validation, conversion, and documentation.
"""

from typing import Any, Dict, List, Optional, Union, Tuple, get_type_hints
from pydantic import BaseModel, Field, create_model, ConfigDict, ValidationError
import inspect
import logging

from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.exceptions import ParameterValidationError

logger = logging.getLogger(__name__)



def create_parameter_model_from_function(func) -> type[BaseModel]:
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
    
    # Get type hints
    type_hints = get_type_hints(func)
    
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
            
        # Get parameter description from docstring
        param_desc = ""
        if func.__doc__:
            docstring_lines = func.__doc__.split('\n')
            for i, line in enumerate(docstring_lines):
                if f"{name}:" in line or f"{name} :" in line:
                    # Extract description from this line and possibly the next few lines
                    param_desc = line.split(':', 1)[1].strip() if ':' in line else ""
                    # Check for multi-line descriptions (indented lines following the parameter)
                    for j in range(i + 1, min(i + 5, len(docstring_lines))):
                        next_line = docstring_lines[j].strip()
                        if next_line and not next_line.endswith(':'):
                            param_desc += " " + next_line
                        else:
                            break
                    break
        
        # Check if parameter has a default value
        if param.default is not inspect.Parameter.empty:
            # Parameter has a default value
            fields[name] = (type_hint, Field(default=param.default, description=param_desc))
        elif param.kind == inspect.Parameter.VAR_POSITIONAL:
            # *args parameter
            fields[name] = (List[Any], Field(default_factory=list, description="Variable positional arguments"))
        elif param.kind == inspect.Parameter.VAR_KEYWORD:
            # **kwargs parameter
            fields[name] = (Dict[str, Any], Field(default_factory=dict, description="Variable keyword arguments"))
        else:
            # Required parameter
            fields[name] = (type_hint, Field(..., description=param_desc))
    
    # Create the model
    model_name = f"{func.__name__}Parameters"
    model = create_model(model_name, **fields)
    
    # Add function reference to the model for later use
    model.__function__ = func
    
    return model


def validate_function_parameters(func, *args, **kwargs) -> Dict[str, Any]:
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
        func.__parameter_model__ = create_parameter_model_from_function(func)
    
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
            loc = '.'.join(str(l) for l in error['loc'])
            msg = error['msg']
            error_messages.append(f"{loc}: {msg}")
        
        raise ParameterValidationError(f"Parameter validation failed: {'; '.join(error_messages)}")


def with_parameter_validation(func):
    """Decorator that adds parameter validation to a function.
    
    This decorator creates a Pydantic model for the function's parameters
    and validates all inputs against this model before calling the function.
    
    Args:
        func: The function to add parameter validation to
        
    Returns:
        Decorated function with parameter validation
    """
    # Create parameter model
    func.__parameter_model__ = create_parameter_model_from_function(func)
    
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
                context={"validation_error": str(e)}
            )
    
    # Copy function metadata
    wrapper.__name__ = func.__name__
    wrapper.__doc__ = func.__doc__
    wrapper.__module__ = func.__module__
    wrapper.__qualname__ = func.__qualname__
    wrapper.__annotations__ = func.__annotations__
    
    return wrapper
