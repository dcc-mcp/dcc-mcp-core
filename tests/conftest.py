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
from dcc_mcp_core.plugin_manager import PluginManager


@pytest.fixture
def test_data_dir():
    """Fixture to provide the test data directory path."""
    return os.path.join(os.path.dirname(__file__), "data")


@pytest.fixture
def dcc_name():
    """Fixture to provide a DCC name for testing."""
    return "test"


@pytest.fixture
def plugin_manager(dcc_name):
    """Fixture to create a plugin manager for testing."""
    manager = PluginManager(dcc_name)
    return manager
