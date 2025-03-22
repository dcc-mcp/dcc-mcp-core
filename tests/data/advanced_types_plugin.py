"""A test plugin with advanced type annotations.

This plugin demonstrates the use of complex type annotations.
"""

# Import built-in modules
from typing import Callable
from typing import List
from typing import Optional
from typing import Tuple

# Plugin metadata
__action_name__ = "advanced_types_plugin"
__action_version__ = "1.0.0"
__action_description__ = "Plugin with advanced type annotations"
__action_author__ = "Test Author"


def process_complex_data(
    data: List[dict],
    options: Optional[dict] = None,
    callback: Optional[Callable] = None
) -> Tuple[bool, dict]:
    """Process complex data with various options.

    Args:
        data: A list of dictionaries to process
        options: Optional processing options
        callback: Optional callback function to call after processing

    Returns:
        A tuple containing success status and result data

    """
    result = {
        "processed_items": len(data),
        "items": data
    }

    # Apply options if provided
    if options:
        result["options_applied"] = list(options.keys())

    # Call callback if provided
    if callback and callable(callback):
        try:
            callback_result = callback(result)
            result["callback_result"] = callback_result
        except Exception as e:
            result["callback_error"] = str(e)

    return True, result


async def async_operation(task_id: str) -> dict:
    """Perform an asynchronous operation (for testing async function handling).

    Args:
        task_id: ID of the task to process

    Returns:
        Result of the async operation

    """
    # This is just for testing async function signatures
    return {
        "task_id": task_id,
        "status": "completed"
    }
