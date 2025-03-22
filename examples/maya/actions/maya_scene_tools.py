"""Maya Scene Tools Plugin

A useful Maya scene tools plugin, providing common scene operations.
This plugin demonstrates how to create a complete Maya plugin, including actual Maya API calls.
"""

# Import built-in modules
from functools import wraps
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# -------------------------------------------------------------------
# Plugin metadata - Just fill in these basic information
# -------------------------------------------------------------------
__action_name__ = "Maya Scene Tools"
__action_version__ = "1.0.0"
__action_description__ = "Useful tools for Maya scene management"
__action_author__ = "DCC-MCP-Core Team"
__action_requires__ = ["maya"]  # Specify the DCC environment this plugin depends on

# -------------------------------------------------------------------
# Plugin entry function decorator - Used to automatically handle context and provide function information
# -------------------------------------------------------------------

def maya_tool(func):
    """Decorator: Mark a function as a Maya tool, automatically handle context parameters.

    This decorator will:
    1. Automatically extract maya_client and cmds from context
    2. Preserve the original function's docstring and type annotations
    3. Add function metadata, used for AI-friendly interfaces
    """
    # Add function metadata
    func.__maya_tool__ = True
    func.__tool_name__ = func.__name__

    @wraps(func)
    def wrapper(context, *args, **kwargs):
        # Extract maya_client and cmds from context
        maya_client = context.get("maya_client", None)
        if not maya_client:
            return {"error": "Maya client not found in context"}

        cmds = maya_client.cmds
        if not cmds:
            return {"error": "Maya commands interface not found in client"}

        # Call original function, pass context and other parameters
        try:
            return func(context, *args, **kwargs)
        except Exception as e:
            return {
                "status": "error",
                "message": str(e)
            }

    return wrapper

# -------------------------------------------------------------------
# Plugin functionality implementation - Use @maya_tool decorator to mark functions
# -------------------------------------------------------------------

@maya_tool
def get_scene_stats(context: Dict[str, Any]):
    """Get the statistics of the current scene.

    Args:
        context: MCP server provided context object

    Returns:
        Dictionary containing scene statistics

    """
    # Extract Maya commands interface
    cmds = context.get("maya_client", {}).get("cmds", None)
    if not cmds:
        return {"error": "Maya commands interface not found"}

    # Initialize statistics dictionary
    stats = {
        "scene_name": "Untitled",
        "object_count": 0,
        "camera_count": 0,
        "light_count": 0,
        "polygon_count": 0,
        "vertex_count": 0
    }

    # Get scene name
    try:
        scene_path = cmds.file(query=True, sceneName=True)
        if scene_path:
            stats["scene_name"] = scene_path.split("/")[-1]
    except:
        pass

    # Get object count
    stats["object_count"] = len(cmds.ls(transforms=True))
    stats["camera_count"] = len(cmds.ls(cameras=True))
    stats["light_count"] = len(cmds.ls(lights=True))

    # Get polygon and vertex counts
    try:
        poly_count = 0
        vertex_count = 0
        for mesh in cmds.ls(type="mesh"):
            poly_count += cmds.polyEvaluate(mesh, face=True)
            vertex_count += cmds.polyEvaluate(mesh, vertex=True)
        stats["polygon_count"] = poly_count
        stats["vertex_count"] = vertex_count
    except:
        pass

    return {
        "status": "success",
        "message": "Scene statistics retrieved",
        "data": stats
    }

@maya_tool
def create_primitive(context: Dict[str, Any],
                    primitive_type: str,
                    size: float = 1.0,
                    position: Optional[List[float]] = None,
                    name: Optional[str] = None):
    """Create a basic geometric shape.

    Args:
        context: MCP server provided context object
        primitive_type: Geometric shape type (cube, sphere, cylinder, cone, plane, torus)
        size: Geometric shape size
        position: Geometric shape position [x, y, z]
        name: Geometric shape name

    Returns:
        Dictionary containing creation result

    """
    # Extract Maya commands interface
    cmds = context.get("maya_client", {}).get("cmds", None)
    if not cmds:
        return {"error": "Maya commands interface not found"}

    # Default position if not provided
    if position is None:
        position = [0, 0, 0]

    # Default name if not provided
    if name is None:
        name = f"{primitive_type}1"

    # Create primitive based on type
    result = None
    if primitive_type == "cube":
        result = cmds.polyCube(w=size, h=size, d=size, name=name)[0]
    elif primitive_type == "sphere":
        result = cmds.polySphere(r=size, name=name)[0]
    elif primitive_type == "cylinder":
        result = cmds.polyCylinder(r=size, h=size*2, name=name)[0]
    elif primitive_type == "cone":
        result = cmds.polyCone(r=size, h=size*2, name=name)[0]
    elif primitive_type == "plane":
        result = cmds.polyPlane(w=size, h=size, name=name)[0]
    elif primitive_type == "torus":
        result = cmds.polyTorus(r=size, sr=size/4, name=name)[0]

    # Set position
    cmds.move(position[0], position[1], position[2], result)

    return {
        "status": "success",
        "message": f"Created {primitive_type} at position {position}",
        "data": {
            "name": result,
            "type": primitive_type,
            "size": size,
            "position": position
        }
    }

# -------------------------------------------------------------------
# Register function - Plugin manager entry point
# -------------------------------------------------------------------

def register():
    """Register plugin functions.

    Returns:
        A dictionary containing plugin information and callable functions

    """
    # Get all functions in this module that are marked as Maya tools
    tools = {}

    # Use globals() to get all objects in this module
    for name, obj in globals().items():
        # Skip non-functions and special names
        if not callable(obj) or name.startswith('_'):
            continue

        # Check if this function is marked as a Maya tool
        if hasattr(obj, '__maya_tool__') and obj.__maya_tool__:
            tools[name] = obj

    # Return plugin information and tools
    return {
        "name": __action_name__,
        "version": __action_version__,
        "description": __action_description__,
        "author": __action_author__,
        "functions": tools
    }
