"""Additional tests for ActionManager to improve code coverage.

This module contains tests specifically designed to improve code coverage for
the ActionManager class.
"""

# Import built-in modules
import os
import time
from typing import ClassVar
from typing import List
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.middleware import Middleware


class TestAction(Action):
    """Test Action implementation."""

    name = "test_action"
    description = "A test action"
    tags: ClassVar[List[str]] = ["test", "example"]
    dcc = "test"

    class InputModel(Action.InputModel):
        """Test input model."""

    def _execute(self) -> None:
        """Test execution."""
        self.output = self.OutputModel()


class TestMiddleware(Middleware):
    """Test middleware implementation."""

    def process(self, action):
        """Process the action."""
        # Add test data to context
        action.context["middleware_test"] = True
        # Process the action directly
        return action.process()


@pytest.fixture
def action_manager():
    """Create a test ActionManager instance."""
    manager = ActionManager("test", "test_dcc")
    # Register a test action
    manager.registry.register(TestAction)
    return manager


def test_action_manager_auto_refresh(action_manager):
    """Test ActionManager auto refresh functionality."""
    # Enable auto refresh with a short interval
    action_manager._auto_refresh = True
    action_manager._refresh_interval = 0.1
    action_manager._last_refresh_time = time.time() - 1  # Set last refresh to 1 second ago

    # Mock refresh_actions method
    with patch.object(action_manager, "refresh_actions") as mock_refresh:
        # Call the check_auto_refresh method directly
        action_manager._check_auto_refresh()

        # Verify that refresh_actions was called
        assert mock_refresh.called


def test_action_manager_discover_actions_from_package(action_manager):
    """Test ActionManager.discover_actions_from_package method."""
    # Mock the registry's discover_actions_from_package method
    with patch.object(action_manager.registry, "discover_actions_from_package") as mock_discover:
        # Call discover_actions_from_package
        action_manager.discover_actions_from_package("test_package")

        # Verify that the registry method was called with correct parameters
        mock_discover.assert_called_once_with(package_name="test_package", dcc_name=action_manager.dcc_name)


def test_action_manager_discover_actions_from_path(action_manager):
    """Test ActionManager.discover_actions_from_path method."""
    # Mock the registry's discover_actions_from_path method
    with patch.object(action_manager.registry, "discover_actions_from_path") as mock_discover:
        # Call discover_actions_from_path
        action_manager.discover_actions_from_path("/test/path")

        # Verify that the registry method was called with correct parameters
        mock_discover.assert_called_once_with(
            path="/test/path", dependencies=action_manager.context, dcc_name=action_manager.dcc_name
        )


def test_action_manager_middleware_chain(action_manager):
    """Test ActionManager middleware chain functionality."""
    # Get middleware chain
    chain = action_manager.middleware_chain
    assert chain is not None

    # Register the test action
    action_manager.registry.register(TestAction)

    # Create a test action instance directly
    action = TestAction(context=action_manager.context)

    # Add middleware
    action_manager.add_middleware(TestMiddleware)

    # Call the middleware directly to test it
    middleware = TestMiddleware()
    middleware.process(action)

    # Verify that middleware was applied to the action's context
    assert "middleware_test" in action.context
    assert action.context["middleware_test"] is True


def test_action_manager_refresh_actions(action_manager):
    """Test ActionManager.refresh_actions method."""
    # Mock the registry's discover_actions_from_path method
    with patch.object(action_manager.registry, "discover_actions_from_path") as mock_discover:
        # Set up a test path
        test_path = os.path.join("test", "path")

        # Call refresh_actions with a custom path
        action_manager.refresh_actions(action_paths=[test_path], force=True)

        # Verify that discover_actions_from_path was called for the test path
        mock_discover.assert_called_with(
            path=test_path, dependencies=action_manager.context, dcc_name=action_manager.dcc_name
        )


def test_action_manager_update_context(action_manager):
    """Test ActionManager._update_context method."""
    # Initial context
    initial_context = action_manager.context.copy()

    # Update context with new data
    new_context = {"test_key": "test_value"}
    action_manager._update_context(new_context)

    # Verify that context was updated
    assert "test_key" in action_manager.context
    assert action_manager.context["test_key"] == "test_value"

    # Verify that original context data is preserved
    for key, value in initial_context.items():
        assert action_manager.context[key] == value


def test_action_manager_get_action_class(action_manager):
    """Test ActionManager._get_action_class method."""
    # Register the test action in the registry
    action_manager.registry.register(TestAction)

    # Get action class for a registered action
    action_class = action_manager._get_action_class("test_action")
    assert action_class is TestAction

    # Get action class for a non-existent action
    action_class = action_manager._get_action_class("non_existent_action")
    assert action_class is None


def test_action_manager_create_action_instance(action_manager):
    """Test ActionManager._create_action_instance method."""
    # Create an instance of a registered action
    action_instance = action_manager._create_action_instance(TestAction)

    # Verify that the instance was created correctly
    assert isinstance(action_instance, TestAction)
    assert action_instance.context == action_manager.context


def test_action_manager_check_auto_refresh(action_manager):
    """Test ActionManager._check_auto_refresh method."""
    # Enable auto refresh with a short interval
    action_manager._auto_refresh = True
    action_manager._refresh_interval = 0.1

    # Set last refresh time to now
    current_time = time.time()
    action_manager._last_refresh_time = current_time

    # Check auto refresh (should not refresh yet)
    with patch.object(action_manager, "refresh_actions") as mock_refresh:
        action_manager._check_auto_refresh()
        assert not mock_refresh.called

    # Set last refresh time to 1 second ago
    action_manager._last_refresh_time = current_time - 1

    # Check auto refresh (should refresh now)
    with patch.object(action_manager, "refresh_actions") as mock_refresh:
        action_manager._check_auto_refresh()
        assert mock_refresh.called
