"""Tests for all actions in the data directory.

This module uses pytest.mark.parametrize to test all actions in the data directory.
"""

# Import built-in modules
import importlib.util
import os
from pathlib import Path
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.filesystem import append_to_python_path


@pytest.fixture
def action_paths():
    """Get all action files in the data directory.

    Returns:
        List of action file paths

    """
    data_dir = Path(os.path.dirname(__file__)) / "data"
    action_files = list(data_dir.glob("*.py"))
    return [(action.stem, str(action.absolute())) for action in action_files if action.is_file()]


def get_action_paths():
    """Get action paths for parametrization.

    Returns:
        List of action file paths

    """
    data_dir = Path(os.path.dirname(__file__)) / "data"
    action_files = list(data_dir.glob("*.py"))
    return [(action.stem, str(action.absolute())) for action in action_files if action.is_file()]


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
    manager = ActionManager('test')

    # Patch os.path.isfile to return True for any path
    with patch('os.path.isfile', return_value=True):
        yield manager


def test_action_loading(action_info, mock_action_manager):
    """Test that an action can be loaded.

    Args:
        action_info: Tuple of (action_name, action_path)
        mock_action_manager: Mock ActionManager instance

    """
    action_name, action_path = action_info

    # Use a context manager to add the action directory to sys.path
    action_dir = os.path.dirname(action_path)
    with append_to_python_path(action_dir):
        # Load the action module directly using importlib
        spec = importlib.util.spec_from_file_location(action_name, action_path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        # Register the module with the action manager
        mock_action_manager._action_modules[action_name] = module
        mock_action_manager._actions[action_name] = mock_action_manager._auto_register_functions(module, action_name)

        # Get action info
        action_info = mock_action_manager.get_action_info(action_name)

        # Verify action info is an ActionResultModel and contains an ActionModel in context['result']
        assert isinstance(action_info, ActionResultModel)
        assert action_info.success
        assert 'result' in action_info.context

        # Get the actual ActionModel from the result
        action_model = action_info.context['result']
        assert isinstance(action_model, ActionModel)

        # Verify basic metadata
        assert action_model.name == action_name
        assert hasattr(action_model, 'version')
        assert hasattr(action_model, 'description')
        assert hasattr(action_model, 'author')

        # Verify functions
        assert hasattr(action_model, 'functions')
        assert len(action_model.functions) > 0, f"Action {action_name} has no functions"

        # Test specific features based on action name
        if action_name == "basic_action":
            # Verify basic_action has specific functions
            assert "hello_world" in action_model.functions
            assert "add_numbers" in action_model.functions
            assert "process_data" in action_model.functions

            # Verify function parameters
            add_func = action_model.functions["add_numbers"]
            assert len(add_func.parameters) == 2
            assert add_func.parameters[0].name == "a"
            assert add_func.parameters[1].name == "b"

        elif action_name == "minimal_action":
            # Verify minimal_action has minimal metadata
            # Minimal action might have empty metadata
            assert "minimal_function" in action_model.functions

        elif action_name == "advanced_types_action":
            # Verify advanced_types_action has functions with complex types
            assert "process_complex_data" in action_model.functions
            assert "async_operation" in action_model.functions

            # Verify function with complex types
            process_complex_func = action_model.functions["process_complex_data"]
            assert process_complex_func.return_type and ("Tuple" in process_complex_func.return_type)

        elif action_name == "internal_helpers_action":
            # Verify public functions are exposed and internal helpers are not
            assert "get_calculated_value" in action_model.functions
            assert "get_formatted_calculation" in action_model.functions
            assert "_calculate_value" not in action_model.functions
            assert "_format_result" not in action_model.functions

        elif action_name == "maya_action":
            # Verify maya_action has Maya-specific functions
            assert "create_cube" in action_model.functions
            assert "list_objects" in action_model.functions
