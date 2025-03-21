"""Parameter validation and conversion using Pydantic models.

This module provides utilities for validating and converting function parameters
using Pydantic models, making it easier to handle different input formats and types.
"""

from typing import Any, Dict, List, Optional, Tuple, Callable, Type, get_type_hints, Union
import inspect
import logging
from pydantic import BaseModel, Field, create_model, ValidationError

from dcc_mcp_core.parameters.processor import process_parameters

logger = logging.getLogger(__name__)


def create_parameter_model(func: Callable) -> Type[BaseModel]:
    """Create a Pydantic model for function parameters.
    
    Args:
        func: The function to create a parameter model for
        
    Returns:
        A Pydantic model class for validating function parameters
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
        
        # Check if parameter has a default value
        if param.default is not inspect.Parameter.empty:
            # Parameter has a default value
            fields[name] = (type_hint, Field(default=param.default))
        elif param.kind == inspect.Parameter.VAR_POSITIONAL:
            # *args parameter
            fields[name] = (List[Any], Field(default_factory=list))
        elif param.kind == inspect.Parameter.VAR_KEYWORD:
            # **kwargs parameter
            fields[name] = (Dict[str, Any], Field(default_factory=dict))
        else:
            # Required parameter
            fields[name] = (type_hint, ...)
    
    # Create the model
    model_name = f"{func.__name__}Parameters"
    return create_model(model_name, **fields)


def validate_and_convert_parameters(func: Callable, args: Tuple[Any, ...], kwargs: Dict[str, Any]) -> Dict[str, Any]:
    """Validate and convert function parameters using Pydantic.
    
    Args:
        func: The function to validate parameters for
        args: Positional arguments
        kwargs: Keyword arguments
        
    Returns:
        Dictionary of validated and converted parameters
        
    Raises:
        ValueError: If parameter validation fails
    """
    # Create parameter model
    param_model = create_parameter_model(func)
    
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
    
    # Pre-process string parameters
    processed_kwargs = {}
    for key, value in all_kwargs.items():
        if isinstance(value, str) and key != 'context':
            try:
                # Try to use process_parameters for complex string inputs
                processed_value = process_parameters(value)
                if isinstance(processed_value, dict) and len(processed_value) > 0:
                    processed_kwargs[key] = processed_value
                else:
                    processed_kwargs[key] = value
            except Exception as e:
                logger.debug(f"Error pre-processing parameter {key}: {e}")
                processed_kwargs[key] = value
        else:
            processed_kwargs[key] = value
    
    # Validate and convert parameters using the Pydantic model
    try:
        validated_params = param_model(**processed_kwargs)
        return validated_params.model_dump()
    except ValidationError as e:
        # Convert Pydantic validation error to a more user-friendly message
        error_messages = []
        for error in e.errors():
            loc = '.'.join(str(l) for l in error['loc'])
            msg = error['msg']
            error_messages.append(f"{loc}: {msg}")
        
        raise ValueError(f"Parameter validation failed: {'; '.join(error_messages)}")
