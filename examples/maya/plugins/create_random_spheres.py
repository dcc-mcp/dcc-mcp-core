"""Maya plugin for creating random spheres.

A simple plugin example that demonstrates how to create Maya objects
through RPyC remote execution.
"""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List

# -------------------------------------------------------------------
# Plugin Metadata - Just fill in these basic information
# -------------------------------------------------------------------
__plugin_name__ = "Random Spheres Generator"
__plugin_version__ = "1.0.0"
__plugin_description__ = "A plugin for creating random spheres in Maya"
__plugin_author__ = "DCC-MCP-Core Team"
__plugin_requires__ = ["maya"]  # Specify the DCC environment this plugin depends on

# -------------------------------------------------------------------
# Plugin Function Implementation - Write your functions directly, no need to care about registration process
# -------------------------------------------------------------------

def create_sphere(context: Dict[str, Any], radius: float = 1.0, position: List[float] = None) -> Dict[str, Any]:
    """Create a sphere.
    
    Args:
        context: Context object provided by the MCP server
        radius: Radius of the sphere
        position: Position of the sphere [x, y, z]
        
    Returns:
        Dictionary containing creation result

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return {"error": "Maya client not found in context"}
    
    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return {"error": "Maya commands interface not found in client"}
    
    # Set default position
    if position is None:
        position = [0.0, 0.0, 0.0]
    
    try:
        # Create sphere
        sphere = cmds.polySphere(r=radius)[0]
        
        # Set position
        cmds.move(position[0], position[1], position[2], sphere)
        
        return {
            "status": "success",
            "result": {
                "name": sphere,
                "type": "sphere",
                "radius": radius,
                "position": position
            }
        }
    except Exception as e:
        # Capture exception and return error message
        return {
            "status": "error",
            "message": str(e)
        }

def create_random_spheres(context: Dict[str, Any], count: int = 5, 
                         min_radius: float = 0.5, 
                         max_radius: float = 2.0,
                         area_size: float = 10.0) -> Dict[str, Any]:
    """Create multiple random spheres.
    
    Args:
        context: Context object provided by the MCP server
        count: Number of spheres to create
        min_radius: Minimum radius
        max_radius: Maximum radius
        area_size: Area size for sphere distribution
        
    Returns:
        Dictionary containing all creation results

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return {"error": "Maya client not found in context"}
    
    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return {"error": "Maya commands interface not found in client"}
    
    # Get logger (if available)
    logger = context.get("logger", None)
    if logger:
        logger.debug(f"Creating {count} random spheres")
    
    try:
        # Import built-in modules
        import random  # Import random module
        
        results = []
        for i in range(count):
            # Generate random parameters
            radius = random.uniform(min_radius, max_radius)
            position = [
                random.uniform(-area_size/2, area_size/2),
                random.uniform(-area_size/2, area_size/2),
                random.uniform(-area_size/2, area_size/2)
            ]
            
            # Create sphere
            sphere = cmds.polySphere(r=radius, name=f"random_sphere_{i+1}")[0]
            cmds.move(position[0], position[1], position[2], sphere)
            
            # Collect results
            results.append({
                "name": sphere,
                "radius": radius,
                "position": position
            })
        
        return {
            "status": "success",
            "message": f"Created {count} random spheres",
            "spheres": results
        }
    except Exception as e:
        if logger:
            logger.error(f"Error creating spheres: {e}")
        return {
            "status": "error",
            "message": str(e)
        }

def clear_all_spheres(context: Dict[str, Any]) -> Dict[str, Any]:
    """Clear all created spheres.
    
    Args:
        context: Context object provided by the MCP server
        
    Returns:
        Dictionary containing operation result

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return {"error": "Maya client not found in context"}
    
    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return {"error": "Maya commands interface not found in client"}
    
    try:
        # Find all spheres
        spheres = cmds.ls("random_sphere_*")
        count = len(spheres)
        
        # If there are spheres, delete them
        if spheres:
            cmds.delete(spheres)
        
        return {
            "status": "success",
            "message": f"Removed {count} spheres"
        }
    except Exception as e:
        return {
            "status": "error",
            "message": str(e)
        }

# -------------------------------------------------------------------
# Note: No need to write register function, plugin manager will automatically discover and register all public functions
# -------------------------------------------------------------------
