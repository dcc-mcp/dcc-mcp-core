"""Pydantic models for DCC-MCP-Core action management.

This module defines structured data models for actions, functions, parameters,
and other components of the DCC-MCP ecosystem, optimized for AI interaction.

The models are designed to be more streamlined while maintaining all necessary information.
"""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import BaseModel
from pydantic import ConfigDict
from pydantic import Field

# Import local modules
from dcc_mcp_core.utils.constants import ACTION_METADATA


class ParameterModel(BaseModel):
    """Model representing a function parameter.

    This model captures essential information about a function parameter,
    including its name, type, and documentation.
    """

    name: str = Field(description="Name of the parameter")
    type_hint: str = Field(description="Type hint of the parameter (e.g., 'str', 'int', 'List[str]')")
    type: str = Field(description="Actual type of the parameter (e.g., 'str', 'int', 'list')")
    description: str = Field("", description="Description of the parameter")
    required: bool = Field(True, description="Whether the parameter is required")
    default: Optional[Any] = Field(None, description="Default value of the parameter if any")

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "name": "file_path",
                "type_hint": "str",
                "type": "str",
                "description": "Path to the file to process",
                "required": True,
                "default": None,
            }
        }
    )


class FunctionModel(BaseModel):
    """Model representing a function in an action.

    This model captures essential information about a function,
    including its name, parameters, and documentation.
    """

    name: str = Field(description="Name of the function")
    description: str = Field("", description="Description of the function")
    parameters: List[ParameterModel] = Field(default_factory=list, description="Parameters of the function")
    return_type: str = Field(
        "None", description="Return type of the function (e.g., 'str', 'int', 'ActionResultModel')"
    )
    return_description: str = Field("", description="Description of the return value")
    examples: List[Dict[str, Any]] = Field(default_factory=list, description="Usage examples")
    tags: List[str] = Field(default_factory=list, description="Tags for categorizing the function")

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "name": "create_sphere",
                "description": "Creates a sphere in the 3D scene",
                "parameters": [
                    {
                        "name": "radius",
                        "type_hint": "float",
                        "type": "float",
                        "description": "Radius of the sphere",
                        "required": False,
                        "default": 1.0,
                    }
                ],
                "return_type": "ActionResultModel",
                "return_description": "Structured result with message, prompt for next steps, and context data",
                "examples": [
                    {
                        "input": {"radius": 2.0},
                        "output": {
                            "message": "Successfully created sphere at origin",
                            "prompt": "You can now modify the sphere's properties or create more objects",
                            "context": {"object_name": "sphere1", "scene_stats": {"total_objects": 5}},
                        },
                        "description": "Create a sphere with radius 2.0 at the origin",
                    }
                ],
                "tags": ["geometry", "creation"],
            }
        }
    )


class ActionModel(BaseModel):
    """Model representing an action for a DCC application.

    This model captures essential information about an action,
    including metadata and functions.
    """

    name: str = Field(description="Name of the action")
    version: str = Field(ACTION_METADATA["version"]["default"], description="Version of the action")
    description: str = Field(ACTION_METADATA["description"]["default"], description="Description of the action")
    author: str = Field(ACTION_METADATA["author"]["default"], description="Author of the action")
    requires: List[str] = Field(default_factory=list, description="Dependencies required by the action")
    dcc: str = Field(description="DCC application this action is for (e.g., 'maya', 'houdini')")
    functions: Dict[str, FunctionModel] = Field(default_factory=dict, description="Functions provided by the action")
    file_path: str = Field(description="Path to the action file")
    documentation_url: Optional[str] = Field(None, description="URL to the action's documentation")
    tags: List[str] = Field(default_factory=list, description="Tags for categorizing the action")
    capabilities: List[str] = Field(default_factory=list, description="High-level capabilities provided by this action")

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "name": "scene_tools",
                "version": "1.0.0",
                "description": "Tools for managing 3D scenes in Maya",
                "author": "DCC-MCP Team",
                "requires": ["maya"],
                "dcc": "maya",
                "functions": {
                    "create_sphere": {
                        "name": "create_sphere",
                        "description": "Creates a sphere in the 3D scene",
                        "parameters": [
                            {
                                "name": "radius",
                                "type_hint": "float",
                                "type": "float",
                                "description": "Radius of the sphere",
                                "required": False,
                                "default": 1.0,
                            }
                        ],
                        "return_type": "ActionResultModel",
                        "return_description": "Structured result with message, prompt for next steps, and context data",
                        "examples": [
                            {
                                "input": {"radius": 2.0},
                                "output": {
                                    "message": "Successfully created sphere at origin",
                                    "prompt": "You can now modify the sphere's properties or create more objects",
                                    "context": {"object_name": "sphere1", "scene_stats": {"total_objects": 5}},
                                },
                                "description": "Create a sphere with radius 2.0 at the origin",
                            }
                        ],
                        "tags": ["geometry", "creation"],
                    }
                },
                "file_path": "/path/to/scene_tools.py",
                "documentation_url": "https://docs.example.com/scene_tools",
                "tags": ["scene", "geometry"],
                "capabilities": ["create_geometry", "modify_scene"],
            }
        }
    )


class ActionsInfoModel(BaseModel):
    """Model representing information about all available actions.

    This model provides a comprehensive overview of all actions available
    in the system, organized by DCC application.
    """

    dcc_name: str = Field(description="Name of the DCC application")
    actions: Dict[str, ActionModel] = Field(default_factory=dict, description="Available actions")

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "dcc_name": "maya",
                "actions": {
                    "scene_tools": {
                        "name": "scene_tools",
                        "version": "1.0.0",
                        "description": "Tools for managing 3D scenes in Maya",
                        "author": "DCC-MCP Team",
                        "requires": ["maya"],
                        "dcc": "maya",
                        "functions": {
                            "create_sphere": {
                                "name": "create_sphere",
                                "description": "Creates a sphere in the 3D scene",
                                "parameters": [
                                    {
                                        "name": "radius",
                                        "type_hint": "float",
                                        "type": "float",
                                        "description": "Radius of the sphere",
                                        "required": False,
                                        "default": 1.0,
                                    }
                                ],
                                "return_type": "ActionResultModel",
                                "return_description": "Structured result with message, prompt for next steps, "
                                "and context data",
                                "examples": [
                                    {
                                        "input": {"radius": 2.0},
                                        "output": {
                                            "message": "Successfully created sphere at origin",
                                            "prompt": "You can now modify the sphere's properties or create "
                                            "more objects",
                                            "context": {"object_name": "sphere1", "scene_stats": {"total_objects": 5}},
                                        },
                                        "description": "Create a sphere with radius 2.0 at the origin",
                                    }
                                ],
                                "tags": ["geometry", "creation"],
                            }
                        },
                        "file_path": "/path/to/scene_tools.py",
                        "documentation_url": "https://docs.example.com/scene_tools",
                        "tags": ["scene", "geometry"],
                        "capabilities": ["create_geometry", "modify_scene"],
                    }
                },
            }
        }
    )


class ActionResultModel(BaseModel):
    """Model representing the structured result of an action function execution.

    This model provides a standardized format for returning results from action functions,
    including a message about the execution result, a prompt for AI to guide next steps,
    and a context dictionary containing additional information.
    """

    success: bool = Field(True, description="Whether the execution was successful")
    message: str = Field(description="Human-readable message about the execution result")
    prompt: Optional[str] = Field(None, description="Suggestion for AI about next steps or actions")
    error: Optional[str] = Field(None, description="Error message if execution failed")
    context: Dict[str, Any] = Field(default_factory=dict, description="Additional context or data from the execution")

    model_config = ConfigDict(
        json_schema_extra={
            "examples": [
                {
                    "success": True,
                    "message": "Successfully created 10 spheres",
                    "prompt": "If you want to modify these spheres, you can use the modify_spheres function",
                    "error": None,
                    "context": {
                        "created_objects": ["sphere1", "sphere2", "sphere3"],
                        "total_count": 3,
                        "scene_stats": {"total_objects": 15, "memory_usage": "2.5MB"},
                    },
                },
                {
                    "success": False,
                    "message": "Failed to create spheres",
                    "prompt": "Inform the user about the error and suggest a solution. "
                    "Wait for user confirmation before proceeding.",
                    "error": "Memory limit exceeded",
                    "context": {
                        "error_details": {
                            "code": "MEM_LIMIT",
                            "scene_stats": {"available_memory": "1.2MB", "required_memory": "5.0MB"},
                        },
                        "possible_solutions": [
                            "Reduce the number of objects",
                            "Close other scenes",
                            "Increase memory allocation",
                        ],
                    },
                },
            ]
        }
    )
