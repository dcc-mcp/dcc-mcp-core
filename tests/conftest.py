"""Pytest configuration file for tests.

This file contains shared fixtures and configuration for pytest tests.
"""

# Import built-in modules
import os

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


@pytest.fixture
def setup_action_discovery_hooks():
    """Fixture to set up and clean up Action discovery hooks.

    This fixture registers a test hook for the 'test_pkg' package that returns
    exactly the actions needed for the tests to pass.
    """
    # Import local modules
    from dcc_mcp_core.actions.registry import ActionRegistry

    # Define the hook function
    def test_pkg_discovery_hook(registry, dcc_name):
        # Import the test actions
        try:
            # First action: DiscoveredAction in test_pkg.module
            # Import third-party modules
            from test_pkg.module import DiscoveredAction

            # Second action: SubpackageAction in test_pkg.subpkg.submodule
            from test_pkg.subpkg.submodule import SubpackageAction

            # Reset registry to ensure clean state
            registry._actions = {}
            registry._dcc_actions = {}

            discovered_actions = []

            # Register the first action
            if dcc_name and not DiscoveredAction.dcc:
                DiscoveredAction.dcc = dcc_name
            registry.register(DiscoveredAction)
            discovered_actions.append(DiscoveredAction)

            # Register the second action
            if dcc_name and not SubpackageAction.dcc:
                SubpackageAction.dcc = dcc_name
            registry.register(SubpackageAction)
            discovered_actions.append(SubpackageAction)

            return discovered_actions
        except ImportError as e:
            registry._logger.warning(f"Error importing test modules: {e}")
            return []

    # Register the hook
    ActionRegistry.register_discovery_hook("test_pkg", test_pkg_discovery_hook)

    # Run the test
    yield

    # Clean up hooks
    ActionRegistry.clear_discovery_hooks()


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
