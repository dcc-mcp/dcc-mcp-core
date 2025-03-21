"""Tests for the filesystem module."""

# Import built-in modules
import os
import shutil
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils.constants import ENV_ACTION_PATH_PREFIX
from dcc_mcp_core.utils.filesystem import _load_paths_from_env
from dcc_mcp_core.utils.filesystem import discover_actions
from dcc_mcp_core.utils.filesystem import ensure_directory_exists
from dcc_mcp_core.utils.filesystem import get_actions_dir
from dcc_mcp_core.utils.filesystem import get_actions_dir_from_env
from dcc_mcp_core.utils.filesystem import get_user_actions_directory
from dcc_mcp_core.utils.filesystem import load_actions_paths_config
from dcc_mcp_core.utils.filesystem import register_dcc_actions_path
from dcc_mcp_core.utils.filesystem import register_dcc_actions_paths
from dcc_mcp_core.utils.filesystem import save_actions_paths_config
from dcc_mcp_core.utils.filesystem import set_default_action_paths
from dcc_mcp_core.utils.filesystem import _dcc_actions_paths_cache
from dcc_mcp_core.utils.filesystem import _default_actions_paths_cache


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
    original_dcc_paths = _dcc_actions_paths_cache.copy()
    original_default_paths = _default_actions_paths_cache.copy()

    # Clear for testing
    _dcc_actions_paths_cache.clear()
    _default_actions_paths_cache.clear()

    # Setup some test DCCs for testing
    _default_actions_paths_cache["maya"] = []
    _default_actions_paths_cache["houdini"] = []
    _default_actions_paths_cache["blender"] = []

    yield

    # Restore original values
    _dcc_actions_paths_cache.clear()
    _dcc_actions_paths_cache.update(original_dcc_paths)

    _default_actions_paths_cache.clear()
    _default_actions_paths_cache.update(original_default_paths)


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


def test_register_dcc_actions_path(reset_action_paths):
    """Test registering a plugin path for a DCC."""
    # Register a path with forward slashes
    path = "/path/to/maya/plugins"
    normalized_path = os.path.normpath(path)  # This will convert to OS-specific format
    register_dcc_actions_path("maya", path)

    # Check if it was registered
    assert "maya" in _dcc_actions_paths_cache
    assert normalized_path in _dcc_actions_paths_cache["maya"]

    # Register the same path again (should not duplicate)
    register_dcc_actions_path("maya", path)
    assert len(_dcc_actions_paths_cache["maya"]) == 1

    # Register a different path
    another_path = "/another/path"
    normalized_another_path = os.path.normpath(another_path)
    register_dcc_actions_path("maya", another_path)
    assert len(_dcc_actions_paths_cache["maya"]) == 2
    assert normalized_another_path in _dcc_actions_paths_cache["maya"]

    # Test case normalization
    third_path = "/third/path"
    normalized_third_path = os.path.normpath(third_path)
    register_dcc_actions_path("MAYA", third_path)
    assert len(_dcc_actions_paths_cache["maya"]) == 3
    assert normalized_third_path in _dcc_actions_paths_cache["maya"]


def test_register_dcc_actions_paths(reset_action_paths):
    """Test registering multiple plugin paths for a DCC."""
    # Register multiple paths
    paths = ["/path1", "/path2", "/path3"]
    normalized_paths = [os.path.normpath(p) for p in paths]
    register_dcc_actions_paths("maya", paths)

    # Check if they were all registered
    assert "maya" in _dcc_actions_paths_cache
    for path in normalized_paths:
        assert path in _dcc_actions_paths_cache["maya"]
