"""Maya plugin for creating random spheres.

A simple plugin example that demonstrates how to create Maya objects
through RPyC remote execution.
"""

# Import built-in modules
import random
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core.actions.manager import ActionResultModel
from dcc_mcp_core.utils.decorators import error_handler

# -------------------------------------------------------------------
# Plugin Metadata - Just fill in these basic information
# -------------------------------------------------------------------
__action_name__ = "Random Spheres Generator"
__action_version__ = "1.0.0"
__action_description__ = "Create random spheres in Maya scene"
__action_author__ = "DCC-MCP-Core Team"
__action_requires__ = ["maya"]  # Specify the DCC environment this plugin depends on

# -------------------------------------------------------------------
# Plugin Function Implementation - Write your functions directly, no need to care about registration process
# -------------------------------------------------------------------


@error_handler
def create_sphere(
    context: Dict[str, Any], radius: float = 1.0, position: Optional[List[float]] = None
) -> Dict[str, Any]:
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
        return {"success": False, "error": "Maya client not found in context"}

    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return {"success": False, "error": "Maya commands interface not found in client"}

    # Set default position
    if position is None:
        position = [0.0, 0.0, 0.0]

    try:
        # Create sphere
        sphere = cmds.polySphere(r=radius, name="sphere")[0]
        cmds.move(position[0], position[1], position[2], sphere)

        return ActionResultModel(
            success=True,
            message=f"Created sphere with radius {radius} at position {position}",
            prompt="You can now modify the sphere or create more objects",
            context={"object_name": sphere, "radius": radius, "position": position},
        )
    except Exception as e:
        return ActionResultModel(success=False, message="Failed to create sphere", error=str(e))


@error_handler
def create_random_spheres(
    context: Dict[str, Any],
    count: int = 5,
    min_radius: float = 0.5,
    max_radius: float = 2.0,
    area_size: float = 10.0,
    distribution_type: str = "uniform",
    with_materials: bool = False,
) -> Dict[str, Any]:
    """Create multiple random spheres.

    Args:
        context: Context object provided by the MCP server
        count: Number of spheres to create
        min_radius: Minimum radius
        max_radius: Maximum radius
        area_size: Area size for sphere distribution
        distribution_type: Type of distribution (uniform, gaussian, grid)
        with_materials: Whether to add random materials to spheres

    Returns:
        Dictionary containing all creation results

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return ActionResultModel(
            success=False, message="Failed to create spheres", error="Maya client not found in context"
        )

    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return ActionResultModel(
            success=False, message="Failed to create spheres", error="Maya commands interface not found in client"
        )

    # Get logger (if available)
    logger = context.get("logger", None)
    if logger:
        logger.debug(f"Creating {count} random spheres")

    try:
        # Import built-in modules
        import math

        results = []

        # Generate positions based on distribution type
        positions = []
        if distribution_type == "uniform":
            # Uniform random distribution
            for i in range(count):
                positions.append(
                    [
                        random.uniform(-area_size / 2, area_size / 2),
                        random.uniform(-area_size / 2, area_size / 2),
                        random.uniform(-area_size / 2, area_size / 2),
                    ]
                )
        elif distribution_type == "gaussian":
            # Gaussian distribution centered at origin
            for i in range(count):
                positions.append(
                    [random.gauss(0, area_size / 4), random.gauss(0, area_size / 4), random.gauss(0, area_size / 4)]
                )
        elif distribution_type == "grid":
            # Grid distribution
            grid_size = math.ceil(math.pow(count, 1 / 3))  # Cube root to get grid dimensions
            spacing = area_size / grid_size
            index = 0
            for x in range(grid_size):
                for y in range(grid_size):
                    for z in range(grid_size):
                        if index < count:
                            positions.append(
                                [
                                    (x - grid_size / 2 + 0.5) * spacing,
                                    (y - grid_size / 2 + 0.5) * spacing,
                                    (z - grid_size / 2 + 0.5) * spacing,
                                ]
                            )
                            index += 1
        else:
            # Default to uniform if invalid distribution type
            for i in range(count):
                positions.append(
                    [
                        random.uniform(-area_size / 2, area_size / 2),
                        random.uniform(-area_size / 2, area_size / 2),
                        random.uniform(-area_size / 2, area_size / 2),
                    ]
                )

        for i in range(count):
            # Generate random radius
            radius = random.uniform(min_radius, max_radius)
            position = positions[i]

            # Create sphere
            sphere = cmds.polySphere(r=radius, name=f"random_sphere_{i + 1}")[0]
            cmds.move(position[0], position[1], position[2], sphere)

            # Add random material if requested
            if with_materials:
                # Create a new Phong material
                material = cmds.shadingNode("phong", asShader=True, name=f"sphere_material_{i + 1}")
                sg = cmds.sets(renderable=True, noSurfaceShader=True, empty=True, name=f"{material}SG")
                cmds.connectAttr(f"{material}.outColor", f"{sg}.surfaceShader")

                # Set random color
                cmds.setAttr(f"{material}.color", random.random(), random.random(), random.random(), type="double3")

                # Assign material to sphere
                cmds.sets(sphere, edit=True, forceElement=sg)

            # Collect results
            results.append({"name": sphere, "radius": radius, "position": position})

        return ActionResultModel(
            success=True,
            message=f"Created {count} random spheres",
            prompt="If you want to modify these spheres, you can use the modify_spheres function",
            context={
                "created_objects": [result["name"] for result in results],
                "total_count": len(results),
                "distribution_type": distribution_type,
            },
        )
    except Exception as e:
        if logger:
            logger.error(f"Error creating spheres: {e}")
        return ActionResultModel(
            success=False,
            message="Failed to create random spheres",
            error=str(e),
            context={
                "error_details": {
                    "count": count,
                    "min_radius": min_radius,
                    "max_radius": max_radius,
                    "area_size": area_size,
                    "distribution_type": distribution_type,
                }
            },
        )


@error_handler
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
        return ActionResultModel(
            success=False, message="Failed to clear spheres", error="Maya client not found in context"
        )

    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return ActionResultModel(
            success=False, message="Failed to clear spheres", error="Maya commands interface not found in client"
        )

    try:
        # Find all objects with name matching random_sphere_*
        spheres = cmds.ls("random_sphere_*")

        # Delete all found spheres
        if spheres:
            cmds.delete(spheres)

            # Also delete materials
            materials = cmds.ls("sphere_material_*")
            if materials:
                cmds.delete(materials)

            # And material groups
            sgs = cmds.ls("sphere_material_*SG")
            if sgs:
                cmds.delete(sgs)

            return ActionResultModel(
                success=True, message=f"Cleared {len(spheres)} spheres", context={"deleted_count": len(spheres)}
            )
        else:
            return ActionResultModel(success=True, message="No spheres found to clear")
    except Exception as e:
        return ActionResultModel(success=False, message="Failed to clear spheres", error=str(e))
