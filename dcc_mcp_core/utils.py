"""Utility functions for the DCC-MCP ecosystem.

This module provides utility functions that are used throughout the DCC-MCP ecosystem.
"""

# Import built-in modules
import inspect
import logging
from typing import Any
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core.constants import PLUGIN_METADATA


def extract_module_metadata(module: Any, module_name: Optional[str] = None) -> Dict[str, Any]:
    """Extract metadata from a module based on predefined metadata configuration.
    
    Args:
        module: The module to extract metadata from
        module_name: Optional name to use as default for the name field
        
    Returns:
        Dictionary containing the extracted metadata

    """
    metadata = {}
    
    for key, config in PLUGIN_METADATA.items():
        attr_name = config["attr"]
        default_value = config["default"]
        
        # For the name field, if default value is None, use module_name
        if key == "name" and default_value is None and module_name is not None:
            default_value = module_name
            
        metadata[key] = getattr(module, attr_name, default_value)
    
    return metadata


def extract_function_info(func: Callable) -> Dict[str, Any]:
    """Extract detailed information about a function.
    
    Args:
        func: The function to extract information from
        
    Returns:
        Dictionary with function information including docstring, parameters, return type, and example

    """
    # Get function docstring
    func_doc = func.__doc__ or ""
    
    # Initialize default values in case signature extraction fails
    parameters = []
    return_type = "Any"
    example = func.__name__ + "(...)"
    
    try:
        # Get function signature
        sig = inspect.signature(func)
        
        # Extract parameters with their types, default values, and descriptions
        parameters = []
        for name, param in sig.parameters.items():
            param_info = {
                "name": name,
                "type": str(param.annotation) if param.annotation != inspect.Parameter.empty else "Any",
                "default": None if param.default == inspect.Parameter.empty else param.default,
                "required": param.default == inspect.Parameter.empty and param.kind not in
                           (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD)
            }
            parameters.append(param_info)
        
        # Extract return type
        return_type = str(sig.return_annotation) if sig.return_annotation != inspect.Signature.empty else "Any"
        
        # Generate a simple usage example
        example = generate_function_example(func, parameters)
    except (ValueError, TypeError) as e:
        # This can happen with built-in functions or C extension functions
        # In this case, we'll return minimal information
        logging.warning(f"Could not extract complete signature for {func.__name__}: {e}")
    
    return {
        "docstring": func_doc,
        "parameters": parameters,
        "return_type": return_type,
        "example": example
    }


def generate_function_example(func: Callable, parameters: List[Dict[str, Any]]) -> str:
    """Generate a simple usage example for a function.
    
    Args:
        func: The function to generate an example for
        parameters: List of parameter information dictionaries
        
    Returns:
        A string containing a usage example

    """
    func_name = func.__name__
    args = []
    
    for param in parameters:
        if not param["required"]:
            # Skip optional parameters in the example
            continue
        
        name = param["name"]
        param_type = param["type"]
        
        # Generate appropriate example values based on parameter type
        if "int" in param_type.lower():
            args.append(f"{name}=1")
        elif "float" in param_type.lower():
            args.append(f"{name}=1.0")
        elif "str" in param_type.lower():
            args.append(f"{name}='example'")
        elif "bool" in param_type.lower():
            args.append(f"{name}=True")
        elif "list" in param_type.lower() or "tuple" in param_type.lower():
            args.append(f"{name}=[]")
        elif "dict" in param_type.lower():
            args.append(f"{name}={{}}")
        else:
            args.append(f"{name}=...")
    
    return f"{func_name}({', '.join(args)})"
