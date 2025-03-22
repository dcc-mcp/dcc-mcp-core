"""Tests for the action loading functionality of the ActionManager class.

This module contains tests for the action loading methods of the ActionManager class.
"""

# Import built-in modules
import os
import types
from types import ModuleType
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionResultModel


@pytest.fixture
def mock_import_module():
    """Fixture to mock the importlib.import_module function."""
    with patch('importlib.import_module') as mock:
        yield mock


@pytest.fixture
def test_data_dir():
    """Fixture to provide the path to the test data directory."""
    return os.path.join(os.path.dirname(__file__), 'data')


def test_action_manager_load_action(mock_import_module, test_data_dir):
    """Test ActionManager.load_action method."""
    manager = ActionManager('maya')

    # Use an actual existing test file path
    action_path = os.path.abspath(os.path.join(test_data_dir, 'basic_plugin.py'))
    print(f"Action path: {action_path}")
    # Ensure the file exists
    assert os.path.isfile(action_path), f"test file does not exist: {action_path}"

    # Create a mock module with all necessary metadata
    mock_module = types.ModuleType('basic_plugin')
    mock_module.__file__ = action_path
    mock_module.__action_name__ = "basic_plugin"
    mock_module.__action_version__ = "1.0.0"
    mock_module.__action_description__ = "A basic test plugin with complete metadata"
    mock_module.__action_author__ = "Test Author"
    mock_module.__action_requires__ = ["dependency1", "dependency2"]

    # Add some test functions to the mock module
    def hello_world():
        return "Hello, World!"

    def add_numbers(a, b):
        return a + b

    mock_module.hello_world = hello_world
    mock_module.add_numbers = add_numbers

    # Mock the load_module_from_path function to return our mock module
    with patch('dcc_mcp_core.utils.module_loader.load_module_from_path', return_value=mock_module):
        # Mock os.path.getmtime to avoid file not found errors
        with patch('os.path.getmtime', return_value=12345):
            # Load action
            result = manager.load_action(action_path)

            # Print detailed result information for debugging
            print(f"\nLoad action result: {result}")
            print(f"Success: {result.success}")
            print(f"Message: {result.message}")
            print(f"Error: {result.error}")
            print(f"Context: {result.context}")

            if not result.success and 'error_details' in result.context:
                print(f"Error details: {result.context['error_details']}")

            # Verify result
            assert isinstance(result, ActionResultModel)
            assert result.success is True, f"Loading failed, error: {result.error}\nDetails: {result.context.get('error_details', '')}"
            assert "loaded successfully" in result.message
            assert result.context['action_name'] == 'basic_plugin'

            # Verify the module has been correctly loaded into the manager
            assert 'basic_plugin' in manager._action_modules
            # Compare module attributes instead of identity
            loaded_module = manager._action_modules['basic_plugin']
            assert loaded_module.__action_name__ == mock_module.__action_name__
            assert loaded_module.__action_version__ == mock_module.__action_version__
            assert loaded_module.__action_description__ == mock_module.__action_description__


def test_load_action_missing_file():
    """Test loading an action from a non-existent file."""
    manager = ActionManager('maya')

    # Use a non-existent file path
    action_path = '/path/to/non_existent_file.py'

    # Mock os.path.isfile to return False
    with patch('os.path.isfile', return_value=False):
        # Load the action
        result = manager.load_action(action_path)

        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Action file not found" in result.message
        assert result.error is not None


def test_load_action_invalid_module():
    """Test loading an action from an invalid module."""
    manager = ActionManager('maya')

    # Use a valid file path
    action_path = '/path/to/valid_file.py'

    # Mock os.path.isfile to return True
    with patch('os.path.isfile', return_value=True), \
         patch('dcc_mcp_core.utils.module_loader.load_module_from_path') as mock_load_module, \
         patch('dcc_mcp_core.actions.manager.append_to_python_path') as mock_append_to_python_path, \
         patch('os.path.getmtime', return_value=12345):  # Mock getmtime to avoid FileNotFoundError

        # Set up context manager behavior
        mock_append_to_python_path.return_value.__enter__.return_value = None
        mock_append_to_python_path.return_value.__exit__.return_value = None

        # Configure mock_load_module to raise an exception
        mock_load_module.side_effect = ImportError("Module could not be imported")

        # Load the action
        result = manager.load_action(action_path)

        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Failed to load action 'valid_file'" == result.message
        assert result.error is not None


def test_load_action_missing_metadata():
    """Test loading an action with missing metadata."""
    manager = ActionManager('maya')

    # Use a valid file path
    action_path = '/path/to/valid_file.py'

    # Mock os.path.isfile to return True and os.path.getmtime to return a fixed timestamp
    with patch('os.path.isfile', return_value=True), \
         patch('os.path.getmtime', return_value=1000), \
         patch('dcc_mcp_core.utils.module_loader.load_module_from_path') as mock_load_module, \
         patch('dcc_mcp_core.actions.manager.append_to_python_path') as mock_append_to_python_path:

        # Set up context manager behavior
        mock_append_to_python_path.return_value.__enter__.return_value = None
        mock_append_to_python_path.return_value.__exit__.return_value = None

        # Create a mock module without required metadata
        mock_module = types.ModuleType('invalid_module')
        mock_module.__file__ = action_path
        # Missing __action_name__, __action_version__, etc.

        # Configure mock_load_module to return the invalid module
        mock_load_module.return_value = mock_module

        # Load the action
        result = manager.load_action(action_path)

        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Missing required metadata" in result.message or "Failed to load" in result.message
        assert result.error is not None
