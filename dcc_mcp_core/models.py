"""Pydantic models for DCC-MCP-Core action management.

This module defines the ActionResultModel for structured action execution results.
All other models have been moved to the new Action system.
"""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import Optional

# Import third-party modules
from pydantic import BaseModel
from pydantic import ConfigDict
from pydantic import Field


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

    # Note: For creating instances, use the factory functions in result_factory.py
    # Examples:
    #   from dcc_mcp_core.utils.result_factory import success_result, error_result, from_exception
    #   result = success_result("Operation completed", "Next, you can...", data=value)
    #   error = error_result("Operation failed", "Error details", "Try again with...")
    #   exception = from_exception(e, "Failed to process", include_traceback=True)

    def with_error(self, error: str) -> "ActionResultModel":
        """Create a new instance with error information.

        Args:
            error: Error message

        Returns:
            New ActionResultModel instance

        """
        return self.__class__(
            success=False, message=self.message, prompt=self.prompt, error=error, context=self.context
        )

    def with_context(self, **kwargs) -> "ActionResultModel":
        """Create a new instance with updated context.

        Args:
            **kwargs: Key-value pairs to add to context

        Returns:
            New ActionResultModel instance

        """
        new_context = dict(self.context)
        new_context.update(kwargs)
        return self.__class__(
            success=self.success, message=self.message, prompt=self.prompt, error=self.error, context=new_context
        )

    def to_dict(self) -> Dict[str, Any]:
        """Convert the model to a dictionary representation.

        This method provides a version-independent way to convert the model to a dictionary,
        compatible with both Pydantic v1 and v2.

        Returns:
            Dict[str, Any]: Dictionary representation of the model

        """
        try:
            # Try Pydantic v2 method first
            if hasattr(self, "model_dump"):
                return self.model_dump()
            # Fall back to Pydantic v1 method
            return self.dict()
        except Exception:
            # If all else fails, manually create a dictionary
            return {
                "success": self.success,
                "message": self.message,
                "prompt": self.prompt,
                "error": self.error,
                "context": self.context,
            }

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
                    "error": "Out of memory",
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
