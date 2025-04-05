"""Tests for the Action adapter functions.

This module contains tests for the adapter functions that convert Action classes
to callable functions compatible with function-based APIs.
"""

# Import third-party modules
from pydantic import Field
import pytest

# Import local modules
from dcc_mcp_core.actions.adapter import create_function_adapter
from dcc_mcp_core.actions.adapter import create_function_adapters
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry


# Define test Action classes
class AdapterTestAction(Action):
    """Test Action for adapter testing."""

    name = "adapter_test_action"
    description = "Test Action for adapter"
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model for AdapterTestAction."""

        value: int = Field(description="Test value")
        optional_value: str = Field("default", description="Optional test value")

    class OutputModel(Action.OutputModel):
        """Output model for AdapterTestAction."""

        result: int = Field(description="Test result")

    def _execute(self) -> None:
        value = self.input.value

        self.output = self.OutputModel(result=value * 2, prompt=f"The result is {value * 2}")


@pytest.fixture
def clean_registry():
    """Fixture to provide a clean ActionRegistry for each test."""
    registry = ActionRegistry()
    registry._actions = {}  # Clear the registry
    registry.register(AdapterTestAction)  # Register test action
    return registry


def test_create_function_adapter(clean_registry):
    """Test creating a function adapter for an Action."""
    # Create adapter for the test action
    adapter = create_function_adapter("adapter_test_action")

    # Call the adapter
    result = adapter(value=42)

    # Check result
    assert result.success is True
    assert "Successfully executed adapter_test_action" in result.message
    assert "The result is 84" in result.prompt
    assert result.context["result"] == 84


def test_create_function_adapter_with_optional_params(clean_registry):
    """Test function adapter with optional parameters."""
    # Create adapter for the test action
    adapter = create_function_adapter("adapter_test_action")

    # Call the adapter with optional parameter
    result = adapter(value=42, optional_value="custom")

    # Check result
    assert result.success is True
    assert "Successfully executed adapter_test_action" in result.message
    assert "The result is 84" in result.prompt
    assert result.context["result"] == 84


def test_create_function_adapter_invalid_action(clean_registry):
    """Test function adapter for non-existent action."""
    # Create adapter for a non-existent action
    adapter = create_function_adapter("non_existent_action")

    # Call the adapter
    result = adapter(value=42)

    # Check result
    assert result.success is False
    assert "not found" in result.message
    assert result.error is not None


def test_create_function_adapter_invalid_params(clean_registry):
    """Test function adapter with invalid parameters."""
    # Create adapter for the test action
    adapter = create_function_adapter("adapter_test_action")

    # Call the adapter with invalid parameters should raise a validation error
    with pytest.raises(Exception) as excinfo:
        adapter(value="not_an_int")

    # Check that the error is a validation error
    assert "validation error" in str(excinfo.value).lower() or "invalid" in str(excinfo.value).lower()


def test_create_function_adapters(clean_registry):
    """Test creating function adapters for all registered Actions."""

    # Register another test action
    class AnotherAction(Action):
        name = "another_action"
        dcc = "test"

        class InputModel(Action.InputModel):
            text: str

        class OutputModel(Action.OutputModel):
            result: str = Field(description="Test result")

        def _execute(self) -> None:
            self.output = self.OutputModel(result=f"Processed: {self.input.text}")

    clean_registry.register(AnotherAction)

    # Create adapters for all actions
    adapters = create_function_adapters()

    # Check adapters
    assert len(adapters) == 2
    assert "adapter_test_action" in adapters
    assert "another_action" in adapters

    # Test both adapters
    result1 = adapters["adapter_test_action"](value=42)
    assert result1.success is True
    assert "Successfully executed adapter_test_action" in result1.message

    result2 = adapters["another_action"](text="test")
    assert result2.success is True
    assert "Successfully executed another_action" in result2.message
