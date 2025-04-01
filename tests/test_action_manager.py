"""Tests for the ActionManager class with class-based Actions.

This module contains tests for the ActionManager's load_actions functionality with class-based Actions.
"""

# Import built-in modules
from typing import ClassVar
from typing import List
from unittest.mock import patch

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionResultModel


class TestAction(Action):
    """Test Action implementation."""

    name = "test_action"
    description = "A test action"
    version = "1.0.0"
    author = "Test Author"
    requires: ClassVar[List[str]] = ["test_dependency"]
    tags: ClassVar[List[str]] = ["test", "example"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Test input model."""

    def _execute(self) -> None:
        """Test execution implementation."""


class AdvancedAction(Action):
    """Advanced Action implementation."""

    name = "advanced_action"
    description = "An advanced test action"
    version = "2.0.0"
    author = "Advanced Author"
    requires: ClassVar[List[str]] = ["advanced_dependency"]
    tags: ClassVar[List[str]] = ["advanced", "example"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Advanced input model."""

    def _execute(self) -> None:
        """Advanced execution implementation."""


def test_action_manager_get_actions_info():
    """Test ActionManager.get_actions_info method with class-based Actions."""
    # Create ActionManager instance
    manager = ActionManager("test")

    # Mock the registry with test actions
    with patch.object(manager.registry, "list_actions") as mock_list_actions:
        mock_list_actions.return_value = [{"name": "test_action"}, {"name": "advanced_action"}]

        with patch.object(manager.registry, "get_action") as mock_get_action:

            def side_effect(action_name):
                if action_name == "test_action":
                    return TestAction
                elif action_name == "advanced_action":
                    return AdvancedAction
                return None

            mock_get_action.side_effect = side_effect

            # Call get_actions_info method
            result = manager.get_actions_info()

            # Verify result
            assert isinstance(result, ActionResultModel)
            assert result.success is True
            assert "Actions info retrieved for test" in result.message

            # Verify actions_info structure
            actions_info = result.context.get("actions")
            assert isinstance(actions_info, dict)
            assert len(actions_info) == 2

            # Verify action information
            assert "test_action" in actions_info
            assert actions_info["test_action"]["name"] == "test_action"
            assert actions_info["test_action"]["description"] == "A test action"

            assert "advanced_action" in actions_info
            assert actions_info["advanced_action"]["name"] == "advanced_action"
            assert actions_info["advanced_action"]["description"] == "An advanced test action"


def test_action_manager_call_action():
    """Test ActionManager.call_action method with class-based Actions."""
    # Create ActionManager instance
    manager = ActionManager("test")

    # Mock the registry with test actions
    with patch.object(manager.registry, "get_action") as mock_get_action:
        mock_get_action.return_value = TestAction

        # Mock the action instance
        with patch("dcc_mcp_core.actions.base.Action.process") as mock_process:
            mock_process.return_value = ActionResultModel(
                success=True, message="Action executed successfully", context={"test": True}
            )

            # Call action
            result = manager.call_action("test_action", param1="value1")

            # Verify result
            assert isinstance(result, ActionResultModel)
            assert result.success is True
            assert "executed successfully" in result.message
            assert result.context["test"] is True

            # Verify mock calls
            mock_get_action.assert_called_once_with("test_action")
            mock_process.assert_called_once()


def test_action_manager_call_nonexistent_action():
    """Test ActionManager.call_action method with a nonexistent action."""
    # Create ActionManager instance
    manager = ActionManager("test")

    # Mock the registry to return None for nonexistent action
    with patch.object(manager.registry, "get_action", return_value=None):
        # Call nonexistent action
        result = manager.call_action("nonexistent_action")

        # Verify result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "not found" in result.message
