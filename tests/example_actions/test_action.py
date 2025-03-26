"""Example Action class for testing purposes.

This module demonstrates how to create a custom Action class using the new class-based design.
"""

# Import built-in modules
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import Field

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel


class TestAction(Action):
    """A simple test action for demonstration purposes.

    This action demonstrates the basic structure and functionality of the new class-based
    Action design. It includes input validation, processing, and result generation.
    """

    name = "test_action"
    description = "A test action for demonstration purposes"
    tags: ClassVar[List[str]] = ["test", "example"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model for TestAction.

        Attributes:
            message: A message to include in the result
            count: Number of items to generate
            include_details: Whether to include additional details in the result

        """

        message: str = Field(default="Hello, World!", description="A message to include in the result")
        count: int = Field(default=1, description="Number of items to generate", ge=1, le=10)
        include_details: bool = Field(default=False, description="Whether to include additional details in the result")

    class OutputModel(Action.OutputModel):
        """Output model for TestAction.

        Attributes:
            items: List of generated items
            total_count: Total number of items generated
            details: Optional additional details

        """

        items: List[str] = Field(description="List of generated items")
        total_count: int = Field(description="Total number of items generated")
        details: Optional[Dict[str, Any]] = Field(default=None, description="Optional additional details")

    def process(self) -> ActionResultModel:
        """Process the action and return the result.

        Returns:
            ActionResultModel with the result of the action

        """
        # Get validated input
        input_data = self.input

        # Generate items
        items = [f"Item {i}: {input_data.message}" for i in range(1, input_data.count + 1)]

        # Prepare output data
        output_data = {"items": items, "total_count": len(items)}

        # Add details if requested
        if input_data.include_details:
            output_data["details"] = {
                "timestamp": "2023-01-01T00:00:00Z",
                "generator": "TestAction",
                "version": "1.0.0",
            }

        # Validate output against OutputModel
        output = self.OutputModel(**output_data)

        # Create and return result
        return ActionResultModel(
            success=True,
            message=f"Generated {len(items)} items successfully",
            prompt="You can now use these items for testing",
            context=output.model_dump(),
        )
