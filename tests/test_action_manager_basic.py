"""Tests for the basic functionality of the ActionManager class.

This module contains tests for the initialization and core functionality of the ActionManager class.
"""

# Import built-in modules
from unittest.mock import MagicMock
from unittest.mock import patch

# Import local modules
# Import DCC-MCP-Core modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.models import ActionResultModel


def test_action_manager_init():
    """Test ActionManager initialization."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Check that the manager has the correct DCC name
    assert manager.dcc_name == "maya"

    # Check that the manager has initialized registry and action paths
    assert hasattr(manager, "registry")
    assert isinstance(manager._action_paths, list)
    assert len(manager._action_paths) == 0


def test_create_action_manager():
    """Test create_action_manager function."""
    # Create a new ActionManager instance
    manager = create_action_manager("maya", load_env_paths=False)

    # Check that the manager has the correct DCC name
    assert manager.dcc_name == "maya"

    # Check that the manager is registered in _action_managers
    # Import local modules
    from dcc_mcp_core.actions.manager import _action_managers

    assert "maya" in _action_managers
    assert _action_managers["maya"] == manager


def test_get_action_manager():
    """Test get_action_manager function."""
    # Create a new ActionManager instance
    create_manager = create_action_manager("maya", load_env_paths=False)

    # Get the manager using get_action_manager
    get_manager = get_action_manager("maya")

    # Check that the managers are the same
    assert create_manager == get_manager

    # Check that get_action_manager creates a new manager for non-existent DCC
    non_existent_manager = get_action_manager("non_existent_dcc")
    assert non_existent_manager.dcc_name == "non_existent_dcc"


def test_get_actions_info_empty(cleanup_action_managers):
    """Test get_actions_info method when no actions are loaded."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Get actions info
    result = manager.get_actions_info()

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates success
    assert result.success is True

    # Check that the message indicates actions info retrieved
    assert "Actions info retrieved" in result.message

    # Check that the context contains the DCC name and empty actions dictionary
    assert result.context["dcc_name"] == "maya"
    assert isinstance(result.context["actions"], dict)
    assert len(result.context["actions"]) == 0


def test_register_action_path():
    """Test register_action_path method."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Check that the initial action paths list is empty
    assert manager._action_paths == []

    # Register action paths
    test_paths = ["/path/to/actions1", "/path/to/actions2"]
    for path in test_paths:
        manager.register_action_path(path)

    # Check that the action paths are set correctly
    assert manager._action_paths == test_paths

    # Test registering a duplicate path (should not add it again)
    manager.register_action_path(test_paths[0])
    assert manager._action_paths == test_paths


@patch("dcc_mcp_core.actions.manager.ActionManager._discover_actions_from_path")
def test_refresh_actions(mock_discover_actions):
    """Test refresh_actions method."""
    # Create a new ActionManager instance
    manager = ActionManager("maya", auto_refresh=False, load_env_paths=False)

    # Register action paths
    test_paths = ["/path/to/actions1", "/path/to/actions2"]
    for path in test_paths:
        manager.register_action_path(path)

    # Call refresh_actions
    manager.refresh_actions(force=True)

    # Verify that _discover_actions_from_path was called for each path
    assert mock_discover_actions.call_count == len(test_paths)
    for path in test_paths:
        mock_discover_actions.assert_any_call(path)


def test_call_action():
    """Test call_action method."""

    # Create a mock Action class
    class MockAction:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            self.kwargs = kwargs
            return self

        def process(self):
            return ActionResultModel(
                success=True, message="Successfully executed test_action", context={"args": self.kwargs}
            )

    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Mock the registry's get_action method
    manager.registry.get_action = MagicMock(return_value=MockAction)

    # Call action with arguments
    result = manager.call_action("test_action", param1="value1", param2=42)

    # Verify the result
    assert result.success is True
    assert "Successfully executed test_action" in result.message
    assert result.context["args"] == {"param1": "value1", "param2": 42}

    # Verify that get_action was called correctly
    manager.registry.get_action.assert_called_once_with("test_action")


def test_call_action_with_registry_get_action_returning_none():
    """Test call_action method when registry.get_action returns None."""

    # Create a mock Action class
    class MockAction:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            self.kwargs = kwargs
            return self

        def process(self):
            return ActionResultModel(
                success=True, message="Successfully executed test_action", context={"args": self.kwargs}
            )

    # Create a new ActionManager instance
    manager = ActionManager("maya", load_env_paths=False)

    # Mock the registry's get_action method
    manager.registry.get_action = MagicMock(return_value=None)

    # Call action with arguments
    result = manager.call_action("test_action", param1="value1", param2=42)

    # Verify the result
    assert result.success is False
    assert "Action test_action not found" in result.message
    assert result.context == {}

    # Verify that get_action was called correctly
    manager.registry.get_action.assert_called_once_with("test_action")
