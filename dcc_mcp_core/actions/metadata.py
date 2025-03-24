"""Action metadata extraction and model creation utilities.

This module provides functions for extracting metadata from action modules
and creating structured models for AI interaction.
"""

# Import built-in modules
import inspect
import logging
from typing import Any
from typing import Callable
from typing import Dict

# Import local modules
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionsInfoModel
from dcc_mcp_core.models import FunctionModel
from dcc_mcp_core.models import ParameterModel
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
        "name": getattr(action_module, "__action_name__", ""),
        "version": getattr(action_module, "__action_version__", ACTION_METADATA["version"]["default"]),
        "description": getattr(action_module, "__action_description__", ACTION_METADATA["description"]["default"]),
        "author": getattr(action_module, "__action_author__", ACTION_METADATA["author"]["default"]),
        "requires": getattr(action_module, "__action_requires__", []),
        "documentation_url": getattr(action_module, "__action_documentation_url__", ""),
        "tags": getattr(action_module, "__action_tags__", []),
        "capabilities": getattr(action_module, "__action_capabilities__", []),
        "file_path": getattr(action_module, "__file__", ""),
    }

    # Extract docstring if available
    if not metadata["description"] and action_module.__doc__:
        metadata["description"] = inspect.cleandoc(action_module.__doc__)

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
        "name": func_name,
        "description": "",
        "parameters": [],
        "return_type": "None",
        "return_description": "",
        "examples": [],
        "tags": [],
    }

    # Extract docstring
    if func.__doc__:
        doc = inspect.cleandoc(func.__doc__)
        metadata["description"] = doc

    # Extract signature
    try:
        sig = inspect.signature(func)

        # Extract parameters
        for param_name, param in sig.parameters.items():
            # Skip self parameter for methods
            if param_name == "self":
                continue

            # Get parameter type hint
            type_hint = "Any"
            if param.annotation is not param.empty:
                type_hint = str(param.annotation).replace("typing.", "")

            # Get parameter default value
            default = None
            required = True
            if param.default is not param.empty:
                default = param.default
                required = False

            # Create parameter metadata
            metadata["parameters"].append(
                {
                    "name": param_name,
                    "type_hint": type_hint,
                    "type": param.kind.name.lower(),  # Simplified type handling
                    "description": "",  # Will be extracted from docstring
                    "required": required,
                    "default": default,
                }
            )

        # Extract return type
        if sig.return_annotation is not sig.empty:
            metadata["return_type"] = str(sig.return_annotation).replace("typing.", "")
    except (ValueError, TypeError):
        # If we can't get the signature, just continue with default metadata
        pass

    # Extract parameter descriptions and return description from docstring
    if func.__doc__:
        # Simple docstring parsing
        docstring_lines = inspect.cleandoc(func.__doc__).split("\n")

        # Initialize sections
        current_section = "description"
        sections = {"description": [], "Args": [], "Returns": [], "Examples": []}
        param_descriptions = {}

        # Parse docstring sections
        for line in docstring_lines:
            line = line.strip()

            # Check if this is a section header
            if line.endswith(":") and line[:-1] in sections:
                current_section = line[:-1]
                continue

            # Add line to current section
            if current_section in sections:
                sections[current_section].append(line)

                # Extract parameter descriptions
                if current_section == "Args" and ":" in line:
                    parts = line.split(":", 1)
                    if len(parts) == 2:
                        param_name = parts[0].strip()
                        param_desc = parts[1].strip()
                        param_descriptions[param_name] = param_desc

        # Update parameter descriptions
        for param in metadata["parameters"]:
            if param["name"] in param_descriptions:
                param["description"] = param_descriptions[param["name"]]

        # Update return description
        if sections["Returns"]:
            metadata["return_description"] = " ".join(sections["Returns"])

        # Update examples
        if sections["Examples"]:
            metadata["examples"] = [" ".join(sections["Examples"])]

    return metadata


def create_action_model(
    action_name: str, action_module: Any, action_functions: Dict[str, Callable], dcc_name: str = ""
) -> ActionModel:
    """Create an ActionModel instance with comprehensive information about an action.

    Args:
        action_name: Name of the action
        action_module: The action module
        action_functions: Dictionary of function names to function objects
        dcc_name: Name of the DCC application this action is for

    Returns:
        ActionModel instance with comprehensive information about the action

    """
    # Extract metadata from the action module
    metadata = extract_action_metadata(action_module)

    # Create function models
    function_models = {}
    for func_name, func in action_functions.items():
        # Extract function metadata
        func_metadata = extract_function_metadata(func_name, func)

        # Create parameter models
        parameter_models = []
        for param_data in func_metadata.get("parameters", []):
            parameter_models.append(ParameterModel(**param_data))

        # Create function model
        function_models[func_name] = FunctionModel(
            name=func_metadata.get("name", ""),
            description=func_metadata.get("description", ""),
            parameters=parameter_models,
            return_type=func_metadata.get("return_type", "None"),
            return_description=func_metadata.get("return_description", ""),
            examples=func_metadata.get("examples", []),
            tags=func_metadata.get("tags", []),
        )

    # Create action model
    return ActionModel(
        name=metadata.get("name", action_name),
        version=metadata.get("version", ACTION_METADATA["version"]["default"]),
        description=metadata.get("description", ACTION_METADATA["description"]["default"]),
        author=metadata.get("author", ACTION_METADATA["author"]["default"]),
        requires=metadata.get("requires", ACTION_METADATA["requires"]["default"]),
        dcc=dcc_name,
        functions=function_models,
        file_path=metadata.get("file_path", ""),
        documentation_url=metadata.get("documentation_url", None),
        tags=metadata.get("tags", []),
        capabilities=metadata.get("capabilities", []),
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
