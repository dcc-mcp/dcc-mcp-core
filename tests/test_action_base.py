"""Tests for the Action base class.

This module contains tests for the Action base class functionality,
including input validation, execution, and error handling.
"""

# Import built-in modules
from typing import ClassVar
from typing import List

# Import third-party modules
from pydantic import Field
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action


class TestAction(Action):
    """Test action class."""

    name = "test_action"
    description = "Action for testing"
    tags: ClassVar[List[str]] = ["test", "example"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Test input model."""

        value: int = Field(description="Test value")
        optional_value: str = Field("default", description="Optional test value")

    class OutputModel(Action.OutputModel):
        """Test output model."""

        result: int = Field(description="Test result")
        processed: bool = Field(description="Whether processing was done")

    def _execute(self) -> None:
        """Test execution implementation."""
        # Get input parameters
        value = self.input.value

        # Set output
        self.output = self.OutputModel(result=value * 2, processed=True, prompt=f"The result is {value * 2}")


class ErrorAction(Action):
    """Action that raises an error during execution."""

    name = "error_action"

    class InputModel(Action.InputModel):
        """Input model for ErrorAction."""

        value: int

    def _execute(self) -> None:
        raise ValueError("Test error")


class IncompleteAction(Action):
    """Action with incomplete implementation."""

    name = "incomplete_action"

    class InputModel(Action.InputModel):
        """Input model for IncompleteAction."""

        value: int


def test_action_metadata():
    """Test that Action metadata is correctly defined."""
    assert TestAction.name == "test_action"
    assert TestAction.description == "Action for testing"
    assert TestAction.tags == ["test", "example"]
    assert TestAction.dcc == "test"


def test_action_input_validation_success():
    """Test successful input validation."""
    action = TestAction()

    # Valid input
    input_model = action.validate_input(value=42)
    assert isinstance(input_model, TestAction.InputModel)
    assert input_model.value == 42
    assert input_model.optional_value == "default"

    # Valid input with optional parameter
    input_model = action.validate_input(value=42, optional_value="custom")
    assert isinstance(input_model, TestAction.InputModel)
    assert input_model.value == 42
    assert input_model.optional_value == "custom"


def test_action_input_validation_failure():
    """Test failed input validation."""
    action = TestAction()

    # Missing required parameter
    with pytest.raises(Exception):
        action.validate_input()

    # Invalid type
    with pytest.raises(Exception):
        action.validate_input(value="not_an_int")


def test_action_process_success():
    """Test successful action processing."""
    action = TestAction()

    # Setup with valid input and then process
    action.setup(value=42)
    result = action.process()

    # Check result
    assert result.success is True
    assert "Successfully executed test_action" in result.message
    assert "The result is 84" in result.prompt
    assert result.context["result"] == 84
    assert result.context["processed"] is True


def test_action_process_validation_failure():
    """Test action processing with invalid input."""
    action = TestAction()

    # Do not setup and directly call process should fail
    # But now process method will catch the exception and return ActionResultModel, so we need to check the return value
    result = action.process()

    # Verify that the result is failed
    assert result.success is False
    assert "Failed to execute test_action" in result.message
    assert "'NoneType' object has no attribute" in result.error


def test_action_process_execution_error():
    """Test action processing with execution error."""
    action = ErrorAction()

    # Setup with valid input and then process
    action.setup(value=42)
    result = action.process()

    # Check result
    assert result.success is False
    assert "Failed to execute error_action" in result.message
    assert "Test error" in result.error
    assert "traceback" in result.context


def test_action_execute_not_implemented():
    """Test that _execute must be implemented."""
    action = IncompleteAction()

    # Setup with valid input
    action.setup(value=42)

    # _execute should raise NotImplementedError
    with pytest.raises(NotImplementedError):
        action._execute()
