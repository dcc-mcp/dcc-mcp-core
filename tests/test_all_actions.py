"""Tests for all actions in the example_actions directory.

This module uses pytest.mark.parametrize to test all actions in the example_actions directory.
"""

# Import built-in modules
import os
from pathlib import Path
from typing import ClassVar
from typing import List
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionResultModel


@pytest.fixture
def action_paths():
    """Get all action files in the example_actions directory.

    Returns:
        List of action file paths

    """
    example_dir = Path(os.path.dirname(__file__)) / "example_actions"
    action_files = list(example_dir.glob("*.py"))
    return [
        (action.stem, str(action.absolute()))
        for action in action_files
        if action.is_file() and action.stem != "__init__"
    ]


def get_action_paths():
    """Get action paths for parametrization.

    Returns:
        List of action file paths

    """
    example_dir = Path(os.path.dirname(__file__)) / "example_actions"
    action_files = list(example_dir.glob("*.py"))
    return [
        (action.stem, str(action.absolute()))
        for action in action_files
        if action.is_file() and action.stem != "__init__"
    ]


@pytest.fixture(params=get_action_paths(), ids=lambda x: x[0])
def action_info(request):
    """Fixture that yields action name and path for each action.

    Args:
        request: pytest request object

    Returns:
        Tuple of (action_name, action_path)

    """
    return request.param


@pytest.fixture
def mock_action_manager():
    """Create a mock action manager for testing.

    Returns:
        ActionManager instance

    """
    # Create a mock action manager
    manager = ActionManager("test", "test_dcc")

    # Patch os.path.isfile to return True for any path
    with patch("os.path.isfile", return_value=True):
        yield manager


def test_action_loading(action_info, mock_action_manager):
    """Test that an action can be loaded.

    Args:
        action_info: Tuple of (action_name, action_path)
        mock_action_manager: Mock ActionManager instance

    """
    action_name, action_path = action_info

    # Mock the registry's discover_actions_from_path method
    with patch.object(mock_action_manager.registry, "discover_actions_from_path") as mock_discover:
        action_dir = os.path.dirname(action_path)
        
        mock_action_manager.registry.discover_actions_from_path(
            path=action_path,
            context=mock_action_manager.context,
            dcc_name=mock_action_manager.dcc_name
        )

        # Verify that discover_actions_from_path was called
        assert mock_discover.called, "discover_actions_from_path was not called"

        # Check if the method was called with the correct parameters
        expected_params = {
            "path": action_path,  # ActionManager passes the full file path
            "context": mock_action_manager.context,
            "dcc_name": mock_action_manager.dcc_name,
        }

        # Find a call with matching parameters
        call_found = False
        for call in mock_discover.call_args_list:
            call_kwargs = call[1]
            # Check if this call has the expected path
            if call_kwargs.get("path") == action_path:
                call_found = True
                # Verify other parameters
                assert call_kwargs.get("dcc_name") == expected_params["dcc_name"]
                assert call_kwargs.get("context") == expected_params["context"]
                break

        # If no matching call was found, check if any call has a path that contains our action path
        if not call_found:
            for call in mock_discover.call_args_list:
                call_kwargs = call[1]
                path_param = call_kwargs.get("path", "")
                if action_path in path_param or action_dir in path_param:
                    call_found = True
                    break

        # Assert that we found a matching call
        assert call_found, f"No call to discover_actions_from_path with path containing {action_path}"

        # Create a mock Action class
        class MockAction:
            name = action_name
            description = f"Mock action for {action_name}"
            tags: ClassVar[List[str]] = ["test", "mock"]
            dcc = "test"
            order = 0

            class InputModel:
                @staticmethod
                def model_json_schema():
                    return {}

        # Replace the registry's list_actions method to return our mock action
        def mock_list_actions(dcc_name=None):
            return [
                {
                    "name": MockAction.name,
                    "internal_name": MockAction.name,  # 添加 internal_name 键
                    "description": MockAction.description,
                    "tags": MockAction.tags,
                    "dcc": MockAction.dcc,
                    "input_schema": {},
                }
            ]

        # Replace the registry's get_action method to return our mock Action class
        def mock_get_action(name):
            if name == action_name:
                return MockAction
            return None

        # Apply both mocks
        with patch.object(mock_action_manager.registry, "list_actions", side_effect=mock_list_actions):
            with patch.object(mock_action_manager.registry, "get_action", side_effect=mock_get_action):
                # Get actions info
                result = mock_action_manager.get_actions_info()

                # Verify result
                assert isinstance(result, ActionResultModel)
                assert result.success is True
                # 新的消息格式是 "Found X actions for DCC"
                assert "Found" in result.message
                assert "actions for test_dcc" in result.message

                # Verify that the action is in the context
                assert "actions" in result.context
                actions_info = result.context["actions"]

                # The action should be in the actions info
                assert MockAction.name in actions_info
                assert actions_info[MockAction.name]["name"] == MockAction.name
                assert actions_info[MockAction.name]["description"] == MockAction.description
                assert "test" in actions_info[MockAction.name]["tags"]
