"""A test plugin that simulates Maya integration.

This plugin demonstrates how to use the context parameter to access Maya functionality.
"""

# Import built-in modules
# Import typing modules for annotations
from typing import Any
from typing import Dict
from typing import Optional

# Plugin metadata
__action_name__ = "maya_plugin"
__action_version__ = "1.0.0"
__action_description__ = "Maya integration plugin for testing"
__action_author__ = "Test Author"
__action_requires__ = ["maya"]


def create_cube(size: float = 1.0, context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Create a cube in Maya.

    Args:
        size: Size of the cube
        context: Context object containing Maya modules and functions

    Returns:
        Information about the created cube

    """
    # In a real plugin, we would use context to access Maya modules
    # For testing, we'll simulate the behavior
    if context is None:
        return {"error": "Context is required for Maya operations"}

    # Simulate Maya cube creation
    cube_info = {
        "type": "cube",
        "size": size,
        "transform": "cube1",
        "shape": "cubeShape1"
    }

    return {
        "status": "success",
        "message": f"Created cube with size {size}",
        "data": cube_info
    }


def list_objects(type_filter: Optional[str] = None, context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """List objects in the Maya scene.

    Args:
        type_filter: Optional filter for object types
        context: Context object containing Maya modules and functions

    Returns:
        List of objects in the scene

    """
    if context is None:
        return {"error": "Context is required for Maya operations"}

    # Simulate Maya object listing
    objects = [
        {"name": "persp", "type": "camera"},
        {"name": "top", "type": "camera"},
        {"name": "front", "type": "camera"},
        {"name": "side", "type": "camera"},
        {"name": "cube1", "type": "transform"}
    ]

    # Apply type filter if specified
    if type_filter:
        objects = [obj for obj in objects if obj["type"] == type_filter]

    return {
        "status": "success",
        "count": len(objects),
        "objects": objects
    }
