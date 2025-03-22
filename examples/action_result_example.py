"""Example demonstrating the use of ActionResultModel in DCC-MCP-Core.

This example shows how to create and use structured action results for better
AI interaction and error handling.
"""

# Import built-in modules
import logging
from typing import List

# Import local modules
# Import DCC-MCP-Core modules
from dcc_mcp_core.models import ActionResultModel

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


def create_spheres(count: int = 3, radius: float = 1.0) -> ActionResultModel:
    """Create multiple spheres with the specified radius.

    Args:
        count: Number of spheres to create
        radius: Radius of each sphere

    Returns:
        ActionResultModel with the result of the operation

    """
    try:
        # Simulate creating spheres
        logger.info(f"Creating {count} spheres with radius {radius}")

        # Check for invalid parameters
        if count <= 0:
            return ActionResultModel(
                success=False,
                message="Failed to create spheres: count must be positive",
                error="Invalid parameter: count must be positive",
                prompt="Please try again with a positive count value.",
                context={
                    "error_details": {
                        "code": "INVALID_PARAM",
                        "param": "count",
                        "value": count
                    }
                }
            )

        if radius <= 0:
            return ActionResultModel(
                success=False,
                message="Failed to create spheres: radius must be positive",
                error="Invalid parameter: radius must be positive",
                prompt="Please try again with a positive radius value.",
                context={
                    "error_details": {
                        "code": "INVALID_PARAM",
                        "param": "radius",
                        "value": radius
                    }
                }
            )

        # Simulate memory limit error for large counts
        if count > 100:
            return ActionResultModel(
                success=False,
                message="Failed to create spheres: memory limit exceeded",
                error="Memory limit exceeded",
                prompt="Try reducing the number of spheres or closing other scenes to free up memory.",
                context={
                    "error_details": {
                        "code": "MEM_LIMIT",
                        "scene_stats": {
                            "available_memory": "1.2MB",
                            "required_memory": "5.0MB"
                        }
                    },
                    "possible_solutions": [
                        "Reduce the number of objects",
                        "Close other scenes",
                        "Increase memory allocation"
                    ]
                }
            )

        # Create the spheres (simulated)
        created_objects = [f"sphere{i+1}" for i in range(count)]

        # Return success result with context
        return ActionResultModel(
            success=True,
            message=f"Successfully created {count} spheres with radius {radius}",
            prompt="If you want to modify these spheres, you can use the modify_spheres function",
            context={
                "created_objects": created_objects,
                "total_count": len(created_objects),
                "scene_stats": {
                    "total_objects": len(created_objects),
                    "memory_usage": f"{count * 0.25:.1f}MB"
                }
            }
        )

    except Exception as e:
        # Handle unexpected errors
        logger.error(f"Error creating spheres: {e}")
        return ActionResultModel(
            success=False,
            message=f"Failed to create spheres: {e!s}",
            error=str(e),
            prompt="An unexpected error occurred. Please check the error message and try again.",
            context={"exception": str(e)}
        )


def modify_spheres(names: List[str], scale: float = 1.0, color: str = "red") -> ActionResultModel:
    """Modify properties of existing spheres.

    Args:
        names: List of sphere names to modify
        scale: Scale factor to apply
        color: Color to apply

    Returns:
        ActionResultModel with the result of the operation

    """
    try:
        # Simulate modifying spheres
        logger.info(f"Modifying {len(names)} spheres: scale={scale}, color={color}")

        # Check for invalid parameters
        if not names:
            return ActionResultModel(
                success=False,
                message="Failed to modify spheres: no sphere names provided",
                error="Invalid parameter: names list is empty",
                prompt="Please provide at least one sphere name to modify.",
                context={
                    "error_details": {
                        "code": "EMPTY_LIST",
                        "param": "names"
                    }
                }
            )

        if scale <= 0:
            return ActionResultModel(
                success=False,
                message="Failed to modify spheres: scale must be positive",
                error="Invalid parameter: scale must be positive",
                prompt="Please try again with a positive scale value.",
                context={
                    "error_details": {
                        "code": "INVALID_PARAM",
                        "param": "scale",
                        "value": scale
                    }
                }
            )

        # Simulate missing objects
        missing_objects = [name for name in names if not name.startswith("sphere")]
        if missing_objects:
            return ActionResultModel(
                success=False,
                message=f"Failed to modify spheres: {len(missing_objects)} objects not found",
                error=f"Objects not found: {', '.join(missing_objects)}",
                prompt="Please check the sphere names and try again.",
                context={
                    "error_details": {
                        "code": "NOT_FOUND",
                        "missing_objects": missing_objects
                    }
                }
            )

        # Modify the spheres (simulated)
        modified_objects = [name for name in names if name.startswith("sphere")]

        # Return success result with context
        return ActionResultModel(
            success=True,
            message=f"Successfully modified {len(modified_objects)} spheres",
            prompt="You can now render the scene or add more objects.",
            context={
                "modified_objects": modified_objects,
                "applied_properties": {
                    "scale": scale,
                    "color": color
                },
                "scene_stats": {
                    "total_modified": len(modified_objects)
                }
            }
        )

    except Exception as e:
        # Handle unexpected errors
        logger.error(f"Error modifying spheres: {e}")
        return ActionResultModel(
            success=False,
            message=f"Failed to modify spheres: {e!s}",
            error=str(e),
            prompt="An unexpected error occurred. Please check the error message and try again.",
            context={"exception": str(e)}
        )


def process_result(result: ActionResultModel) -> None:
    """Process and display an action result.

    Args:
        result: The ActionResultModel to process

    """
    # Print a separator
    print("\n" + "-" * 80)

    # Print the result status
    status = "SUCCESS" if result.success else "FAILURE"
    print(f"RESULT STATUS: {status}")

    # Print the message
    print(f"MESSAGE: {result.message}")

    # Print the error if present
    if result.error:
        print(f"ERROR: {result.error}")

    # Print the prompt if present
    if result.prompt:
        print(f"PROMPT: {result.prompt}")

    # Print the context if present
    if result.context:
        print("CONTEXT:")
        for key, value in result.context.items():
            print(f"  {key}: {value}")

    # Print a separator
    print("-" * 80)


def main():
    """Run the example."""
    print("ActionResultModel Example")
    print("=======================")

    # Example 1: Successful creation
    print("\nExample 1: Creating 3 spheres (success)")
    result1 = create_spheres(count=3, radius=2.0)
    process_result(result1)

    # Example 2: Invalid parameter
    print("\nExample 2: Creating -5 spheres (invalid parameter)")
    result2 = create_spheres(count=-5, radius=1.0)
    process_result(result2)

    # Example 3: Memory limit error
    print("\nExample 3: Creating 200 spheres (memory limit)")
    result3 = create_spheres(count=200, radius=1.0)
    process_result(result3)

    # Example 4: Successful modification
    print("\nExample 4: Modifying spheres (success)")
    result4 = modify_spheres(names=["sphere1", "sphere2", "sphere3"], scale=2.0, color="blue")
    process_result(result4)

    # Example 5: Missing objects
    print("\nExample 5: Modifying non-existent spheres (not found)")
    result5 = modify_spheres(names=["cube1", "sphere1"], scale=1.5, color="green")
    process_result(result5)


if __name__ == "__main__":
    main()
