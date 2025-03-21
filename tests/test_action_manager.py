"""Tests for the action_manager module.

This module contains tests for the plugin manager functionality, including
discovering plugins, loading plugins, and calling plugin functions.
"""

# Import built-in modules
import os
import sys
from unittest.mock import MagicMock
from unittest.mock import patch
from unittest.mock import call

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import call_action_function
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import discover_actions
from dcc_mcp_core.actions.manager import get_action
from dcc_mcp_core.actions.manager import get_action_info
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.actions.manager import get_actions
from dcc_mcp_core.actions.manager import load_action
from dcc_mcp_core.actions.manager import load_actions
from dcc_mcp_core.actions.manager import get_actions_info
from dcc_mcp_core.models import ActionModel, FunctionModel, ParameterModel, ActionsInfoModel, ActionResultModel


@pytest.fixture
def cleanup_action_managers():
    """Fixture to clean up action managers after each test."""
    # Store the original action managers
    # Import local modules
    from dcc_mcp_core.actions.manager import _action_managers
    original_managers = _action_managers.copy()

    # Run the test
    yield

    # Clean up action managers
    _action_managers.clear()
    _action_managers.update(original_managers)

    # Clear action modules and actions for each manager
    for manager in _action_managers.values():
        manager._action_modules = {}
        manager._actions = {}


@pytest.fixture
def mock_discover_actions():
    """Fixture to mock the discover_actions function."""
    with patch('dcc_mcp_core.actions.manager.fs_discover_actions') as mock:
        # Set up the mock to return a predefined list of actions
        mock.return_value = {
            'maya': ['path/to/maya_action1.py', 'path/to/maya_action2.py'],
            'houdini': ['path/to/houdini_action.py']
        }
        yield mock


@pytest.fixture
def mock_import_module():
    """Fixture to mock importlib.import_module."""
    with patch('importlib.import_module') as mock:
        # Create a mock module with a register function
        mock_module = MagicMock()
        mock_module.register.return_value = ActionResultModel(success=True, message="Action registered", context={'name': 'test_action'})
        mock.return_value = mock_module
        yield mock


def test_action_manager_init():
    """Test ActionManager initialization."""
    manager = ActionManager('maya')
    assert manager.dcc_name == 'maya'
    assert manager._actions == {}
    assert manager._action_modules == {}


def test_action_manager_discover_actions(mock_discover_actions):
    """Test ActionManager.discover_actions method."""
    # Create an action manager
    manager = ActionManager('maya')

    # Discover actions
    result = manager.discover_actions()

    # Check if the mock was called with the correct arguments
    mock_discover_actions.assert_called_once_with('maya', extension='.py')

    # Check that the result is the expected dictionary
    assert 'maya' in result
    assert isinstance(result['maya'], ActionResultModel)
    assert result['maya'].success is True
    assert "Actions discovered" in result['maya'].message
    assert result['maya'].context['paths'] == ['path/to/maya_action1.py', 'path/to/maya_action2.py']


def test_action_manager_load_action(mock_import_module):
    """Test ActionManager.load_action method."""
    manager = ActionManager('maya')
    action_path = 'path/to/test_action.py'

    # Reset the mock before the test
    mock_import_module.reset_mock()

    # Mock os.path.isfile to avoid file not found error
    with patch('os.path.isfile', return_value=True):
        # Mock the imported module and its initialization
        mock_module = MagicMock()
        mock_import_module.return_value = mock_module
        
        # Mock any additional import_module calls that might happen
        def side_effect(name):
            if name in ('os', 'os.path'):
                # Return a mock for system modules
                return MagicMock()
            # Return our main mock for the action module
            return mock_module
        
        mock_import_module.side_effect = side_effect

        # Load the action
        result = manager.load_action(action_path)

        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert 'loaded successfully' in result.message
        assert result.context['action_name'] == 'test_action'
        assert result.context['paths'] == [action_path]

        # Verify the action was registered
        assert 'test_action' in manager._action_modules
        assert manager._action_modules['test_action'] == mock_module

    # Test error handling for invalid paths
    with patch('os.path.isfile', return_value=False):
        result = manager.load_action('invalid/path.py')
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert 'not found' in result.message


def test_action_manager_load_actions(mock_discover_actions, mock_import_module):
    """Test ActionManager.load_actions method."""
    manager = ActionManager('maya')
    
    with patch.object(manager, 'discover_actions') as mock_discover:
        # 设置 discover_actions 的返回值为新格式
        mock_discover.return_value = {
            'maya': ActionResultModel(
                success=True,
                message="Actions discovered",
                context={'paths': ['path/to/maya_action1.py', 'path/to/maya_action2.py']}
            )
        }
        
        # Mock os.path.isfile to avoid file not found error
        with patch('os.path.isfile', return_value=True):
            # Load all discovered actions
            result = manager.load_actions()

            # Check if discover_plugins was called
            mock_discover.assert_called_once()

            # Check if import_module was called for each action
            # Note: load_action calls import_module and potentially reload for each action
            # So we expect 2 calls per action (2 actions * 2 calls = 4 calls)
            assert mock_import_module.call_count == 4

            # Check if the function returned the correct actions
            assert isinstance(result, ActionsInfoModel)
            assert len(result.actions) == 2


def test_action_manager_get_action():
    """Test ActionManager.get_action method."""
    manager = ActionManager('maya')
    manager._actions = {
        'test_action': {'name': 'test_action'}
    }

    # Get existing action
    action = manager.get_action('test_action')
    assert isinstance(action, ActionResultModel)
    assert action.success is True
    assert action.context['name'] == 'test_action'

    # Get non-existent action
    action = manager.get_action('nonexistent')
    assert isinstance(action, ActionResultModel)
    assert action.success is False
    assert "not found" in action.message


def test_action_manager_get_actions():
    """Test ActionManager.get_actions method."""
    manager = ActionManager('maya')
    manager._actions = {
        'action1': ActionResultModel(success=True, message="Action 1 found", context={'name': 'action1'}),
        'action2': ActionResultModel(success=True, message="Action 2 found", context={'name': 'action2'})
    }

    # Get all actions
    actions = manager.get_actions()
    assert isinstance(actions, ActionResultModel)
    assert actions.success is True
    assert 'action1' in actions.context
    assert 'action2' in actions.context
    assert actions.context['action1'].context['name'] == 'action1'
    assert actions.context['action2'].context['name'] == 'action2'


def test_create_action_manager():
    """Test create_action_manager function."""
    manager = create_action_manager('maya')
    assert isinstance(manager, ActionManager)
    assert manager.dcc_name == 'maya'


def test_get_action_manager(cleanup_action_managers):
    """Test get_action_manager function."""
    # First call should create a new manager
    manager1 = get_action_manager('maya')
    assert isinstance(manager1, ActionManager)
    assert manager1.dcc_name == 'maya'

    # Second call should return the same manager
    manager2 = get_action_manager('maya')
    assert manager2 is manager1

    # Call with a different DCC should create a new manager
    manager3 = get_action_manager('houdini')
    assert isinstance(manager3, ActionManager)
    assert manager3.dcc_name == 'houdini'
    assert manager3 is not manager1


def test_discover_actions(mock_discover_actions):
    """Test discover_actions function."""
    # Call the discover_actions function
    result = discover_actions('maya')
    
    # Check if the mock was called with the correct arguments
    mock_discover_actions.assert_called_once_with('maya', extension='.py')
    
    # Check if the result is the expected dictionary
    assert 'maya' in result
    assert isinstance(result['maya'], ActionResultModel)
    assert result['maya'].success is True
    assert "Actions discovered" in result['maya'].message
    assert result['maya'].context['paths'] == ['path/to/maya_action1.py', 'path/to/maya_action2.py']


def test_load_action_successful_import(cleanup_action_managers, fs):
    """Test successful action loading using pyfakefs for file system operations."""
    # 创建测试目录和文件
    action_dir = '/fake/path/to'
    action_file = f'{action_dir}/test_action.py'
    action_path = action_file
    
    # 使用 pyfakefs 创建目录和文件
    fs.create_dir(action_dir)
    fs.create_file(action_file, contents='''
"""
Test action module for testing.
"""

__action_name__ = "test_action"
__action_version__ = "1.0.0"
__action_description__ = "Test action for unit testing"
__action_author__ = "Test Author"

def test_function():
    """Test function."""
    return "test result"
''')
    
    # 模拟模块导入
    with patch('importlib.import_module') as mock_import_module, \
         patch('dcc_mcp_core.utils.filesystem.convert_path_to_module', return_value='test_action'):
        
        # 设置模拟模块
        mock_module = MagicMock()
        mock_module.__name__ = 'test_action'
        mock_module.__action_name__ = 'test_action'
        mock_module.__action_version__ = '1.0.0'
        mock_module.__action_description__ = 'Test action for unit testing'
        mock_module.__action_author__ = 'Test Author'
        mock_module.test_function = MagicMock(return_value="test result")
        mock_import_module.return_value = mock_module
        
        # 执行被测函数
        result = load_action('maya', action_path)
        
        # 验证结果
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert "Action 'test_action' loaded successfully" in result.message
        assert result.context['action_name'] == 'test_action'
        
        # Verify module is loaded (via public API)
        action = get_action('maya', 'test_action')
        assert action.success is True


def test_load_action_invalid_path(cleanup_action_managers, fs):
    """Test action loading with invalid file path using pyfakefs."""
    # 不创建文件，确保路径无效
    invalid_path = '/fake/invalid/path.py'
    
    # Execute the function being tested
    result = load_action('maya', invalid_path)
    
    # Verify the result
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert 'not found' in result.message


def test_load_action_import_error(cleanup_action_managers, fs):
    """Test action loading with import error using pyfakefs."""
    # Create test file
    action_dir = '/fake/path/to'
    action_file = f'{action_dir}/test_action.py'
    
    # Use pyfakefs to create directory and file
    fs.create_dir(action_dir)
    fs.create_file(action_file, contents='# Empty file that will cause import error')
    
    # Mock import error
    with patch('importlib.import_module') as mock_import_module, \
         patch('dcc_mcp_core.utils.filesystem.convert_path_to_module', return_value='test_action'):
        
        # Set import error
        mock_import_module.side_effect = ImportError("Could not import module")
        
        # Execute the function being tested
        result = load_action('maya', action_file)
        
        # Verify the result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Failed to load action 'test_action'" in result.message
        assert 'Could not import module' in result.error


def test_load_actions(cleanup_action_managers, fs):
    """Test load_actions function using pyfakefs."""
    # Create a fake file system
    fs.create_dir('/path/to/actions')
    fs.create_file('/path/to/actions/action1.py', contents='''
__action_name__ = "action1"
__action_version__ = "1.0.0"
__action_description__ = "Test action 1"
__action_author__ = "Test Author"

def test_function():
    """Test function."""
    return "Test function result"
''')
    fs.create_file('/path/to/actions/action2.py', contents='''
__action_name__ = "action2"
__action_version__ = "1.0.0"
__action_description__ = "Test action 2"
__action_author__ = "Test Author"

def another_function():
    """Another test function."""
    return "Another function result"
''')

    # Mock discover_actions to return our test actions
    with patch('dcc_mcp_core.actions.manager.fs_discover_actions') as mock_discover:
        mock_discover.return_value = {
            'test': ['/path/to/actions/action1.py', '/path/to/actions/action2.py']
        }
        
        # Mock os.path.isfile to return True for our test files
        with patch('os.path.isfile', return_value=True):
            # Mock importlib.import_module to return a mock module
            with patch('importlib.import_module') as mock_import:
                # Create mock modules
                mock_module1 = MagicMock()
                mock_module1.__action_name__ = "action1"
                mock_module1.__action_version__ = "1.0.0"
                mock_module1.__action_description__ = "Test action 1"
                mock_module1.__action_author__ = "Test Author"
                mock_module1.test_function = MagicMock(return_value="Test function result")
                mock_module1.__file__ = '/path/to/actions/action1.py'
                
                mock_module2 = MagicMock()
                mock_module2.__action_name__ = "action2"
                mock_module2.__action_version__ = "1.0.0"
                mock_module2.__action_description__ = "Test action 2"
                mock_module2.__action_author__ = "Test Author"
                mock_module2.another_function = MagicMock(return_value="Another function result")
                mock_module2.__file__ = '/path/to/actions/action2.py'
                
                # Set up the mock to return our mock modules
                def side_effect(name):
                    if name == 'action1':
                        return mock_module1
                    elif name == 'action2':
                        return mock_module2
                    else:
                        return MagicMock()
                        
                mock_import.side_effect = side_effect
                
                # Call load_actions
                result = load_actions('test')
                
                # Check if the result is an ActionsInfoModel
                assert isinstance(result, ActionsInfoModel)
                
                # Check if the function returned the correct actions
                assert result.dcc_name == 'test'
                assert len(result.actions) == 2
                assert 'action1' in result.actions
                assert 'action2' in result.actions
                
                # Check action1 details
                action1 = result.actions['action1']
                assert action1.name == 'action1'
                assert action1.version == '1.0.0'
                assert action1.description == 'Test action 1'
                assert action1.author == 'Test Author'
                assert 'test_function' in action1.functions
                
                # Check action2 details
                action2 = result.actions['action2']
                assert action2.name == 'action2'
                assert action2.version == '1.0.0'
                assert action2.description == 'Test action 2'
                assert action2.author == 'Test Author'
                assert 'another_function' in action2.functions


def test_get_action(cleanup_action_managers):
    """Test get_action function."""
    # Set up an action in the manager
    manager = get_action_manager('maya')
    mock_module = MagicMock()
    mock_functions = {'func1': MagicMock(), 'func2': MagicMock()}
    
    # Register the action module and functions
    manager._action_modules = {'test_action': mock_module}
    manager._actions = {'test_action': mock_functions}

    # Get an existing action
    action = get_action('maya', 'test_action')
    assert isinstance(action, ActionResultModel)
    assert action.success is True
    assert 'test_action' in action.message
    assert action.context['action_name'] == 'test_action'
    assert action.context['module'] == mock_module
    assert action.context['functions'] == mock_functions

    # Test getting a non-existent action
    action = get_action('maya', 'non_existent_action')
    assert isinstance(action, ActionResultModel)
    assert action.success is False
    assert 'not found' in action.message
    assert 'not loaded or does not exist' in action.error


def test_get_actions(cleanup_action_managers):
    """Test get_actions function."""
    # Set up actions in the manager
    manager = get_action_manager('maya')
    manager._actions = {
        'action1': ActionResultModel(success=True, message="Action 1 found", context={'name': 'action1'}),
        'action2': ActionResultModel(success=True, message="Action 2 found", context={'name': 'action2'})
    }

    # Get all actions
    actions = get_actions('maya')
    assert isinstance(actions, ActionResultModel)
    assert actions.success is True
    assert 'action1' in actions.context
    assert 'action2' in actions.context
    assert actions.context['action1'].context['name'] == 'action1'
    assert actions.context['action2'].context['name'] == 'action2'


def test_get_action_info(cleanup_action_managers):
    """Test get_action_info function."""
    # Set up an action with metadata in the manager
    manager = get_action_manager('maya')

    # Create a mock module with the necessary attributes
    action = MagicMock()
    action.__doc__ = "Test action documentation"
    action.__file__ = "path/to/test_action.py"
    action.__name__ = "test_action"

    # Add a test function to the mock module
    test_func = MagicMock()
    test_func.__doc__ = "Test function docstring"
    test_func.__code__ = MagicMock()
    test_func.__code__.co_varnames = ('arg1', 'arg2')
    test_func.__code__.co_argcount = 2

    # Patch the inspect.getmembers function to return our test function
    with patch('inspect.getmembers') as mock_getmembers, \
         patch('dcc_mcp_core.actions.manager.ActionManager.get_action_info') as mock_get_info:
        # Set up the mock to return our test function
        mock_getmembers.return_value = [("test_func", test_func)]

        # Set up the mock to return our action info as an ActionModel
        action_model = ActionModel(
            name='test_action',
            description="Test action documentation",
            file_path="path/to/test_action.py",
            dcc="maya",
            version="1.0.0",
            author="Test Author",
            requires=[],
            functions={
                'test_func': FunctionModel(
                    name='test_func',
                    description="Test function docstring",
                    return_type="str",
                    parameters=[
                        ParameterModel(name="arg1", type_hint="str", type="str", required=True, default=None, description=""),
                        ParameterModel(name="arg2", type_hint="str", type="str", required=True, default=None, description="")
                    ]
                )
            },
            documentation_url=None,
            tags=[],
            capabilities=[]
        )
        mock_get_info.return_value = action_model

        # Register the mock module and action in the manager
        manager._action_modules = {'test_action': action}
        manager._actions = {'test_action': {'name': 'test_action'}}

        # Get action info
        info = get_action_info('maya', 'test_action')

        # Check if the correct info was returned
        assert isinstance(info, ActionModel)
        assert info.name == 'test_action'


def test_get_actions_info(cleanup_action_managers):
    """Test get_actions_info function."""
    # Set up actions with metadata in the manager
    manager = get_action_manager('maya')

    # Create mock modules with the necessary attributes
    action1 = MagicMock()
    action1.__doc__ = "Action 1 documentation"
    action1.__file__ = "path/to/action1.py"
    action1.__name__ = "action1"

    # Add a test function to the first mock module
    func1 = MagicMock()
    func1.__doc__ = "Function 1 docstring"
    func1.__code__ = MagicMock()
    func1.__code__.co_varnames = ('arg1',)
    func1.__code__.co_argcount = 1

    action2 = MagicMock()
    action2.__doc__ = "Action 2 documentation"
    action2.__file__ = "path/to/action2.py"
    action2.__name__ = "action2"

    # Add a test function to the second mock module
    func2 = MagicMock()
    func2.__doc__ = "Function 2 docstring"
    func2.__code__ = MagicMock()
    func2.__code__.co_varnames = ('arg1', 'arg2')
    func2.__code__.co_argcount = 2

    # Patch the get_actions_info function to return our action info
    with patch('dcc_mcp_core.actions.manager.ActionManager.get_actions_info') as mock_get_info:
        # Create ActionModel instances for our actions
        action1_model = ActionModel(
            name='action1',
            description="Action 1 documentation",
            file_path="path/to/action1.py",
            dcc="maya",
            version="1.0.0",
            author="Test Author",
            requires=[],
            functions={
                'func1': FunctionModel(
                    name='func1',
                    description="Function 1 docstring",
                    return_type="int",
                    parameters=[
                        ParameterModel(name="arg1", type_hint="str", type="str", required=True, default=None, description="")
                    ]
                )
            },
            documentation_url=None,
            tags=[],
            capabilities=[]
        )
        
        action2_model = ActionModel(
            name='action2',
            description="Action 2 documentation",
            file_path="path/to/action2.py",
            dcc="maya",
            version="2.0.0",
            author="Another Author",
            requires=["dep1"],
            functions={
                'func2': FunctionModel(
                    name='func2',
                    description="Function 2 docstring",
                    return_type="str",
                    parameters=[
                        ParameterModel(name="arg1", type_hint="str", type="str", required=True, default=None, description=""),
                        ParameterModel(name="arg2", type_hint="str", type="str", required=True, default=None, description="")
                    ]
                )
            },
            documentation_url=None,
            tags=[],
            capabilities=[]
        )
        
        # Create an ActionsInfoModel
        actions_info_model = ActionsInfoModel(
            dcc_name="maya",
            actions={
                'action1': action1_model,
                'action2': action2_model
            }
        )
        
        # Set up the mock to return our ActionsInfoModel
        mock_get_info.return_value = actions_info_model

        # Register the mock modules and actions in the manager
        manager._action_modules = {
            'action1': action1,
            'action2': action2
        }

        manager._actions = {
            'action1': {'name': 'action1'},
            'action2': {'name': 'action2'}
        }

        # Get all action info
        info = get_actions_info('maya')

        # Check if the correct info was returned
        assert isinstance(info, ActionsInfoModel)
        assert info.dcc_name == "maya"
        assert len(info.actions) == 2
        
        # Verify action1
        assert 'action1' in info.actions
        action1_info = info.actions['action1']
        assert isinstance(action1_info, ActionModel)
        assert action1_info.name == 'action1'
        assert action1_info.description == "Action 1 documentation"
        assert 'func1' in action1_info.functions
        
        # Verify action2
        assert 'action2' in info.actions
        action2_info = info.actions['action2']
        assert isinstance(action2_info, ActionModel)
        assert action2_info.name == 'action2'
        assert action2_info.description == "Action 2 documentation"
        assert 'func2' in action2_info.functions
        
        # Verify function in action2
        func2_info = action2_info.functions['func2']
        assert isinstance(func2_info, FunctionModel)
        assert func2_info.name == 'func2'
        assert len(func2_info.parameters) == 2
        
        # Verify parameter in func2
        param = func2_info.parameters[0]
        assert isinstance(param, ParameterModel)
        assert param.name == 'arg1'
        assert param.type_hint == 'str'


def test_call_action_function(cleanup_action_managers):
    """Test call_action_function function."""
    # Set up an action with a test function
    manager = get_action_manager('maya')

    # Create a mock module with a test function
    action = MagicMock()
    test_func = MagicMock(return_value="test_result")
    action.test_func = test_func

    # Register the mock module in the manager
    manager._action_modules = {'test_action': action}
    manager._actions = {'test_action': {'name': 'test_action'}}

    # Test calling the function
    result = call_action_function('maya', 'test_action', 'test_func', arg1='arg1', arg2='value2')

    # Check if the function was called with the correct arguments
    test_func.assert_called_once_with(arg1='arg1', arg2='value2')

    # Check if the correct result was returned
    assert isinstance(result, ActionResultModel)
    assert result.success is True
    assert result.context['result'] == "test_result"

    # Test with a non-existent action
    result = call_action_function('maya', 'non_existent_action', 'test_func')
    assert result.success is False
    assert "not found" in result.error.lower()
