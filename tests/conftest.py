"""Pytest configuration file for tests.

This file contains shared fixtures and configuration for pytest tests.
"""

# Import built-in modules
import os
from pathlib import Path
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


def pytest_collect_file(file_path, parent):
    """Determine if a file should be collected.

    Args:
        file_path: Path to the file being considered for collection (pathlib.Path)
        parent: Parent collector

    """
    return None


def pytest_pycollect_makeitem(collector, name, obj):
    """Determine if an object should be collected as a test item.

    This hook is called for each Python object in a module that pytest is considering
    for collection as a test item. We use it to skip any class that inherits from Action.
    """
    # If the object is a class and is a subclass of Action, skip collection
    if hasattr(obj, "__bases__") and any(base.__name__ == "Action" for base in obj.__bases__):
        return None
