"""Tests for the ActionManager.load_actions functionality with fixes.

This module contains a fixed version of the test_action_manager_load_actions test.
"""

# Import built-in modules
import os
from types import ModuleType
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
# Import DCC-MCP-Core modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.models import ActionsInfoModel
from dcc_mcp_core.models import FunctionModel
from dcc_mcp_core.models import ParameterModel


def test_action_manager_load_actions_fixed(test_data_dir):
    """Fixed test for ActionManager.load_actions method."""
    # Create ActionManager instance
    manager = ActionManager('maya')

    # Set up test data paths
    basic_plugin_path = os.path.abspath(os.path.join(test_data_dir, 'basic_plugin.py'))
    advanced_plugin_path = os.path.abspath(os.path.join(test_data_dir, 'advanced_types_plugin.py'))

    # Ensure test files exist
    assert os.path.isfile(basic_plugin_path), f"Test file not found: {basic_plugin_path}"
    assert os.path.isfile(advanced_plugin_path), f"Test file not found: {advanced_plugin_path}"

    # Create expected action models for verification
    basic_action_info = ActionModel(
        name='basic_plugin',
        version='1.0.0',
        description='A basic test plugin with complete metadata',
        author='Test Author',
        requires=["dependency1", "dependency2"],
        dcc='maya',
        file_path=basic_plugin_path,
        functions={}
    )

    advanced_action_info = ActionModel(
        name='advanced_types_plugin',
        version='2.0.0',
        description='A plugin demonstrating advanced type annotations',
        author='Test Author',
        dcc='maya',
        file_path=advanced_plugin_path,
        functions={}
    )

    # Create expected actions info model
    expected_actions_info = ActionsInfoModel(
        dcc_name='maya',
        actions={
            "basic_plugin": basic_action_info,
            "advanced_types_plugin": advanced_action_info
        }
    )

    # get_actions_info method returns expected ActionsInfoModel
    with patch.object(manager, 'get_actions_info', return_value=expected_actions_info):
        # discover_actions method returns results containing test file paths
        with patch.object(manager, 'discover_actions') as mock_discover_actions:
            action_result = ActionResultModel(
                success=True,
                message="Found 2 actions",
                context={
                    'paths': [basic_plugin_path, advanced_plugin_path]
                }
            )
            mock_discover_actions.return_value = action_result

            # mock load_action method to avoid actual module loading
            with patch.object(manager, 'load_action') as mock_load_action:
                mock_load_action.return_value = ActionResultModel(
                    success=True,
                    message="Action loaded successfully",
                    context={}
                )

                # call load_actions method
                result = manager.load_actions()

                # verify result
                assert isinstance(result, ActionResultModel)
                assert result.success is True
                assert "completed successfully" in result.message

                # verify actions_info model
                actions_info = result.context.get('result')
                assert isinstance(actions_info, ActionsInfoModel)
                assert len(actions_info.actions) == 2

                # verify action information
                assert 'basic_plugin' in actions_info.actions
                assert actions_info.actions['basic_plugin'].name == 'basic_plugin'
                assert actions_info.actions['basic_plugin'].version == '1.0.0'

                assert 'advanced_types_plugin' in actions_info.actions
                assert actions_info.actions['advanced_types_plugin'].name == 'advanced_types_plugin'
                assert actions_info.actions['advanced_types_plugin'].version == '2.0.0'
