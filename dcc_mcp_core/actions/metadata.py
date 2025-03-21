"""Action metadata extraction and model creation utilities.

This module provides functions for extracting metadata from action modules
and creating structured models for AI interaction.
"""

import inspect
import logging
import os
from typing import Any, Dict, List, Optional, Union, get_type_hints, Callable

from dcc_mcp_core.models import ActionModel, FunctionModel, ParameterModel, ActionsInfoModel
from dcc_mcp_core.utils.constants import ACTION_METADATA

logger = logging.getLogger(__name__)




def extract_action_metadata(action_module: Any) -> Dict[str, Any]:
    """Extract metadata from an action module.

    Args:
        action_module: The action module to extract metadata from

    Returns:
        Dictionary with action metadata

    """
    # Default metadata
    metadata = {
        'name': getattr(action_module, '__action_name__', ''),
        'version': getattr(action_module, '__action_version__', '0.1.0'),
        'description': getattr(action_module, '__action_description__', ''),
        'author': getattr(action_module, '__action_author__', ''),
        'requires': getattr(action_module, '__action_requires__', []),
        'documentation_url': getattr(action_module, '__action_documentation_url__', ''),
        'tags': getattr(action_module, '__action_tags__', []),
        'capabilities': getattr(action_module, '__action_capabilities__', []),
        'file_path': getattr(action_module, '__file__', ''),
    }
    
    # Extract docstring if available
    if not metadata['description'] and action_module.__doc__:
        metadata['description'] = inspect.cleandoc(action_module.__doc__)
    
    return metadata


def extract_function_metadata(func_name: str, func: Callable) -> Dict[str, Any]:
    """Extract metadata from a function.

    Args:
        func_name: Name of the function
        func: The function to extract metadata from

    Returns:
        Dictionary with function metadata

    """
    # Default metadata
    metadata = {
        'name': func_name,
        'description': '',
        'parameters': [],
        'return_type': 'None',
        'return_description': '',
        'examples': [],
        'tags': [],
    }
    
    # Extract docstring
    if func.__doc__:
        doc = inspect.cleandoc(func.__doc__)
        metadata['description'] = doc
    
    # Extract signature
    try:
        sig = inspect.signature(func)
        
        # Extract parameters
        for param_name, param in sig.parameters.items():
            # Skip self parameter for methods
            if param_name == 'self':
                continue
                
            # Get parameter type hint
            type_hint = 'Any'
            if param.annotation is not param.empty:
                type_hint = str(param.annotation)
                # Clean up type hint (remove typing. prefix, etc.)
                type_hint = type_hint.replace('typing.', '')
                
            # Get parameter default value
            default = None
            required = True
            if param.default is not param.empty:
                default = str(param.default) if param.default is not None else 'None'
                required = False
                
            # Create parameter metadata
            metadata['parameters'].append({
                'name': param_name,
                'type_hint': type_hint,
                'type': get_parameter_type(param),
                'description': '',  # Will be extracted from docstring
                'required': required,
                'default': default,
            })
            
        # Extract return type
        if sig.return_annotation is not sig.empty:
            metadata['return_type'] = str(sig.return_annotation).replace('typing.', '')
    except (ValueError, TypeError):
        # If we can't get the signature, just continue with default metadata
        pass
    
    # Extract parameter descriptions and return description from docstring
    if func.__doc__:
        # Parse docstring to extract parameter descriptions
        docstring_sections = parse_docstring(func.__doc__)
        
        # Extract parameter descriptions
        if 'Args' in docstring_sections:
            param_docs = parse_parameters_section(docstring_sections['Args'])
            
            # Update parameter descriptions
            for param in metadata['parameters']:
                if param['name'] in param_docs:
                    param['description'] = param_docs[param['name']]
        
        # Extract return description
        if 'Returns' in docstring_sections:
            metadata['return_description'] = docstring_sections['Returns']
            
        # Extract examples
        if 'Examples' in docstring_sections:
            metadata['examples'] = [docstring_sections['Examples']]
    
    return metadata


def create_action_model(action_name: str, action_module: Any, action_functions: Dict[str, Any], file_path: str = None, dcc_name: str = None) -> ActionModel:
    """Create an action model with detailed information.

    Args:
        action_name: Name of the action
        action_module: The action module
        action_functions: Dictionary mapping function names to callable functions
        file_path: Optional path to the action file
        dcc_name: Optional name of the DCC this action is for

    Returns:
        ActionModel instance with comprehensive action information

    """
    # Extract metadata from the action module
    metadata = extract_action_metadata(action_module)
    
    # Use provided file_path or get it from metadata
    if file_path is None:
        file_path = metadata.get('file_path', '')
    
    # Create function models for each function
    function_models = {}
    for func_name, func in action_functions.items():
        # Skip special attributes and non-callable items
        if func_name.startswith('__') or not callable(func):
            continue
            
        # Extract function metadata
        func_metadata = extract_function_metadata(func_name, func)
        
        # Create a function model
        function_models[func_name] = FunctionModel(
            name=func_name,
            description=func_metadata.get('description', ''),
            parameters=[ParameterModel(**param) for param in func_metadata.get('parameters', [])],
            return_type=func_metadata.get('return_type', 'None'),
            return_description=func_metadata.get('return_description', ''),
            examples=func_metadata.get('examples', []),
            tags=func_metadata.get('tags', [])
        )
    
    # Create the action model
    return ActionModel(
        name=metadata.get('name', action_name),
        version=metadata.get('version', '0.1.0'),
        description=metadata.get('description', ''),
        author=metadata.get('author', ''),
        requires=metadata.get('requires', []),
        dcc=dcc_name or '',
        functions=function_models,
        file_path=file_path,
        documentation_url=metadata.get('documentation_url'),
        tags=metadata.get('tags', []),
        capabilities=metadata.get('capabilities', [])
    )


def create_actions_info_model(dcc_name: str, action_info: Dict[str, ActionModel]) -> ActionsInfoModel:
    """Create an actions info model with detailed information.

    Args:
        dcc_name: Name of the DCC
        action_info: Dictionary mapping action names to action models

    Returns:
        ActionsInfoModel instance with comprehensive information about all actions

    """
    return ActionsInfoModel(
        dcc_name=dcc_name,
        actions=action_info,
    )


def parse_docstring(docstring: str) -> Dict[str, str]:
    """Parse a docstring into sections.

    Args:
        docstring: The docstring to parse

    Returns:
        Dictionary mapping section names to section content

    """
    # Clean up docstring
    docstring = inspect.cleandoc(docstring)
    
    # Split into lines
    lines = docstring.split('\n')
    
    # Initialize sections
    sections = {}
    current_section = 'Description'
    current_content = []
    
    # Process each line
    for line in lines:
        # Check if this is a section header
        if line.endswith(':') and not line.startswith(' '):
            # Save previous section
            if current_content:
                sections[current_section] = '\n'.join(current_content).strip()
                current_content = []
            
            # Start new section
            current_section = line.rstrip(':')
        else:
            # Add to current section
            current_content.append(line)
    
    # Save the last section
    if current_content:
        sections[current_section] = '\n'.join(current_content).strip()
    
    return sections


def parse_parameters_section(params_section: str) -> Dict[str, str]:
    """Parse a parameters section from a docstring.

    Args:
        params_section: The parameters section content

    Returns:
        Dictionary mapping parameter names to descriptions

    """
    # Split into lines
    lines = params_section.split('\n')
    
    # Initialize parameters
    params = {}
    current_param = None
    current_description = []
    
    # Process each line
    for line in lines:
        # Check if this is a parameter definition
        if not line.startswith(' ') and ':' in line:
            # Save previous parameter
            if current_param and current_description:
                params[current_param] = '\n'.join(current_description).strip()
                current_description = []
            
            # Parse parameter name and description
            parts = line.split(':', 1)
            current_param = parts[0].strip()
            if len(parts) > 1 and parts[1].strip():
                current_description.append(parts[1].strip())
        elif line.strip() and current_param:
            # Add to current parameter description
            current_description.append(line.strip())
    
    # Save the last parameter
    if current_param and current_description:
        params[current_param] = '\n'.join(current_description).strip()
    
    return params


def get_parameter_type(param: inspect.Parameter) -> str:
    """Get the parameter type as a string.

    Args:
        param: The parameter to get the type for

    Returns:
        String representing the parameter type

    """
    # Default type is 'any'
    param_type = 'any'
    
    # Check annotation
    if param.annotation is not param.empty:
        type_str = str(param.annotation)
        
        # Extract base type from typing annotations
        if 'typing.' in type_str:
            # Handle common typing annotations
            if 'List' in type_str:
                param_type = 'array'
            elif 'Dict' in type_str:
                param_type = 'object'
            elif 'Optional' in type_str:
                # Extract inner type
                inner_type = type_str.split('[', 1)[1].rstrip(']')
                param_type = get_simple_type(inner_type)
            else:
                # Default to string for other typing annotations
                param_type = 'string'
        else:
            # Handle basic types
            param_type = get_simple_type(type_str)
    
    return param_type


def get_simple_type(type_str: str) -> str:
    """Convert a type string to a simple type name.

    Args:
        type_str: The type string to convert

    Returns:
        Simple type name

    """
    # Map Python types to simple types
    type_map = {
        'str': 'string',
        'int': 'integer',
        'float': 'number',
        'bool': 'boolean',
        'list': 'array',
        'dict': 'object',
        'None': 'null',
    }
    
    # Extract base type name
    base_type = type_str.split('[', 1)[0].split('.')[-1]
    
    # Return mapped type or original type
    return type_map.get(base_type, base_type.lower())
