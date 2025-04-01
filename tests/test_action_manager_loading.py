"""Tests for the action loading functionality of the ActionManager class.

This module contains tests for the action loading methods of the ActionManager class.
"""

# Import built-in modules
import os
from typing import ClassVar
from typing import List
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionResultModel


@pytest.fixture
def test_actions_dir(tmp_path):
    """Fixture to create a temporary directory with test action files."""
    # Create a test directory structure
    actions_dir = tmp_path / "actions"
    actions_dir.mkdir()

    # Create a test action file
    action_file = actions_dir / "test_action.py"
    action_file.write_text('''
# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

class TestAction(Action):
    """Test action class."""

    name = "test_action"
    description = "A test action"
    version = "1.0.0"
    author = "Test Author"
    requires = ["dependency1", "dependency2"]
    tags = ["test", "example"]
    dcc = "maya"

    class InputModel(Action.InputModel):
        """Input model for TestAction."""
        pass

    def _execute(self) -> None:
        """Execute the action."""
        self.output = self.OutputModel()
''')

    # Return the path to the actions directory
    return actions_dir


def test_action_manager_discover_actions(test_actions_dir):
    """Test ActionManager's ability to discover actions from a path."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Register the test actions directory
    manager.register_action_path(str(test_actions_dir))

    # Mock the registry's discover_actions_from_path method
    with patch.object(manager.registry, "discover_actions_from_path") as mock_discover:
        # Call refresh_actions to discover actions
        manager.refresh_actions(force=True)

        # Verify that discover_actions_from_path was called
        assert mock_discover.called
        # 检查是否使用了正确的路径参数
        call_args = mock_discover.call_args[1]
        assert "path" in call_args
        assert str(test_actions_dir) in call_args["path"]


def test_action_manager_get_actions_info(test_actions_dir):
    """Test ActionManager's get_actions_info method."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Create a mock Action class
    class MockAction:
        name = "mock_action"
        description = "A mock action"
        tags: ClassVar[List[str]] = ["mock", "test"]
        dcc = "maya"
        order = 0

    # Mock the registry's list_actions and get_action methods
    with patch.object(manager.registry, "list_actions") as mock_list_actions:
        mock_list_actions.return_value = [{"name": "mock_action"}]

        with patch.object(manager.registry, "get_action") as mock_get_action:
            mock_get_action.return_value = MockAction

            # Call get_actions_info
            result = manager.get_actions_info()

            # Verify result
            assert isinstance(result, ActionResultModel)
            assert result.success is True
            assert "Actions info retrieved" in result.message

            # Verify actions info
            actions_info = result.context["actions"]
            assert len(actions_info) == 1
            assert "mock_action" in actions_info
            assert actions_info["mock_action"]["name"] == "mock_action"
            assert actions_info["mock_action"]["description"] == "A mock action"
            assert "mock" in actions_info["mock_action"]["tags"]


def test_action_manager_load_module_exception():
    """Test ActionManager's handling of exceptions during module loading."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Create a test action path
    action_path = "/path/to/nonexistent/action.py"

    # Register the action path
    manager.register_action_path(os.path.dirname(action_path))

    # Mock the registry's discover_actions_from_path method to raise an exception
    with patch.object(manager.registry, "discover_actions_from_path", side_effect=Exception("Test exception")):
        # Call refresh_actions - this should not raise an exception
        # even though discover_actions_from_path raises one
        try:
            manager.refresh_actions(force=True)
            # If we get here, the exception was handled properly
            exception_handled = True
        except Exception:
            # If we get here, the exception was not handled properly
            exception_handled = False

        # Verify that the exception was handled properly
        assert exception_handled, "Exception was not handled properly by ActionManager.refresh_actions"
