"""Example action that prints information about the current DCC application.

This action is an example of a simple action that prints information about the current DCC application.

"""

# Import built-in modules
from typing import Any
from typing import Dict

__action_name__ = "Print Info"
__action_version__ = "1.0.0"
__action_description__ = "Print information about the current DCC application"
__action_author__ = "DCC-MCP-Core Team"
__action_requires__ = ["python"]

def print_info(context: Dict[str, Any]) -> Dict[str, Any]:
    """Print information about the current DCC application.

    Args:
        context (Dict[str, Any]): The context dictionary.

    Returns:
        Dict[str, Any]: The result dictionary.

    """
    return {"success": True, "message": "Info printed successfully"}
