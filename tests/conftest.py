"""Pytest configuration file for tests.

This file contains shared fixtures and configuration for pytest tests.
"""

# Import built-in modules
import os
import sys
from unittest.mock import MagicMock

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager


@pytest.fixture
def test_data_dir():
    """Fixture to provide the test data directory path."""
    return os.path.join(os.path.dirname(__file__), "data")


@pytest.fixture
def dcc_name():
    """Fixture to provide a DCC name for testing."""
    return "test"


@pytest.fixture
def action_manager(dcc_name):
    """Fixture to create an action manager for testing."""
    manager = ActionManager(dcc_name)
    return manager


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
