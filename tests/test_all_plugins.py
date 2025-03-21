"""Tests for all plugins in the data directory.

This module uses pytest.mark.parametrize to test all plugins in the data directory.
"""

# Import built-in modules
import os
import importlib.util
from pathlib import Path

# Import third-party modules
import pytest
from unittest.mock import patch

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionModel, ActionResultModel
from dcc_mcp_core.utils.filesystem import append_to_python_path


@pytest.fixture
def plugin_paths():
    """Get all plugin files in the data directory.
    
    Returns:
        List of plugin file paths
    """
    data_dir = Path(os.path.dirname(__file__)) / "data"
    plugin_files = list(data_dir.glob("*.py"))
    return [(plugin.stem, str(plugin.absolute())) for plugin in plugin_files if plugin.is_file()]


def get_plugin_paths():
    """Helper function to get plugin paths for parametrization.
    
    Returns:
        List of plugin file paths
    """
    data_dir = Path(os.path.dirname(__file__)) / "data"
    plugin_files = list(data_dir.glob("*.py"))
    return [(plugin.stem, str(plugin.absolute())) for plugin in plugin_files if plugin.is_file()]


@pytest.fixture(params=get_plugin_paths(), ids=lambda x: x[0])
def plugin_info(request):
    """Fixture that yields plugin name and path for each plugin.
    
    Args:
        request: pytest request object
        
    Returns:
        Tuple of (plugin_name, plugin_path)
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


def test_plugin_loading(plugin_info, mock_action_manager):
    """Test that a plugin can be loaded.
    
    Args:
        plugin_info: Tuple of (plugin_name, plugin_path)
        mock_action_manager: Mock ActionManager instance
    """
    plugin_name, plugin_path = plugin_info
    
    # Use a context manager to add the plugin directory to sys.path
    plugin_dir = os.path.dirname(plugin_path)
    with append_to_python_path(plugin_dir):
        # Load the plugin module directly using importlib
        spec = importlib.util.spec_from_file_location(plugin_name, plugin_path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        
        # Register the module with the action manager
        mock_action_manager._action_modules[plugin_name] = module
        mock_action_manager._actions[plugin_name] = mock_action_manager._auto_register_functions(module)
        
        # Get action info
        action_info = mock_action_manager.get_action_info(plugin_name)
        
        # Verify action info is an ActionModel
        assert isinstance(action_info, ActionModel)
        
        # Verify basic metadata
        assert action_info.name == plugin_name
        assert hasattr(action_info, 'version')
        assert hasattr(action_info, 'description')
        assert hasattr(action_info, 'author')
        
        # Verify functions
        assert hasattr(action_info, 'functions')
        assert len(action_info.functions) > 0, f"Plugin {plugin_name} has no functions"
        
        # Test specific features based on plugin name
        if plugin_name == "basic_plugin":
            # Verify basic_plugin has specific functions
            assert "hello_world" in action_info.functions
            assert "add_numbers" in action_info.functions
            assert "process_data" in action_info.functions
            
            # Verify function parameters
            add_func = action_info.functions["add_numbers"]
            assert len(add_func.parameters) == 2
            assert add_func.parameters[0].name == "a"
            assert add_func.parameters[1].name == "b"
            
        elif plugin_name == "minimal_plugin":
            # Verify minimal_plugin has minimal metadata
            # Minimal plugin might have empty metadata
            assert "test_function" in action_info.functions
            
        elif plugin_name == "advanced_types_plugin":
            # Verify advanced_types_plugin has functions with complex types
            assert "process_list" in action_info.functions
            assert "process_dict" in action_info.functions
            assert "process_complex" in action_info.functions
            
            # Verify function with complex types
            process_list_func = action_info.functions["process_list"]
            assert process_list_func.return_type and ("List" in process_list_func.return_type or "list" in process_list_func.return_type.lower())
            
        elif plugin_name == "internal_helpers_plugin":
            # Verify internal helper functions are not exposed
            assert "process_text" in action_info.functions
            assert "process_values" in action_info.functions
            assert "get_version" in action_info.functions
            assert "_format_string" not in action_info.functions
            assert "_process_data" not in action_info.functions
            
        elif plugin_name == "maya_plugin":
            # Verify maya_plugin has Maya-specific functions
            assert "create_cube" in action_info.functions
            assert "create_sphere" in action_info.functions
            assert "get_selection" in action_info.functions
