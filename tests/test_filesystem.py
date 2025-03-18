"""Tests for the filesystem module."""

# Import built-in modules
import os
import shutil
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.filesystem import ENV_VAR_PREFIX
from dcc_mcp_core.filesystem import _dcc_plugin_paths_cache
from dcc_mcp_core.filesystem import _default_plugin_paths_cache
from dcc_mcp_core.filesystem import _load_paths_from_env
from dcc_mcp_core.filesystem import discover_plugins
from dcc_mcp_core.filesystem import ensure_directory_exists
from dcc_mcp_core.filesystem import get_plugin_paths
from dcc_mcp_core.filesystem import get_plugin_paths_from_env
from dcc_mcp_core.filesystem import get_user_plugin_directory
from dcc_mcp_core.filesystem import load_plugin_paths_config
from dcc_mcp_core.filesystem import register_dcc_plugin_path
from dcc_mcp_core.filesystem import register_dcc_plugin_paths
from dcc_mcp_core.filesystem import save_plugin_paths_config
from dcc_mcp_core.filesystem import set_default_plugin_paths


@pytest.fixture
def temp_dir():
    """Create a temporary directory for testing."""
    temp_dir = tempfile.mkdtemp()
    yield temp_dir
    shutil.rmtree(temp_dir)


@pytest.fixture
def reset_plugin_paths():
    """Reset plugin paths before and after tests."""
    # Save original values
    original_dcc_paths = _dcc_plugin_paths_cache.copy()
    original_default_paths = _default_plugin_paths_cache.copy()

    # Clear for testing
    _dcc_plugin_paths_cache.clear()
    _default_plugin_paths_cache.clear()

    # Setup some test DCCs for testing
    _default_plugin_paths_cache["maya"] = []
    _default_plugin_paths_cache["houdini"] = []
    _default_plugin_paths_cache["blender"] = []

    yield

    # Restore original values
    _dcc_plugin_paths_cache.clear()
    _dcc_plugin_paths_cache.update(original_dcc_paths)

    _default_plugin_paths_cache.clear()
    _default_plugin_paths_cache.update(original_default_paths)


@pytest.fixture
def env_vars_cleanup():
    """Save and restore environment variables for testing."""
    # Save original environment variables
    saved_env = {}
    env_vars_to_remove = []

    for key in os.environ:
        if key.startswith(ENV_VAR_PREFIX):
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


def test_register_dcc_plugin_path(reset_plugin_paths):
    """Test registering a plugin path for a DCC."""
    # Register a path with forward slashes
    path = "/path/to/maya/plugins"
    normalized_path = os.path.normpath(path)  # This will convert to OS-specific format
    register_dcc_plugin_path("maya", path)

    # Check if it was registered
    assert "maya" in _dcc_plugin_paths_cache
    assert normalized_path in _dcc_plugin_paths_cache["maya"]

    # Register the same path again (should not duplicate)
    register_dcc_plugin_path("maya", path)
    assert len(_dcc_plugin_paths_cache["maya"]) == 1

    # Register a different path
    another_path = "/another/path"
    normalized_another_path = os.path.normpath(another_path)
    register_dcc_plugin_path("maya", another_path)
    assert len(_dcc_plugin_paths_cache["maya"]) == 2
    assert normalized_another_path in _dcc_plugin_paths_cache["maya"]

    # Test case normalization
    third_path = "/third/path"
    normalized_third_path = os.path.normpath(third_path)
    register_dcc_plugin_path("MAYA", third_path)
    assert len(_dcc_plugin_paths_cache["maya"]) == 3
    assert normalized_third_path in _dcc_plugin_paths_cache["maya"]


def test_register_dcc_plugin_paths(reset_plugin_paths):
    """Test registering multiple plugin paths for a DCC."""
    # Register multiple paths
    paths = ["/path1", "/path2", "/path3"]
    normalized_paths = [os.path.normpath(p) for p in paths]
    register_dcc_plugin_paths("maya", paths)

    # Check if they were all registered
    assert "maya" in _dcc_plugin_paths_cache
    for path in normalized_paths:
        assert path in _dcc_plugin_paths_cache["maya"]


def test_get_plugin_paths_with_dcc(reset_plugin_paths):
    """Test getting plugin paths for a specific DCC."""
    # Register some paths
    maya_path = "/maya/plugins"
    houdini_path = "/houdini/plugins"


    register_dcc_plugin_path("maya", maya_path)
    register_dcc_plugin_path("houdini", houdini_path)

    # Get paths for a specific DCC
    maya_paths = get_plugin_paths("maya")
    # Use os.path.normpath to normalize the path for the current OS
    normalized_maya_path = os.path.normpath(maya_path)
    assert normalized_maya_path in maya_paths

    # Get paths for a DCC with no registered paths (should return default paths)
    blender_paths = get_plugin_paths("blender")
    assert blender_paths == []

    # Set a default path and check again
    blender_default_path = "/blender/default/plugins"
    normalized_blender_path = os.path.normpath(blender_default_path)
    set_default_plugin_paths("blender", [blender_default_path])
    blender_paths = get_plugin_paths("blender")
    assert normalized_blender_path in blender_paths

    # Get paths for a DCC that doesn't exist (should return empty list)
    nonexistent_paths = get_plugin_paths("nonexistent")
    assert nonexistent_paths == []


def test_get_plugin_paths(reset_plugin_paths):
    """Test getting plugin paths for a DCC."""
    # Register paths for different DCCs
    maya_path = "/maya/path"
    normalized_maya_path = os.path.normpath(maya_path)
    houdini_path = "/houdini/path"
    normalized_houdini_path = os.path.normpath(houdini_path)

    register_dcc_plugin_path("maya", maya_path)
    register_dcc_plugin_path("houdini", houdini_path)

    # Get paths for a specific DCC
    maya_paths = get_plugin_paths("maya")
    assert isinstance(maya_paths, list)
    assert normalized_maya_path in maya_paths

    # Get paths for all DCCs
    all_paths = get_plugin_paths()
    assert isinstance(all_paths, dict)
    assert "maya" in all_paths
    assert "houdini" in all_paths
    assert normalized_maya_path in all_paths["maya"]
    assert normalized_houdini_path in all_paths["houdini"]


def test_set_default_plugin_paths(reset_plugin_paths):
    """Test setting default plugin paths for a DCC."""
    # Set default paths
    paths = ["/default/path1", "/default/path2"]
    normalized_paths = [os.path.normpath(p) for p in paths]
    set_default_plugin_paths("maya", paths)

    # Check if they were set
    assert "maya" in _default_plugin_paths_cache
    for path in normalized_paths:
        assert path in _default_plugin_paths_cache["maya"]

    # Set for a DCC that doesn't have a default entry
    custom_path = "/custom/path"
    normalized_custom_path = os.path.normpath(custom_path)
    set_default_plugin_paths("custom_dcc", [custom_path])
    assert "custom_dcc" in _default_plugin_paths_cache
    assert normalized_custom_path in _default_plugin_paths_cache["custom_dcc"]


def test_discover_plugins(temp_dir, reset_plugin_paths):
    """Test discovering plugins in registered paths."""
    # Create some test plugin files
    maya_dir = os.path.join(temp_dir, "maya")
    houdini_dir = os.path.join(temp_dir, "houdini")
    os.makedirs(maya_dir)
    os.makedirs(houdini_dir)

    # Create Maya plugins
    with open(os.path.join(maya_dir, "plugin1.py"), "w") as f:
        f.write("# Test plugin")
    with open(os.path.join(maya_dir, "plugin2.py"), "w") as f:
        f.write("# Test plugin")

    # Create Houdini plugins
    with open(os.path.join(houdini_dir, "plugin1.py"), "w") as f:
        f.write("# Test plugin")

    # Register plugin directories
    register_dcc_plugin_path("maya", maya_dir)
    register_dcc_plugin_path("houdini", houdini_dir)

    # Discover plugins for a specific DCC
    maya_plugins = discover_plugins("maya")
    assert "maya" in maya_plugins
    assert len(maya_plugins["maya"]) == 2

    # Discover plugins for all DCCs
    all_plugins = discover_plugins()
    assert "maya" in all_plugins
    assert "houdini" in all_plugins
    assert len(all_plugins["maya"]) == 2
    assert len(all_plugins["houdini"]) == 1


def test_ensure_directory_exists(temp_dir):
    """Test ensuring a directory exists."""
    # Test creating a new directory
    new_dir = os.path.join(temp_dir, "new_dir")
    assert not os.path.exists(new_dir)

    result = ensure_directory_exists(new_dir)
    assert result
    assert os.path.exists(new_dir)

    # Test with an existing directory
    result = ensure_directory_exists(new_dir)
    assert result


def test_get_user_plugin_directory():
    """Test getting the user's plugin directory for a DCC."""
    # Get user plugin directory
    maya_dir = get_user_plugin_directory("maya")

    # Check if it exists and is correct
    assert os.path.exists(maya_dir)
    assert os.path.basename(os.path.dirname(maya_dir)) == "plugins"
    assert os.path.basename(maya_dir) == "maya"


def test_save_load_plugin_paths_config(temp_dir, reset_plugin_paths):
    """Test saving and loading plugin paths configuration."""
    # Register some paths
    maya_path = "/maya/path"
    houdini_path = "/houdini/path"
    blender_path = "/blender/path"

    normalized_maya_path = os.path.normpath(maya_path)
    normalized_houdini_path = os.path.normpath(houdini_path)
    normalized_blender_path = os.path.normpath(blender_path)

    register_dcc_plugin_path("maya", maya_path)
    register_dcc_plugin_path("houdini", houdini_path)

    # Set some default paths
    set_default_plugin_paths("blender", [blender_path])

    # Save configuration to a temporary file
    config_path = os.path.join(temp_dir, "plugin_paths.json")
    result = save_plugin_paths_config(config_path)
    assert result
    assert os.path.exists(config_path)

    # Clear paths
    _dcc_plugin_paths_cache.clear()
    _default_plugin_paths_cache.clear()
    _default_plugin_paths_cache["maya"] = []
    _default_plugin_paths_cache["houdini"] = []
    _default_plugin_paths_cache["blender"] = []

    # Load configuration
    result = load_plugin_paths_config(config_path)
    assert result

    # Check if paths were loaded correctly
    assert "maya" in _dcc_plugin_paths_cache
    assert normalized_maya_path in _dcc_plugin_paths_cache["maya"]
    assert "houdini" in _dcc_plugin_paths_cache
    assert normalized_houdini_path in _dcc_plugin_paths_cache["houdini"]
    assert normalized_blender_path in _default_plugin_paths_cache["blender"]


def test_get_plugin_paths_from_env(temp_dir, reset_plugin_paths, env_vars_cleanup):
    """Test getting plugin paths from environment variables."""
    # Create test directories
    maya_dir = os.path.join(temp_dir, "maya_env")
    maya_dir2 = os.path.join(temp_dir, "maya_env2")
    os.makedirs(maya_dir)
    os.makedirs(maya_dir2)

    # Set environment variables
    os.environ[f"{ENV_VAR_PREFIX}MAYA"] = f"{maya_dir}{os.pathsep}{maya_dir2}"

    # Get plugin paths from environment
    maya_paths = get_plugin_paths_from_env("maya")

    # Check if paths were returned correctly
    assert "maya" in maya_paths
    assert len(maya_paths["maya"]) == 2
    assert maya_dir in maya_paths["maya"]
    assert maya_dir2 in maya_paths["maya"]

    # Test with non-existent path in environment variable
    non_existent_dir = os.path.join(temp_dir, "non_existent")
    os.environ[f"{ENV_VAR_PREFIX}BLENDER"] = non_existent_dir

    blender_paths = get_plugin_paths_from_env("blender")
    assert "blender" in blender_paths
    assert non_existent_dir in blender_paths["blender"]


def test_load_paths_from_env(temp_dir, reset_plugin_paths, env_vars_cleanup):
    """Test loading plugin paths from environment variables."""
    # Create test directories
    maya_dir = os.path.join(temp_dir, "maya_env_load")
    houdini_dir = os.path.join(temp_dir, "houdini_env_load")
    os.makedirs(maya_dir)
    os.makedirs(houdini_dir)

    # Set environment variables
    os.environ[f"{ENV_VAR_PREFIX}MAYA"] = maya_dir
    os.environ[f"{ENV_VAR_PREFIX}HOUDINI"] = houdini_dir

    # Load paths from environment
    _load_paths_from_env()

    # Check if paths were registered
    maya_paths = get_plugin_paths("maya")
    houdini_paths = get_plugin_paths("houdini")

    assert maya_dir in maya_paths
    assert houdini_dir in houdini_paths


def test_get_plugin_paths_with_env(temp_dir, reset_plugin_paths, env_vars_cleanup):
    """Test getting plugin paths with environment variables."""
    # Create test directories
    config_maya_dir = os.path.join(temp_dir, "maya_config")
    env_maya_dir = os.path.join(temp_dir, "maya_env")
    os.makedirs(config_maya_dir)
    os.makedirs(env_maya_dir)

    # Register path through configuration
    register_dcc_plugin_path("maya", config_maya_dir)

    # Set environment variable
    os.environ[f"{ENV_VAR_PREFIX}MAYA"] = env_maya_dir

    # Load paths from environment
    _load_paths_from_env()

    # Get paths for maya
    maya_paths = get_plugin_paths("maya")

    # Check if both paths are included
    assert config_maya_dir in maya_paths
    assert env_maya_dir in maya_paths

    # Test with a new environment path
    env_maya_dir2 = os.path.join(temp_dir, "maya_env2")
    os.makedirs(env_maya_dir2)
    os.environ[f"{ENV_VAR_PREFIX}MAYA"] = env_maya_dir2

    # Clear cache and reload
    _dcc_plugin_paths_cache.clear()
    _default_plugin_paths_cache.clear()

    #
    register_dcc_plugin_path("maya", config_maya_dir)
    _load_paths_from_env()

    # Get paths again
    maya_paths = get_plugin_paths("maya")

    # Check if the new path is included and old env path is removed
    assert config_maya_dir in maya_paths
    assert env_maya_dir not in maya_paths
    assert env_maya_dir2 in maya_paths
