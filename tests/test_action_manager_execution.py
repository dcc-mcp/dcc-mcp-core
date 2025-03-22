"""Tests for the action execution functionality of the ActionManager class.

This module contains tests for the action execution methods of the ActionManager class.
"""

# Import built-in modules
import os
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
# Import DCC-MCP-Core modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.models import FunctionModel
from dcc_mcp_core.models import ParameterModel


def test_action_manager_execute_action():
    """Test ActionManager.call_action_function method."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.test_function = MagicMock(return_value="Test result")

    # Add the mock module to the manager
    manager._action_modules['test_action'] = mock_module
    manager._actions['test_action'] = {'test_function': mock_module.test_function}

    # Execute the function
    result = manager.call_action_function('test_action', 'test_function')

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates success
    assert result.success is True

    # Check that the message indicates success
    assert "Successfully called" in result.message

    # Check that the context contains the result
    assert result.context['result'] == "Test result"

    # Check that the function was called
    mock_module.test_function.assert_called_once()


def test_action_manager_execute_action_with_args():
    """Test ActionManager.call_action_function method with arguments."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.test_function = MagicMock(return_value="Test result with args")

    # Add the mock module to the manager
    manager._action_modules['test_action'] = mock_module
    manager._actions['test_action'] = {'test_function': mock_module.test_function}

    # Define arguments
    args = {'arg1': 'value1', 'arg2': 'value2'}

    # Execute the function with arguments
    result = manager.call_action_function('test_action', 'test_function', **args)

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates success
    assert result.success is True

    # Check that the message indicates success
    assert "Successfully called" in result.message

    # Check that the context contains the result
    assert result.context['result'] == "Test result with args"

    # Check that the function was called with the correct arguments
    mock_module.test_function.assert_called_once_with(**args)


def test_action_manager_execute_action_nonexistent_action():
    """Test ActionManager.call_action_function method with a nonexistent action."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Execute a nonexistent action
    result = manager.call_action_function('non_existent_action', 'test_function')

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates failure
    assert result.success is False

    # Check that the message indicates an error occurred
    assert "Failed to call" in result.message

    # Check that the error contains the action name
    assert "non_existent_action" in result.error


def test_action_manager_execute_action_nonexistent_function():
    """Test ActionManager.call_action_function method with a nonexistent function."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module without the non_existent_function
    mock_module = MagicMock(spec=['test_function'])  # Only test_function is allowed
    mock_module.test_function = MagicMock(return_value="Test result")

    # Add the mock module to the manager
    manager._action_modules['test_action'] = mock_module
    manager._actions['test_action'] = {'test_function': mock_module.test_function}

    # Execute a nonexistent function
    result = manager.call_action_function('test_action', 'non_existent_function')

    # Print result for debugging
    print(f"Result: {result}")
    print(f"Success: {result.success}")
    print(f"Message: {result.message}")
    print(f"Error: {result.error}")

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates failure
    assert result.success is False

    # Check that the message indicates an error occurred
    assert "Failed to call" in result.message

    # Check that the error contains the function name
    assert "non_existent_function" in result.error


def test_action_manager_execute_action_exception():
    """Test ActionManager.call_action_function method with a function that raises an exception."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module with a function that raises an exception
    mock_module = MagicMock()
    mock_module.test_function = MagicMock(side_effect=Exception("Test exception"))

    # Add the mock module to the manager
    manager._action_modules['test_action'] = mock_module
    manager._actions['test_action'] = {'test_function': mock_module.test_function}

    # Execute the function
    result = manager.call_action_function('test_action', 'test_function')

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates failure
    assert result.success is False

    # Check that the message indicates an error occurred
    assert "Failed to call" in result.message

    # Check that the error contains the exception message
    assert "Test exception" in result.error


def test_get_action_info_result_structure():
    """Test the structure of the ActionResultModel returned by get_action_info."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock action model
    action_model = ActionModel(
        name="test_action",
        version="1.0.0",
        description="Test action for testing",
        author="DCC-MCP-Core",
        dcc="maya",  # Add required dcc field
        file_path="/path/to/test_action.py",  # Add required file_path field
        functions={
            "test_function": FunctionModel(
                name="test_function",
                description="Test function",
                parameters=[
                    ParameterModel(
                        name="param1",
                        description="Test parameter",
                        type="str",
                        type_hint="str",  # Add required type_hint field
                        default="default"
                    )
                ]
            )
        }
    )

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.__action_name__ = "test_action"
    mock_module.__action_version__ = "1.0.0"
    mock_module.__action_description__ = "Test action for testing"
    mock_module.__action_author__ = "DCC-MCP-Core"
    mock_module.test_function = MagicMock(return_value="Test result")

    # Set internal attributes directly
    manager._action_modules = {"test_action": mock_module}
    manager._actions = {"test_action": {"test_function": mock_module.test_function}}

    # Mock the create_action_model function
    with patch('dcc_mcp_core.actions.manager.create_action_model', return_value=action_model):
        # Call the get_action_info method
        result = manager.get_action_info('test_action')

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates success
        assert result.success is True

        # Check that the message indicates success
        assert "found" in result.message

        # Check that the context contains the result
        assert 'result' in result.context

        # Check that the result in the context is an ActionModel
        assert isinstance(result.context['result'], ActionModel)

        # Check the attributes of the ActionModel
        action_result = result.context['result']
        assert action_result.name == "test_action"
        assert action_result.version == "1.0.0"
        assert action_result.description == "Test action for testing"
        assert action_result.author == "DCC-MCP-Core"
        assert "test_function" in action_result.functions


def test_get_actions_info_result_structure():
    """Test the structure of the ActionResultModel returned by get_actions_info."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock action model
    action_model = ActionModel(
        name="test_action",
        version="1.0.0",
        description="Test action for testing",
        author="DCC-MCP-Core",
        dcc="maya",
        file_path="/path/to/test_action.py",
        functions={}
    )

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.__action_name__ = "test_action"
    mock_module.__action_version__ = "1.0.0"
    mock_module.__action_description__ = "Test action for testing"
    mock_module.__action_author__ = "DCC-MCP-Core"

    # Set internal attributes directly
    manager._action_modules = {"test_action": mock_module}
    manager._actions = {"test_action": {}}

    # Mock the get_action_info_cached method
    mock_result = ActionResultModel(
        success=True,
        message="Action 'test_action' found",
        context={'result': action_model}
    )
    with patch.object(manager, 'get_action_info_cached', return_value=mock_result):
        # Mock the create_actions_info_model function
        # Import local modules
        from dcc_mcp_core.models import ActionsInfoModel
        actions_info_model = ActionsInfoModel(
            dcc_name="maya",
            actions={"test_action": action_model}
        )
        with patch('dcc_mcp_core.actions.manager.create_actions_info_model', return_value=actions_info_model):
            # Call the get_actions_info method
            result = manager.get_actions_info()

            # Check that the result is an ActionResultModel
            assert isinstance(result, ActionResultModel)

            # Check that the result indicates success
            assert result.success is True

            # Check that the message indicates success
            assert "Actions info retrieved" in result.message

            # Check that the context contains the result
            assert 'result' in result.context

            # Check that the result in the context has an actions attribute
            assert hasattr(result.context['result'], 'actions')

            # Check that the actions attribute is a dictionary containing the action
            actions = result.context['result'].actions
            assert "test_action" in actions

            # Check the attributes of the action
            action_result = actions["test_action"]
            assert action_result.name == "test_action"
            assert action_result.version == "1.0.0"
            assert action_result.description == "Test action for testing"
            assert action_result.author == "DCC-MCP-Core"


def test_action_result_model_error_handling():
    """Test the error handling in ActionResultModel."""
    manager = ActionManager('maya')

    result = manager.get_action_info('nonexistent_action')

    assert isinstance(result, ActionResultModel)

    assert result.success is False

    assert "not loaded or does not exist" in result.error

    assert "not found" in result.message


def test_get_action_info_nonexistent_action():
    """Test get_action_info method with a nonexistent action."""
    manager = ActionManager('maya')

    manager._action_modules = {}
    manager._actions = {}

    result = manager.get_action_info('nonexistent_action')

    assert isinstance(result, ActionResultModel)

    assert result.success is False

    assert "not found" in result.message
    assert result.error is not None
    assert "not loaded or does not exist" in result.error

    assert result.context is None or 'result' not in result.context


def test_get_action_info_cached():
    """Test the caching functionality of get_action_info_cached method."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.__action_name__ = "test_action"
    mock_module.__action_version__ = "1.0.0"
    mock_module.__action_description__ = "Test action for testing"
    mock_module.__action_author__ = "DCC-MCP-Core"
    mock_module.test_function = MagicMock(return_value="Test result")

    # Set internal attributes directly
    manager._action_modules = {"test_action": mock_module}
    manager._actions = {"test_action": {"test_function": mock_module.test_function}}

    # Create a mock ActionModel
    action_model = ActionModel(
        name="test_action",
        version="1.0.0",
        description="Test action for testing",
        author="DCC-MCP-Core",
        functions={},
        dcc="maya",
        file_path="/path/to/test_action.py"
    )

    # Mock the create_action_model function
    with patch('dcc_mcp_core.actions.manager.create_action_model', return_value=action_model) as mock_create_model:
        # First call to get_action_info_cached
        result1 = manager.get_action_info_cached('test_action')

        # Second call to get_action_info_cached
        result2 = manager.get_action_info_cached('test_action')

        # Verify create_action_model is called only once
        mock_create_model.assert_called_once()

        # Verify both calls return the same result
        assert result1 is result2

        # Verify the result is an ActionResultModel
        assert isinstance(result1, ActionResultModel)
        assert result1.success is True


def test_invalidate_action_cache():
    """Test the invalidate_action_cache method."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Create a mock module with a test function
    mock_module = MagicMock()
    mock_module.__action_name__ = "test_action"
    mock_module.__action_version__ = "1.0.0"
    mock_module.__action_description__ = "Test action for testing"
    mock_module.__action_author__ = "DCC-MCP-Core"

    # Set internal attributes directly
    manager._action_modules = {"test_action": mock_module}
    manager._actions = {"test_action": {}}

    # Create a mock ActionModel
    action_model = ActionModel(
        name="test_action",
        version="1.0.0",
        description="Test action for testing",
        author="DCC-MCP-Core",
        functions={},
        dcc="maya",
        file_path="/path/to/test_action.py"
    )

    # Mock the create_action_model function
    with patch('dcc_mcp_core.actions.manager.create_action_model', return_value=action_model) as mock_create_model:
        # First call to get_action_info_cached
        result1 = manager.get_action_info_cached('test_action')

        # Verify create_action_model is called once
        assert mock_create_model.call_count == 1

        # Invalidate the cache
        manager.invalidate_action_cache()

        # Second call to get_action_info_cached
        result2 = manager.get_action_info_cached('test_action')

        # Verify create_action_model is called twice
        assert mock_create_model.call_count == 2

        # Verify both results are successful
        assert result1.success is True
        assert result2.success is True


def test_get_actions_info_empty():
    """Test get_actions_info method when no actions are loaded."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Ensure internal dictionaries are empty
    manager._action_modules = {}
    manager._actions = {}

    # Mock the create_actions_info_model function
    # Import local modules
    from dcc_mcp_core.models import ActionsInfoModel
    empty_model = ActionsInfoModel(dcc_name="maya", actions={})
    with patch('dcc_mcp_core.actions.metadata.create_actions_info_model', return_value=empty_model):
        # Call the get_actions_info method
        result = manager.get_actions_info()

        # Check that the result is an ActionResultModel
        assert isinstance(result, ActionResultModel)

        # Check that the result indicates success
        assert result.success is True

        # Check that the message indicates success
        assert "retrieved" in result.message

        # Check that the context contains the result
        assert 'result' in result.context

        # Check that the result in the context is an ActionsInfoModel
        assert isinstance(result.context['result'], ActionsInfoModel)

        # Check that the ActionsInfoModel is empty
        assert len(result.context['result'].actions) == 0
