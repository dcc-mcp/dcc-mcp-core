"""Tests for the filesystem module."""

# Import built-in modules
import os
from pathlib import Path
import shutil
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils.constants import ENV_ACTION_PATH_PREFIX
from dcc_mcp_core.utils.filesystem import _config
from dcc_mcp_core.utils.filesystem import register_dcc_actions_path
from dcc_mcp_core.utils.filesystem import register_dcc_actions_paths


@pytest.fixture
def temp_dir():
    """Create a temporary directory for testing."""
    temp_dir = tempfile.mkdtemp()
    yield temp_dir
    shutil.rmtree(temp_dir)


@pytest.fixture
def reset_action_paths():
    """Reset action paths before and after tests."""
    # Save original values
    original_dcc_paths = _config.dcc_actions_paths.copy()
    original_default_paths = _config.default_actions_paths.copy()

    # Clear for testing
    _config.dcc_actions_paths.clear()
    _config.default_actions_paths.clear()

    # Setup some test DCCs for testing
    _config.default_actions_paths["maya"] = []
    _config.default_actions_paths["houdini"] = []
    _config.default_actions_paths["blender"] = []

    yield

    # Restore original values
    _config.dcc_actions_paths.clear()
    _config.dcc_actions_paths.update(original_dcc_paths)

    _config.default_actions_paths.clear()
    _config.default_actions_paths.update(original_default_paths)


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
    for file_path in [maya_action1, maya_action2, maya_init, maya_hidden,
                      houdini_action1, houdini_action2, houdini_init, houdini_hidden,
                      maya_sub_action]:
        with open(file_path, 'w') as f:
            f.write("# Test action file\n")

    return {
        "temp_dir": temp_dir,
        "maya_dir": maya_dir,
        "houdini_dir": houdini_dir,
        "maya_actions": [maya_action1, maya_action2, maya_sub_action],
        "houdini_actions": [houdini_action1, houdini_action2],
        "hidden_files": [maya_hidden, houdini_hidden],
        "init_files": [maya_init, houdini_init]
    }


def test_register_dcc_actions_path(reset_action_paths, env_vars_cleanup):
    """Test registering a plugin path for a DCC."""
    # Register a path with forward slashes
    path = "path/to/maya/plugins"  # Use relative path without leading slash
    # The path will be resolved to an absolute path in the function
    resolved_path = str(Path(path).resolve())
    register_dcc_actions_path("maya", path)

    # Check if it was registered
    assert "maya" in _config.dcc_actions_paths
    assert resolved_path in _config.dcc_actions_paths["maya"]

    # Register the same path again (should not duplicate)
    register_dcc_actions_path("maya", path)
    assert len(_config.dcc_actions_paths["maya"]) == 1

    # Register a different path
    another_path = "another/path"  # Use relative path without leading slash
    resolved_another_path = str(Path(another_path).resolve())
    register_dcc_actions_path("maya", another_path)
    assert len(_config.dcc_actions_paths["maya"]) == 2
    assert resolved_another_path in _config.dcc_actions_paths["maya"]

    # Test case normalization
    third_path = "third/path"
    resolved_third_path = str(Path(third_path).resolve())
    register_dcc_actions_path("MAYA", third_path)
    assert len(_config.dcc_actions_paths["maya"]) == 3
    assert resolved_third_path in _config.dcc_actions_paths["maya"]


def test_register_dcc_actions_paths(reset_action_paths, env_vars_cleanup):
    """Test registering multiple plugin paths for a DCC."""
    # Register multiple paths
    paths = [
        "path/to/maya/plugins",
        "another/path",
        "third/path"
    ]
    resolved_paths = [str(Path(path).resolve()) for path in paths]
    register_dcc_actions_paths("maya", paths)

    # Check if they were registered
    assert "maya" in _config.dcc_actions_paths
    for path in resolved_paths:
        assert path in _config.dcc_actions_paths["maya"]


def test_discover_actions_in_paths(mock_action_dirs):
    """Test the _discover_actions_in_paths function."""
    # Import local modules
    from dcc_mcp_core.utils.filesystem import _discover_actions_in_paths

    # Test discovering actions in maya directory
    maya_actions = _discover_actions_in_paths([mock_action_dirs["maya_dir"]], ".py")

    # Should find 3 actions (2 in root, 1 in subdirectory)
    # Should exclude __init__.py and _hidden.py
    assert len(maya_actions) == 3

    # Check that all expected files are found
    for action in mock_action_dirs["maya_actions"]:
        assert action in maya_actions

    # Check that hidden files and init files are excluded
    for hidden in mock_action_dirs["hidden_files"]:
        assert hidden not in maya_actions

    for init in mock_action_dirs["init_files"]:
        assert init not in maya_actions


def test_fs_discover_actions(mock_action_dirs, reset_action_paths):
    """Test the fs_discover_actions function."""
    # Import local modules
    from dcc_mcp_core.utils.filesystem import clear_dcc_actions_paths
    from dcc_mcp_core.utils.filesystem import discover_actions
    from dcc_mcp_core.utils.filesystem import register_dcc_actions_path

    # Clear any existing paths
    clear_dcc_actions_paths()

    # Register the mock directories
    register_dcc_actions_path("maya", mock_action_dirs["maya_dir"])
    register_dcc_actions_path("houdini", mock_action_dirs["houdini_dir"])

    # Test discovering actions for maya
    maya_result = discover_actions("maya")
    assert "maya" in maya_result
    assert len(maya_result["maya"]) == 3  # 2 in root, 1 in subdirectory

    # Test discovering actions for houdini
    houdini_result = discover_actions("houdini")
    assert "houdini" in houdini_result
    assert len(houdini_result["houdini"]) == 2

    # Test discovering actions for all DCCs
    all_results = discover_actions()
    assert "maya" in all_results
    assert "houdini" in all_results
    assert len(all_results["maya"]) == 3
    assert len(all_results["houdini"]) == 2
