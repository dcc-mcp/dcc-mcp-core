"""Tests for the plugin_manager module.

This module contains tests for the plugin manager functionality, including
discovering plugins, loading plugins, and calling plugin functions.
"""

# Import built-in modules
import os
import sys
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.plugin_manager import PluginManager
from dcc_mcp_core.plugin_manager import call_plugin_function
from dcc_mcp_core.plugin_manager import create_plugin_manager
from dcc_mcp_core.plugin_manager import discover_plugins
from dcc_mcp_core.plugin_manager import get_plugin
from dcc_mcp_core.plugin_manager import get_plugin_info
from dcc_mcp_core.plugin_manager import get_plugin_manager
from dcc_mcp_core.plugin_manager import get_plugins
from dcc_mcp_core.plugin_manager import get_plugins_info
from dcc_mcp_core.plugin_manager import load_plugin
from dcc_mcp_core.plugin_manager import load_plugins


@pytest.fixture
def cleanup_plugin_managers():
    """Fixture to clean up plugin managers after each test."""
    # Store the original plugin managers
    # Import local modules
    from dcc_mcp_core.plugin_manager import _plugin_managers
    original_managers = _plugin_managers.copy()

    # Run the test
    yield

    # Clean up plugin managers
    _plugin_managers.clear()
    _plugin_managers.update(original_managers)

    # Clear plugin modules and plugins for each manager
    for manager in _plugin_managers.values():
        manager._plugin_modules = {}
        manager._plugins = {}


@pytest.fixture
def mock_discover_plugins():
    """Fixture to mock the discover_plugins function."""
    with patch('dcc_mcp_core.plugin_manager.discover_plugins') as mock:
        # Set up the mock to return a predefined list of plugins
        mock.return_value = {
            'maya': ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py'],
            'houdini': ['path/to/houdini_plugin.py']
        }
        yield mock


@pytest.fixture
def mock_import_module():
    """Fixture to mock importlib.import_module."""
    with patch('importlib.import_module') as mock:
        # Create a mock module with a register function
        mock_module = MagicMock()
        mock_module.register.return_value = {'name': 'test_plugin'}
        mock.return_value = mock_module
        yield mock


def test_plugin_manager_init():
    """Test PluginManager initialization."""
    manager = PluginManager('maya')
    assert manager.dcc_name == 'maya'
    assert manager._plugins == {}
    assert manager._plugin_modules == {}


def test_plugin_manager_discover_plugins():
    """Test PluginManager.discover_plugins method."""
    # Create a plugin manager
    manager = PluginManager('maya')

    # Mock the filesystem.discover_plugins function
    with patch('dcc_mcp_core.filesystem.discover_plugins') as mock_discover:
        mock_discover.return_value = {'maya': ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']}

        # Discover plugins
        plugins = manager.discover_plugins()

        # Check if filesystem.discover_plugins was called with correct arguments
        mock_discover.assert_called_once_with('maya', '.py')

        # Check if the correct plugins were returned
        assert plugins == ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']


def test_plugin_manager_load_plugin(mock_import_module):
    """Test PluginManager.load_plugin method."""
    manager = PluginManager('maya')
    plugin_path = 'path/to/test_plugin.py'

    # Load the plugin
    result = manager.load_plugin(plugin_path)

    # Check if importlib.import_module was called with the correct module name
    mock_import_module.assert_called_once_with('test_plugin')

    # Check if the plugin was stored correctly
    assert 'test_plugin' in manager._plugin_modules
    assert 'test_plugin' in manager._plugins
    assert manager._plugins['test_plugin'] == {'name': 'test_plugin'}

    # Check if the function returned the correct module
    assert result == mock_import_module.return_value


def test_plugin_manager_load_plugins(mock_discover_plugins, mock_import_module):
    """Test PluginManager.load_plugins method."""
    manager = PluginManager('maya')

    # Mock the discover_plugins method to return specific paths
    with patch.object(manager, 'discover_plugins') as mock_discover:
        mock_discover.return_value = ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']

        # Load all discovered plugins
        result = manager.load_plugins()

        # Check if discover_plugins was called
        mock_discover.assert_called_once()

        # Check if import_module was called for each plugin
        assert mock_import_module.call_count == 2

        # Check if the function returned the correct plugins
        assert len(result) == 2
        assert all(plugin in result for plugin in ['maya_plugin1', 'maya_plugin2'])


def test_plugin_manager_get_plugin():
    """Test PluginManager.get_plugin method."""
    manager = PluginManager('maya')
    manager._plugins = {'test_plugin': {'name': 'test_plugin'}}

    # Get an existing plugin
    plugin = manager.get_plugin('test_plugin')
    assert plugin == {'name': 'test_plugin'}

    # Get a non-existent plugin
    plugin = manager.get_plugin('non_existent')
    assert plugin is None


def test_plugin_manager_get_plugins():
    """Test PluginManager.get_plugins method."""
    manager = PluginManager('maya')
    manager._plugins = {
        'plugin1': {'name': 'plugin1'},
        'plugin2': {'name': 'plugin2'}
    }

    # Get all plugins
    plugins = manager.get_plugins()
    assert plugins == manager._plugins


def test_create_plugin_manager():
    """Test create_plugin_manager function."""
    manager = create_plugin_manager('maya')
    assert isinstance(manager, PluginManager)
    assert manager.dcc_name == 'maya'


def test_get_plugin_manager(cleanup_plugin_managers):
    """Test get_plugin_manager function."""
    # First call should create a new manager
    manager1 = get_plugin_manager('maya')
    assert isinstance(manager1, PluginManager)
    assert manager1.dcc_name == 'maya'

    # Second call should return the same manager
    manager2 = get_plugin_manager('maya')
    assert manager2 is manager1

    # Call with a different DCC should create a new manager
    manager3 = get_plugin_manager('houdini')
    assert isinstance(manager3, PluginManager)
    assert manager3.dcc_name == 'houdini'
    assert manager3 is not manager1


def test_discover_plugins():
    """Test discover_plugins function."""
    # Mock the filesystem.discover_plugins function
    with patch('dcc_mcp_core.filesystem.discover_plugins') as mock_discover:
        mock_discover.return_value = {'maya': ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']}

        # Discover plugins
        plugins = discover_plugins('maya')

        # Check if filesystem.discover_plugins was called with correct arguments
        mock_discover.assert_called_once_with('maya', '.py')

        # Check if the correct plugins were returned
        assert plugins == ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']


def test_load_plugin(mock_import_module, cleanup_plugin_managers):
    """Test load_plugin function."""
    # Reset the mock before the test
    mock_import_module.reset_mock()

    # Create a plugin manager and ensure it's clean
    manager = get_plugin_manager('maya')
    manager._plugin_modules = {}
    manager._plugins = {}

    plugin_path = 'path/to/test_plugin.py'

    # Mock the import_module to return a mock module
    mock_module = MagicMock()
    mock_import_module.return_value = mock_module

    # Load the plugin
    result = load_plugin('maya', plugin_path)

    # Check if importlib.import_module was called with the correct module name
    mock_import_module.assert_called_once_with('test_plugin')

    # Check if the function returned the correct module
    assert result == mock_module

    # Check if the plugin was stored in the manager
    manager = get_plugin_manager('maya')
    assert 'test_plugin' in manager._plugin_modules
    assert 'test_plugin' in manager._plugins


def test_load_plugins(mock_import_module, cleanup_plugin_managers):
    """Test load_plugins function."""
    # Reset the mock before the test
    mock_import_module.reset_mock()

    # Create a plugin manager and ensure it's clean
    manager = get_plugin_manager('maya')
    manager._plugin_modules = {}
    manager._plugins = {}

    # Mock both the module-level discover_plugins function and the PluginManager.discover_plugins method
    plugin_paths = ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']
    with patch('dcc_mcp_core.plugin_manager.discover_plugins', return_value=plugin_paths), \
         patch.object(PluginManager, 'discover_plugins', return_value=plugin_paths):
        # Mock the import_module to return a mock module
        mock_module1 = MagicMock()
        mock_module2 = MagicMock()
        mock_import_module.side_effect = lambda name: mock_module1 if 'maya_plugin1' in name else mock_module2

        # Load all discovered plugins
        result = load_plugins('maya')

        # Check if the function returned the expected plugins
        assert set(result.keys()) == {'maya_plugin1', 'maya_plugin2'}

        # Check that import_module was called at least once
        assert mock_import_module.call_count >= 1


def test_get_plugin(cleanup_plugin_managers):
    """Test get_plugin function."""
    # Set up a plugin in the manager
    manager = get_plugin_manager('maya')
    manager._plugins = {'test_plugin': {'name': 'test_plugin'}}

    # Get an existing plugin
    plugin = get_plugin('maya', 'test_plugin')
    assert plugin == {'name': 'test_plugin'}

    # Get a non-existent plugin
    plugin = get_plugin('maya', 'non_existent')
    assert plugin is None


def test_get_plugins(cleanup_plugin_managers):
    """Test get_plugins function."""
    # Set up plugins in the manager
    manager = get_plugin_manager('maya')
    manager._plugins = {
        'plugin1': {'name': 'plugin1'},
        'plugin2': {'name': 'plugin2'}
    }

    # Get all plugins
    plugins = get_plugins('maya')
    assert plugins == manager._plugins


def test_get_plugin_info(cleanup_plugin_managers):
    """Test get_plugin_info function."""
    # Set up a plugin with metadata in the manager
    manager = get_plugin_manager('maya')

    # Create a mock module with the necessary attributes
    plugin = MagicMock()
    plugin.__doc__ = "Test plugin documentation"
    plugin.__file__ = "path/to/test_plugin.py"
    plugin.__name__ = "test_plugin"

    # Add a test function to the mock module
    test_func = MagicMock()
    test_func.__doc__ = "Test function docstring"
    test_func.__code__ = MagicMock()
    test_func.__code__.co_varnames = ('arg1', 'arg2')
    test_func.__code__.co_argcount = 2

    # Patch the inspect.getmembers function to return our test function
    with patch('inspect.getmembers') as mock_getmembers, \
         patch('dcc_mcp_core.plugin_manager.PluginManager.get_plugin_info') as mock_get_info:
        # Set up the mock to return our test function
        mock_getmembers.return_value = [("test_func", test_func)]

        # Set up the mock to return our plugin info
        mock_get_info.return_value = {
            'name': 'test_plugin',
            'docstring': "Test plugin documentation",
            'file': "path/to/test_plugin.py",
            'functions': {
                'test_func': {
                    'docstring': "Test function docstring",
                    'parameters': ['arg1', 'arg2']
                }
            }
        }

        # Register the mock module and plugin in the manager
        manager._plugin_modules = {'test_plugin': plugin}
        manager._plugins = {'test_plugin': {'name': 'test_plugin'}}

        # Get plugin info
        info = get_plugin_info('maya', 'test_plugin')

        # Check if the correct info was returned
        assert info['name'] == 'test_plugin'
        assert info['docstring'] == "Test plugin documentation"
        assert info['file'] == "path/to/test_plugin.py"
        assert 'functions' in info
        assert 'test_func' in info['functions']
        assert info['functions']['test_func']['docstring'] == "Test function docstring"
        assert info['functions']['test_func']['parameters'] == ['arg1', 'arg2']


def test_get_plugins_info(cleanup_plugin_managers):
    """Test get_plugins_info function."""
    # Set up plugins with metadata in the manager
    manager = get_plugin_manager('maya')

    # Create mock modules with the necessary attributes
    plugin1 = MagicMock()
    plugin1.__doc__ = "Plugin 1 documentation"
    plugin1.__file__ = "path/to/plugin1.py"
    plugin1.__name__ = "plugin1"

    # Add a test function to the first mock module
    func1 = MagicMock()
    func1.__doc__ = "Function 1 docstring"
    func1.__code__ = MagicMock()
    func1.__code__.co_varnames = ('arg1',)
    func1.__code__.co_argcount = 1

    plugin2 = MagicMock()
    plugin2.__doc__ = "Plugin 2 documentation"
    plugin2.__file__ = "path/to/plugin2.py"
    plugin2.__name__ = "plugin2"

    # Add a test function to the second mock module
    func2 = MagicMock()
    func2.__doc__ = "Function 2 docstring"
    func2.__code__ = MagicMock()
    func2.__code__.co_varnames = ('arg1', 'arg2')
    func2.__code__.co_argcount = 2

    # Patch the get_plugins_info function to return our plugin info
    with patch('dcc_mcp_core.plugin_manager.PluginManager.get_plugins_info') as mock_get_info:
        # Set up the mock to return our plugin info
        mock_get_info.return_value = {
            'plugin1': {
                'name': 'plugin1',
                'docstring': "Plugin 1 documentation",
                'file': "path/to/plugin1.py",
                'functions': {
                    'func1': {
                        'docstring': "Function 1 docstring",
                        'parameters': ['arg1']
                    }
                }
            },
            'plugin2': {
                'name': 'plugin2',
                'docstring': "Plugin 2 documentation",
                'file': "path/to/plugin2.py",
                'functions': {
                    'func2': {
                        'docstring': "Function 2 docstring",
                        'parameters': ['arg1', 'arg2']
                    }
                }
            }
        }

        # Register the mock modules and plugins in the manager
        manager._plugin_modules = {
            'plugin1': plugin1,
            'plugin2': plugin2
        }

        manager._plugins = {
            'plugin1': {'name': 'plugin1'},
            'plugin2': {'name': 'plugin2'}
        }

        # Get all plugin info
        info = get_plugins_info('maya')

        # Check if the correct info was returned
        assert len(info) == 2
        assert info['plugin1']['name'] == 'plugin1'
        assert info['plugin1']['docstring'] == "Plugin 1 documentation"
        assert 'functions' in info['plugin1']
        assert 'func1' in info['plugin1']['functions']

        assert info['plugin2']['name'] == 'plugin2'
        assert info['plugin2']['docstring'] == "Plugin 2 documentation"
        assert 'functions' in info['plugin2']
        assert 'func2' in info['plugin2']['functions']


def test_call_plugin_function(cleanup_plugin_managers):
    """Test call_plugin_function function."""
    # Set up a plugin with a test function
    manager = get_plugin_manager('maya')

    # Create a mock module with a test function
    plugin = MagicMock()
    test_func = MagicMock(return_value="test_result")
    plugin.test_func = test_func

    # Register the mock module in the manager
    manager._plugin_modules = {'test_plugin': plugin}

    # Test calling the function
    result = call_plugin_function('maya', 'test_plugin', 'test_func', 'arg1', arg2='value2')

    # Check if the function was called with the correct arguments
    test_func.assert_called_once_with('arg1', arg2='value2')

    # Check if the correct result was returned
    assert result == "test_result"

    # Test with a non-existent plugin
    with pytest.raises(ValueError, match="Plugin not found"):
        call_plugin_function('maya', 'non_existent_plugin', 'test_func')

    # Test with a non-existent function
    with patch('dcc_mcp_core.plugin_manager.PluginManager.call_plugin_function') as mock_call:
        mock_call.side_effect = ValueError("Function not found in plugin test_plugin: non_existent_function")
        with pytest.raises(ValueError, match="Function not found"):
            call_plugin_function('maya', 'test_plugin', 'non_existent_function')
