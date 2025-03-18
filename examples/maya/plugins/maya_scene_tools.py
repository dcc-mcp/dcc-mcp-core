"""Maya Scene Tools Plugin

A useful Maya scene tools plugin, providing common scene operations.
This plugin demonstrates how to create a complete Maya plugin, including actual Maya API calls.
"""

# Import built-in modules
from functools import wraps
import inspect
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple
from typing import get_type_hints

# -------------------------------------------------------------------
# Plugin metadata - Just fill in these basic information
# -------------------------------------------------------------------
__plugin_name__ = "Maya Scene Tools"
__plugin_version__ = "1.0.0"
__plugin_description__ = "Useful tools for Maya scene management"
__plugin_author__ = "DCC-MCP-Core Team"
__plugin_requires__ = ["maya"]  # Specify the DCC environment this plugin depends on

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
    # Get function type hints
    type_hints = get_type_hints(func)
    # Get function signature
    sig = inspect.signature(func)
    # Get parameters excluding context
    params = list(sig.parameters.values())[1:] if sig.parameters else []
    
    # Build parameter metadata
    param_info = []
    for param in params:
        param_type = type_hints.get(param.name, Any).__name__
        default = "Required" if param.default is param.empty else str(param.default)
        param_info.append({
            "name": param.name,
            "type": param_type,
            "default": default,
            "required": param.default is param.empty
        })
    
    # Add function metadata
    func.__maya_tool__ = True
    func.__tool_name__ = func.__name__
    func.__tool_params__ = param_info
    func.__tool_return_type__ = type_hints.get('return', Any).__name__
    
    @wraps(func)
    def wrapper(context, *args, **kwargs):
        # 从 context 中提取 maya_client 和 cmds
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
def get_scene_stats(context: Dict[str, Any]) -> Dict[str, Any]:
    """Get the statistics of the current scene.
    
    Args:
        context: MCP server provided context object
    
    Returns:
        Dictionary containing scene statistics

    """
    # Get necessary Maya components from context
    cmds = context.get("maya_client").cmds
    
    # Get scene statistics
    stats = {}
    
    # Get polygon statistics
    try:
        poly_stats = cmds.polyEvaluate()
        if isinstance(poly_stats, dict):
            stats.update(poly_stats)
    except:
        pass
    
    # 获取对象数量
    stats["object_count"] = len(cmds.ls(transforms=True))
    stats["camera_count"] = len(cmds.ls(cameras=True))
    stats["light_count"] = len(cmds.ls(lights=True))
    
    return {
        "status": "success",
        "stats": stats
    }

@maya_tool
def create_primitive(context: Dict[str, Any], 
                    primitive_type: str, 
                    size: float = 1.0, 
                    position: Optional[List[float]] = None, 
                    name: Optional[str] = None) -> Dict[str, Any]:
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
    # Get necessary Maya components from context
    cmds = context.get("maya_client").cmds
    
    # Set default value
    if position is None:
        position = [0.0, 0.0, 0.0]
        
    # Validate primitive type
    valid_types = ["cube", "sphere", "cylinder", "cone", "plane", "torus"]
    if primitive_type not in valid_types:
        return {
            "status": "error",
            "message": f"Invalid primitive type. Must be one of: {', '.join(valid_types)}"
        }
    
    # Create geometric shape
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
    
    # 设置位置
    cmds.move(position[0], position[1], position[2], result)
    
    return {
        "status": "success",
        "result": {
            "name": result,
            "type": primitive_type,
            "size": size,
            "position": position
        }
    }

@maya_tool
def clean_scene(context: Dict[str, Any], 
               keep_cameras: bool = True, 
               keep_lights: bool = True) -> Dict[str, Any]:
    """Clean up the current scene.
    
    Args:
        context: MCP server provided context object
        keep_cameras: Whether to keep cameras
        keep_lights: Whether to keep lights
        
    Returns:
        Operation result

    """
    # Get necessary Maya components from context
    cmds = context.get("maya_client").cmds
    
    all_objects = cmds.ls(transforms=True)
    to_delete = []
    
    for obj in all_objects:
        # Skip default cameras
        if keep_cameras and cmds.listRelatives(obj, type="camera"):
            continue
        # Skip lights
        if keep_lights and cmds.listRelatives(obj, type="light"):
            continue
        # Add to delete list
        to_delete.append(obj)
    
    deleted_count = len(to_delete)
    if to_delete:
        cmds.delete(to_delete)
    
    return {
        "status": "success",
        "message": "Scene cleaned successfully",
        "details": {
            "objects_removed": deleted_count,
            "kept_cameras": keep_cameras,
            "kept_lights": keep_lights
        }
    }

@maya_tool
def random_layout(context: Dict[str, Any], 
                 object_names: List[str], 
                 area_size: float = 10.0, 
                 min_distance: float = 1.0) -> Dict[str, Any]:
    """Randomly layout objects in the scene.
    
    Args:
        context: MCP server provided context object
        object_names: List of object names to layout
        area_size: Layout area size
        min_distance: Minimum distance between objects
        
    Returns:
        Layout result

    """
    # Get necessary Maya components from context
    cmds = context.get("maya_client").cmds
    
    # Get random module
    # Import built-in modules
    import random
    
    if not object_names:
        return {
            "status": "error",
            "message": "No objects specified for layout"
        }
    
    positions = []
    results = []
    
    for obj in object_names:
        # Check if object exists
        if not cmds.objExists(obj):
            continue
            
        valid_position = False
        attempts = 0
        
        while not valid_position and attempts < 50:
            # Generate random position
            pos = [
                random.uniform(-area_size/2, area_size/2),
                0,  # Assuming y=0 plane
                random.uniform(-area_size/2, area_size/2)
            ]
            
            # Check distance from other objects
            valid_position = True
            for other_pos in positions:
                dist = ((pos[0]-other_pos[0])**2 + (pos[2]-other_pos[2])**2)**0.5
                if dist < min_distance:
                    valid_position = False
                    break
            
            if valid_position:
                positions.append(pos)
                cmds.move(pos[0], pos[1], pos[2], obj)
                
                results.append({
                    "name": obj,
                    "position": pos
                })
                break
                
            attempts += 1
    
    return {
        "status": "success",
        "message": f"Randomly positioned {len(results)} objects",
        "objects": results
    }

@maya_tool
def create_camera_shot(context: Dict[str, Any], 
                      target_object: str, 
                      distance: float = 5.0, 
                      angle: Tuple[float, float] = (30.0, 45.0)) -> Dict[str, Any]:
    """Create a camera shot pointing at a target object.
    
    Args:
        context: MCP server provided context object
        target_object: Target object name
        distance: Camera distance from target
        angle: Camera angle (vertical, horizontal)
        
    Returns:
        Camera creation result

    """
    # Get necessary Maya components from context
    cmds = context.get("maya_client").cmds
    
    # Check if target object exists
    if not cmds.objExists(target_object):
        return {
            "status": "error",
            "message": f"Target object '{target_object}' does not exist"
        }
    
    # Import built-in modules
    import math

    # Create camera
    camera_name = f"shot_camera_{target_object}"
    camera = cmds.camera(name=camera_name)[0]
    
    # Calculate camera position
    vertical_rad = math.radians(angle[0])
    horizontal_rad = math.radians(angle[1])
    
    x = distance * math.sin(horizontal_rad) * math.cos(vertical_rad)
    y = distance * math.sin(vertical_rad)
    z = distance * math.cos(horizontal_rad) * math.cos(vertical_rad)
    
    # Set camera position
    cmds.move(x, y, z, camera)
    
    # Create constraint, make camera look at target
    constraint = cmds.aimConstraint(target_object, camera)[0]
    
    return {
        "status": "success",
        "camera": {
            "name": camera,
            "position": [x, y, z],
            "target": target_object,
            "distance": distance,
            "angle": angle,
            "constraint": constraint
        }
    }

# -------------------------------------------------------------------
# Plugin tools information retrieval function - For AI-friendly interface
# -------------------------------------------------------------------

def get_tools_info() -> Dict[str, Any]:
    """Get structured information about all tools in this plugin, for AI interface.
    
    Returns:
        A dictionary containing all tool information

    """
    tools = {}
    
    # Get all functions decorated with @maya_tool
    for name, func in globals().items():
        if hasattr(func, "__maya_tool__") and func.__maya_tool__ is True:
            # Get the first line of the function docstring as a short description
            doc = func.__doc__ or ""
            short_desc = doc.strip().split('\n')[0] if doc else ""
            
            tools[name] = {
                "name": func.__tool_name__,
                "description": short_desc,
                "parameters": func.__tool_params__,
                "return_type": func.__tool_return_type__,
                "full_doc": doc
            }
    
    return tools

# -------------------------------------------------------------------
# Register function - Plugin manager entry point
# -------------------------------------------------------------------

def register() -> Dict[str, Any]:
    """Register plugin functions.
    
    Returns:
        A dictionary containing plugin information and callable functions

    """
    # Build function mapping
    functions = {}
    for name, func in globals().items():
        if hasattr(func, "__maya_tool__") and func.__maya_tool__ is True:
            functions[name] = func
    
    # Return plugin information and functions
    return {
        "info": {
            "name": __plugin_name__,
            "version": __plugin_version__,
            "description": __plugin_description__,
            "author": __plugin_author__,
            "requires": __plugin_requires__,
            "tools_info": get_tools_info()
        },
        "functions": functions
    }
