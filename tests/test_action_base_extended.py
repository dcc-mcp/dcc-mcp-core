"""Extended tests for the Action base class in DCC-MCP-Core.

This module contains additional tests for the Action base class to improve code coverage.
"""

# Import built-in modules
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
from pydantic import Field
from pydantic import ValidationError
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel


class TestActionWithValidation(Action):
    """Test action class with input validation."""

    name = "test_action_validation"
    description = "A test action with input validation"
    tags: ClassVar[List[str]] = ["test", "validation"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model with validation."""

        value: int = Field(gt=0, description="A positive integer")

    class OutputModel(Action.OutputModel):
        """Output model."""

        result: int = Field(description="Result value")

    def _execute(self) -> None:
        """Execute the action."""
        self.output = self.OutputModel(result=self.input.value * 2)


class TestActionWithSetupContext(Action):
    """Test action class with custom setup context."""

    name = "test_action_setup_context"
    description = "A test action with custom setup context"
    tags: ClassVar[List[str]] = ["test", "setup"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model."""

        value: int = Field(description="A value")

    class OutputModel(Action.OutputModel):
        """Output model."""

        result: int = Field(description="Result value")
        context_value: Optional[str] = Field(None, description="Value from context")

    def _setup_context(self) -> None:
        """Set up additional context."""
        self.context["setup_called"] = True

    def _execute(self) -> None:
        """Execute the action."""
        self.output = self.OutputModel(result=self.input.value, context_value=self.context.get("context_key"))


class TestActionWithError(Action):
    """Test action class that raises an error during execution."""

    name = "test_action_error"
    description = "A test action that raises an error"
    tags: ClassVar[List[str]] = ["test", "error"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model."""

        pass

    class OutputModel(Action.OutputModel):
        """Output model."""

        pass

    def _execute(self) -> None:
        """Execute the action and raise an error."""
        raise ValueError("Test error")


class TestActionWithAsyncExecute(Action):
    """Test action class with async execute method."""

    name = "test_action_async"
    description = "A test action with async execute"
    tags: ClassVar[List[str]] = ["test", "async"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model."""

        value: int = Field(description="A value")

    class OutputModel(Action.OutputModel):
        """Output model."""

        result: int = Field(description="Result value")

    async def _execute_async(self) -> None:
        """Execute the action asynchronously."""
        self.output = self.OutputModel(result=self.input.value * 3)


def test_action_init_with_context():
    """Test Action initialization with context."""
    # Create a context
    context = {"key": "value"}

    # Create a new Action instance with context
    action = TestActionWithValidation(context=context)

    # Check that the context was set correctly
    assert action.context == context
    assert action.input is None
    assert action.output is None


def test_action_setup_with_valid_input():
    """Test Action setup with valid input."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Set up the action with valid input
    result = action.setup(value=10)

    # Check that setup returns self for method chaining
    assert result is action

    # Check that the input was validated and set
    assert action.input is not None
    assert action.input.value == 10


def test_action_setup_with_invalid_input():
    """Test Action setup with invalid input."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Set up the action with invalid input (value <= 0)
    with pytest.raises(ValidationError):
        action.setup(value=0)


def test_action_setup_context():
    """Test Action _setup_context method."""
    # Create a new Action instance
    action = TestActionWithSetupContext()

    # Set up the action
    action.setup(value=10)

    # Check that _setup_context was called
    assert action.context["setup_called"] is True


def test_action_process_success():
    """Test Action process method with successful execution."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Set up and process the action
    result = action.setup(value=10).process()

    # Check that the result is successful
    assert result.success is True
    assert result.message == "Successfully executed test_action_validation"
    assert result.error is None

    # Check that the output was set correctly
    assert action.output is not None
    assert action.output.result == 20


def test_action_process_error():
    """Test Action process method with an error during execution."""
    # Create a new Action instance
    action = TestActionWithError()

    # Set up and process the action
    result = action.setup().process()

    # Check that the result indicates failure
    assert result.success is False
    assert "Failed to execute test_action_error" in result.message
    assert result.error == "Test error"

    # Check that the output was not set
    assert action.output is None


def test_action_process_validation_error():
    """Test Action process method with validation error."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Process without setting up (input is None)
    result = action.process()

    # Check that the result indicates failure
    assert result.success is False
    assert "Failed to execute test_action_validation" in result.message
    assert "'NoneType' object has no attribute 'value'" in result.error


@pytest.mark.asyncio
async def test_action_process_async():
    """Test Action process_async method."""
    # Create a new Action instance
    action = TestActionWithAsyncExecute()

    # Set up the action
    action.setup(value=10)

    # Process the action asynchronously
    result = await action.process_async()

    # Check that the result is successful
    assert result.success is True
    assert result.message == "Successfully executed test_action_async"
    assert result.error is None

    # Check that the output was set correctly
    assert action.output is not None
    assert action.output.result == 30


@pytest.mark.asyncio
async def test_action_process_async_without_async_execute():
    """Test Action process_async method for an action without _execute_async."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Set up the action
    action.setup(value=10)

    # Process the action asynchronously
    result = await action.process_async()

    # Check that the result is successful
    assert result.success is True
    assert result.message == "Successfully executed test_action_validation"
    assert result.error is None

    # Check that the output was set correctly
    assert action.output is not None
    assert action.output.result == 20


@pytest.mark.asyncio
async def test_action_process_async_error():
    """Test Action process_async method with an error during execution."""
    # Create a new Action instance
    action = TestActionWithError()

    # Set up and process the action asynchronously
    result = await action.process_async()

    # Check that the result indicates failure
    assert result.success is False
    assert "Failed to execute test_action_error" in result.message
    assert result.error == "Test error"

    # Check that the output was not set
    assert action.output is None


def test_action_with_context():
    """Test Action with context data."""
    # Create a context with data
    context = {"context_key": "context_value"}

    # Create a new Action instance with context
    action = TestActionWithSetupContext(context=context)

    # Set up and process the action
    action.setup(value=10).process()

    # Check that the context data was used in the output
    assert action.output.context_value == "context_value"


def test_action_validate_input_with_model():
    """Test Action validate_input method with a model."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Validate input
    input_model = action.validate_input(value=10)

    # Check that the input model was created correctly
    assert input_model.value == 10


def test_action_validate_input_with_invalid_data():
    """Test Action validate_input method with invalid data."""
    # Create a new Action instance
    action = TestActionWithValidation()

    # Validate invalid input
    with pytest.raises(ValidationError):
        action.validate_input(value=-1)
