"""Tests for the basic functionality of the ActionManager class.

This module contains tests for the initialization and core functionality of the ActionManager class.
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
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionResultModel


def test_action_manager_init():
    """Test ActionManager initialization."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Check that the manager has the correct DCC name
    assert manager.dcc_name == 'maya'

    # Check that the manager has empty action modules and actions
    assert manager._action_modules == {}
    assert manager._actions == {}


def test_create_action_manager():
    """Test create_action_manager function."""
    # Create a new ActionManager instance
    manager = create_action_manager('maya')

    # Check that the manager has the correct DCC name
    assert manager.dcc_name == 'maya'

    # Check that the manager is registered in _action_managers
    # Import local modules
    from dcc_mcp_core.actions.manager import _action_managers
    assert 'maya' in _action_managers
    assert _action_managers['maya'] == manager


def test_get_action_manager():
    """Test get_action_manager function."""
    # Create a new ActionManager instance
    create_manager = create_action_manager('maya')

    # Get the manager using get_action_manager
    get_manager = get_action_manager('maya')

    # Check that the managers are the same
    assert create_manager == get_manager

    # Check that get_action_manager returns None for non-existent DCC
    assert get_action_manager('non_existent_dcc').get_actions().success is False


def test_get_actions_empty(cleanup_action_managers):
    """Test get_actions method when no actions are loaded."""
    # Create a new ActionManager instance
    manager = ActionManager('maya')

    # Get actions
    result = manager.get_actions()

    # Check that the result is an ActionResultModel
    assert isinstance(result, ActionResultModel)

    # Check that the result indicates failure since no actions are loaded
    assert result.success is False

    # Check that the message indicates no actions are loaded
    assert "No actions loaded" in result.message

    # Check that the error message is set
    assert "No actions have been loaded yet" in result.error

    # Check that the context is empty
    assert result.context == {}
