# Tests for plugin_manager.py

import os
import sys
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock

from dcc_mcp_core.plugin_manager import (
    PluginManager,
    create_plugin_manager,
    get_plugin_manager,
    discover_plugins,
    load_plugin,
    load_plugins,
    get_plugin,
    get_plugins,
    get_plugin_info,
    get_plugins_info,
    call_plugin_function
)


@pytest.fixture
def cleanup_plugin_managers():
    """Fixture to clean up plugin managers after tests."""
    # Store the original state
    from dcc_mcp_core.plugin_manager import _plugin_managers
    original_managers = _plugin_managers.copy()
    
    yield
    
    # Restore the original state
    _plugin_managers.clear()
    _plugin_managers.update(original_managers)


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


def test_plugin_manager_discover_plugins(mock_discover_plugins):
    """Test PluginManager.discover_plugins method."""
    manager = PluginManager('maya')
    plugins = manager.discover_plugins()
    
    # Check if the plugin_manager.discover_plugins was called with correct arguments
    mock_discover_plugins.assert_called_once_with('maya')
    
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


def test_discover_plugins(mock_discover_plugins):
    """Test discover_plugins function."""
    plugins = discover_plugins('maya')
    
    # Check if the plugin_manager.discover_plugins was called with correct arguments
    mock_discover_plugins.assert_called_once_with('maya')
    
    # Check if the correct plugins were returned
    assert plugins == ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']


def test_load_plugin(mock_import_module, cleanup_plugin_managers):
    """Test load_plugin function."""
    plugin_path = 'path/to/test_plugin.py'
    
    # Load the plugin
    result = load_plugin('maya', plugin_path)
    
    # Check if importlib.import_module was called with the correct module name
    mock_import_module.assert_called_once_with('test_plugin')
    
    # Check if the function returned the correct module
    assert result == mock_import_module.return_value
    
    # Check if the plugin was stored in the manager
    manager = get_plugin_manager('maya')
    assert 'test_plugin' in manager._plugin_modules
    assert 'test_plugin' in manager._plugins


def test_load_plugins(mock_discover_plugins, mock_import_module, cleanup_plugin_managers):
    """Test load_plugins function."""
    # Mock the discover_plugins function to return specific paths
    with patch('dcc_mcp_core.plugin_manager.discover_plugins') as mock_discover:
        mock_discover.return_value = ['path/to/maya_plugin1.py', 'path/to/maya_plugin2.py']
        
        # Load all discovered plugins
        result = load_plugins('maya')
        
        # Check if discover_plugins was called
        mock_discover.assert_called_once_with('maya')
        
        # Check if import_module was called for each plugin
        assert mock_import_module.call_count == 2
        
        # Check if the function returned the correct plugins
        assert len(result) == 2
        assert all(plugin in result for plugin in ['maya_plugin1', 'maya_plugin2'])


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
    
    # Create a proper mock module with the necessary attributes
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
    
    # Set up the __dict__ with our test function
    plugin.__dict__ = {'test_func': test_func}
    
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
    plugin1.__dict__ = {'func1': func1}
    
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
    plugin2.__dict__ = {'func2': func2}
    
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
    # Set up a plugin with a test function in the manager
    manager = get_plugin_manager('maya')
    plugin = MagicMock()
    plugin.test_function = MagicMock(return_value="function result")
    
    # 需要同时设置 _plugin_modules 和 _plugins
    manager._plugin_modules = {'test_plugin': plugin}
    manager._plugins = {'test_plugin': plugin}
    
    # Call the plugin function
    result = call_plugin_function('maya', 'test_plugin', 'test_function', arg1='value1')
    
    # Check if the function was called with the correct arguments
    plugin.test_function.assert_called_once_with(arg1='value1')
    
    # Check if the function returned the correct result
    assert result == "function result"
    
    # Test with a non-existent function
    with pytest.raises(AttributeError):
        call_plugin_function('maya', 'test_plugin', 'non_existent_function')
