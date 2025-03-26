"""Tests for the filesystem module."""

# Import built-in modules
import os
from pathlib import Path
import shutil
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.constants import ENV_ACTION_PATH_PREFIX


@pytest.fixture
def temp_dir():
    """Create a temporary directory for testing."""
    temp_dir = tempfile.mkdtemp()
    yield temp_dir
    shutil.rmtree(temp_dir)


@pytest.fixture
def env_vars_cleanup():
    """Save and restore environment variables for testing."""
    # Save original environment variables
    saved_env = {}
    env_vars_to_remove = []

    for key in os.environ:
        if key.startswith(ENV_ACTION_PATH_PREFIX):
            saved_env[key] = os.environ[key]
            env_vars_to_remove.append(key)

    # Remove any existing environment variables with our prefix
    for key in env_vars_to_remove:
        del os.environ[key]

    yield

    # Restore original environment
    for key in env_vars_to_remove:
        if key in saved_env:
            os.environ[key] = saved_env[key]
        else:
            if key in os.environ:
                del os.environ[key]


@pytest.fixture
def mock_action_dirs(temp_dir, test_data_dir):
    """Create a mock directory structure for testing action discovery."""
    # Create main directories
    maya_dir = os.path.join(temp_dir, "maya")
    houdini_dir = os.path.join(temp_dir, "houdini")
    os.makedirs(maya_dir)
    os.makedirs(houdini_dir)

    # Create action files for maya
    maya_action1 = os.path.join(maya_dir, "create_sphere.py")
    maya_action2 = os.path.join(maya_dir, "delete_objects.py")
    maya_init = os.path.join(maya_dir, "__init__.py")
    maya_hidden = os.path.join(maya_dir, "_hidden.py")

    # Create action files for houdini
    houdini_action1 = os.path.join(houdini_dir, "create_geo.py")
    houdini_action2 = os.path.join(houdini_dir, "export_alembic.py")
    houdini_init = os.path.join(houdini_dir, "__init__.py")
    houdini_hidden = os.path.join(houdini_dir, "_hidden.py")

    # Create subdirectory with actions
    maya_sub_dir = os.path.join(maya_dir, "tools")
    os.makedirs(maya_sub_dir)
    maya_sub_action = os.path.join(maya_sub_dir, "advanced_tool.py")

    # Write content to files
    for file_path in [
        maya_action1,
        maya_action2,
        maya_sub_action,
        houdini_action1,
        houdini_action2,
        houdini_hidden,
    ]:
        with open(file_path, "w") as f:
            f.write("# Test action file\n")

    return {
        "temp_dir": temp_dir,
        "maya_dir": maya_dir,
        "houdini_dir": houdini_dir,
        "maya_actions": [maya_action1, maya_action2, maya_sub_action],
        "houdini_actions": [houdini_action1, houdini_action2],
        "hidden_files": [maya_hidden, houdini_hidden],
        "init_files": [maya_init, houdini_init],
    }
