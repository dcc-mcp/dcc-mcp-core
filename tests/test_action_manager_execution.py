"""Tests for the action execution functionality of the ActionManager class.

This module contains tests for the action execution methods of the ActionManager class.
"""

# Import built-in modules
from typing import ClassVar
from typing import List
from unittest.mock import MagicMock
from unittest.mock import patch

# Import local modules
# Import DCC-MCP-Core modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionResultModel


def test_action_manager_execute_action():
    """Test ActionManager.call_action method."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create a mock Action class
    class MockAction:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            pass

        def process(self):
            return ActionResultModel(
                success=True, message="Successfully executed test_action", context={"result": "Test result"}
            )

    # Mock the registry to return our mock Action class
    with patch.object(manager.registry, "get_action", return_value=MockAction):
        # Execute the action
        result = manager.call_action("test_action")

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates success
        assert result.success is True

        # Check that the message indicates success
        assert "Successfully executed test_action" in result.message

        # Check that the context contains the result
        assert result.context["result"] == "Test result"


def test_action_manager_execute_action_with_args():
    """Test ActionManager.call_action method with arguments."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create a mock Action class
    class MockAction:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}
            self.args = None

        def setup(self, **kwargs):
            self.args = kwargs

        def process(self):
            return ActionResultModel(
                success=True,
                message="Successfully executed test_action",
                context={"result": "Test result with args", "args": self.args},
            )

    # Define arguments
    args = {"arg1": "value1", "arg2": "value2"}

    # Mock the registry to return our mock Action class
    with patch.object(manager.registry, "get_action", return_value=MockAction):
        # Execute the action with arguments
        result = manager.call_action("test_action", **args)

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates success
        assert result.success is True

        # Check that the message indicates success
        assert "Successfully executed test_action" in result.message

        # Check that the context contains the result and arguments
        assert result.context["result"] == "Test result with args"
        assert result.context["args"] == args


def test_action_manager_execute_action_nonexistent_action():
    """Test ActionManager.call_action method with a nonexistent action."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Mock the registry to return None for nonexistent action
    with patch.object(manager.registry, "get_action", return_value=None):
        # Execute a nonexistent action
        result = manager.call_action("non_existent_action")

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates failure
        assert result.success is False

        # Check that the message indicates an error occurred
        assert "not found" in result.message

        # Check that the error contains the action name
        assert "non_existent_action" in result.message


def test_action_manager_execute_action_exception():
    """Test ActionManager.call_action method with an action that raises an exception."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create a mock Action class that raises an exception
    class MockActionWithException:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            self.kwargs = kwargs

        def process(self):
            # Simulate an exception during processing
            raise Exception("Test exception")

    # Mock the registry to return our mock Action class
    with patch.object(manager.registry, "get_action", return_value=MockActionWithException):
        # Execute the action
        result = manager.call_action("test_action")

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates failure
        assert result.success is False

        # Check that the message indicates an error occurred
        assert "Action test_action execution failed" in result.message

        # Check that the error contains the exception message
        assert "Test exception" in result.error


def test_action_manager_execute_action_validation_error():
    """Test ActionManager.call_action method with input validation error."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create a mock Action class with validation error
    class MockActionWithValidation:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            # Simulate validation error
            raise ValueError("Validation error: required parameter missing")

        def process(self):
            # This should not be called due to validation error
            return ActionResultModel(success=True, message="This should not be reached")

    # Mock the registry to return our mock Action class
    with patch.object(manager.registry, "get_action", return_value=MockActionWithValidation):
        # Execute the action
        result = manager.call_action("test_action")

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates failure
        assert result.success is False

        # Check that the message indicates a validation error
        assert "Error preparing action test_action" in result.message

        # Check that the error contains validation information
        assert "Validation error" in result.error


def test_action_result_model_error_handling():
    """Test the error handling in ActionResultModel."""
    # Create a new ActionResultModel with error
    result = ActionResultModel(
        success=False, message="Error occurred", error="This is an error message", context={"error_code": 404}
    )

    # Check basic properties
    assert result.success is False
    assert result.message == "Error occurred"
    assert result.error == "This is an error message"
    assert result.context["error_code"] == 404

    # Test string representation
    str_repr = str(result)
    assert "success=False" in str_repr
    assert "Error occurred" in str_repr


def test_get_actions_info_with_registered_actions():
    """Test get_actions_info method with registered actions."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create mock Action classes
    class TestAction1:
        name = "test_action1"
        description = "Test action 1"
        tags: ClassVar[List[str]] = ["test", "example"]
        dcc = "maya"
        order = 0

    class TestAction2:
        name = "test_action2"
        description = "Test action 2"
        tags: ClassVar[List[str]] = ["test", "advanced"]
        dcc = "maya"
        order = 1

    # Mock the registry's list_actions and get_action methods
    with patch.object(manager.registry, "list_actions") as mock_list_actions:
        mock_list_actions.return_value = [
            {
                "name": "test_action1",
                "internal_name": "test_action1",
                "description": "Test action 1",
                "tags": ["test", "example"],
                "dcc": "maya",
                "version": "1.0.0",
            },
            {
                "name": "test_action2",
                "internal_name": "test_action2",
                "description": "Test action 2",
                "tags": ["test", "advanced"],
                "dcc": "maya",
                "version": "1.0.0",
            },
        ]

        with patch.object(manager.registry, "get_action") as mock_get_action:
            # Define side effect to return different Action classes based on name
            def side_effect(action_name):
                if action_name == "test_action1":
                    return TestAction1
                elif action_name == "test_action2":
                    return TestAction2
                return None

            mock_get_action.side_effect = side_effect

            # Call get_actions_info
            result = manager.get_actions_info()

            # Verify result
            assert isinstance(result, ActionResultModel)
            assert result.success is True
            assert "Found" in result.message
            assert "actions for maya" in result.message

            # Verify actions info
            actions_info = result.context["actions"]
            assert len(actions_info) == 2

            # Verify action 1 info
            assert "test_action1" in actions_info
            assert actions_info["test_action1"]["name"] == "test_action1"
            assert actions_info["test_action1"]["description"] == "Test action 1"
            assert "test" in actions_info["test_action1"]["tags"]

            # Verify action 2 info
            assert "test_action2" in actions_info
            assert actions_info["test_action2"]["name"] == "test_action2"
            assert actions_info["test_action2"]["description"] == "Test action 2"
            assert "advanced" in actions_info["test_action2"]["tags"]


def test_middleware_integration():
    """Test integration with middleware."""
    # Create a new ActionManager instance
    manager = ActionManager("test", "maya")

    # Create a mock middleware
    mock_middleware = MagicMock()
    mock_middleware.process.return_value = ActionResultModel(
        success=True, message="Processed by middleware", context={"middleware_processed": True}
    )

    # Set the middleware
    manager.middleware = mock_middleware

    # Create a mock Action class
    class MockAction:
        name = "test_action"
        dcc = "maya"

        def __init__(self, context=None):
            self.context = context or {}

        def setup(self, **kwargs):
            pass

        def process(self):
            # This should not be called because middleware is used
            return ActionResultModel(success=True, message="This should not be reached")

    # Mock the registry to return our mock Action class
    with patch.object(manager.registry, "get_action", return_value=MockAction):
        # Execute the action
        result = manager.call_action("test_action")

        # Verify that middleware was used
        assert result.success is True
        assert "Processed by middleware" in result.message
        assert result.context["middleware_processed"] is True

        # Verify that middleware.process was called with an Action instance
        mock_middleware.process.assert_called_once()
        action_arg = mock_middleware.process.call_args[0][0]
        assert isinstance(action_arg, MockAction)
