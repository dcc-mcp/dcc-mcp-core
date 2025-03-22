"""Tests for the utils.filesystem module."""

# Import built-in modules
import json
import os
from pathlib import Path
import sys
from typing import Dict
from typing import List
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import mock_open
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils.constants import ENV_ACTIONS_DIR
from dcc_mcp_core.utils.constants import ENV_ACTION_PATH_PREFIX
from dcc_mcp_core.utils.filesystem import ActionPathsConfig
from dcc_mcp_core.utils.filesystem import _discover_actions_in_paths
from dcc_mcp_core.utils.filesystem import _load_config_if_needed
from dcc_mcp_core.utils.filesystem import append_to_python_path
from dcc_mcp_core.utils.filesystem import clear_dcc_actions_paths
from dcc_mcp_core.utils.filesystem import convert_path_to_module
from dcc_mcp_core.utils.filesystem import discover_actions
from dcc_mcp_core.utils.filesystem import ensure_directory_exists
from dcc_mcp_core.utils.filesystem import get_action_paths
from dcc_mcp_core.utils.filesystem import get_actions_dir_from_env
from dcc_mcp_core.utils.filesystem import get_actions_paths_from_env
from dcc_mcp_core.utils.filesystem import get_all_registered_dccs
from dcc_mcp_core.utils.filesystem import get_templates_directory
from dcc_mcp_core.utils.filesystem import get_user_actions_directory
from dcc_mcp_core.utils.filesystem import load_actions_paths_config
from dcc_mcp_core.utils.filesystem import load_module_from_path
from dcc_mcp_core.utils.filesystem import register_dcc_actions_path
from dcc_mcp_core.utils.filesystem import register_dcc_actions_paths
from dcc_mcp_core.utils.filesystem import save_actions_paths_config
from dcc_mcp_core.utils.filesystem import set_default_action_paths


@pytest.fixture
def reset_config():
    """Reset the global configuration before and after each test."""
    # Import _config directly here to avoid circular imports
    # Import local modules
    from dcc_mcp_core.utils.filesystem import _config

    # Save original state
    original_dcc_paths = _config.dcc_actions_paths.copy()
    original_default_paths = _config.default_actions_paths.copy()

    # Reset for test
    _config.dcc_actions_paths.clear()
    _config.default_actions_paths.clear()

    yield

    # Restore original state
    _config.dcc_actions_paths.clear()
    _config.dcc_actions_paths.update(original_dcc_paths)
    _config.default_actions_paths.clear()
    _config.default_actions_paths.update(original_default_paths)


class TestActionPathsConfig:
    """Tests for the ActionPathsConfig class."""

    def test_init(self):
        """Test initialization of ActionPathsConfig."""
        config = ActionPathsConfig()
        assert hasattr(config, "dcc_actions_paths")
        assert hasattr(config, "default_actions_paths")

    @patch.dict(os.environ, {f"{ENV_ACTION_PATH_PREFIX}maya": "/path/to/maya/actions"}, clear=True)
    def test_load_from_env(self):
        """Test loading action paths from environment variables."""
        config = ActionPathsConfig()
        assert "maya" in config.dcc_actions_paths
        assert any(p.replace("\\", "/").endswith("/path/to/maya/actions") for p in config.dcc_actions_paths["maya"])


class TestPathRegistration:
    """Tests for path registration functions."""

    @patch("dcc_mcp_core.utils.filesystem.save_actions_paths_config")
    def test_register_dcc_actions_path(self, mock_save, reset_config):
        """Test registering a single action path."""
        test_path = os.path.join("path", "to", "maya", "actions")
        register_dcc_actions_path("maya", test_path)

        paths = get_action_paths("maya")
        assert len(paths) > 0, "No paths were registered"
        assert any("maya" in p and p.endswith("actions") for p in paths), f"Expected path not found in {paths}"
        mock_save.assert_called_once()

    @patch("dcc_mcp_core.utils.filesystem.save_actions_paths_config")
    def test_register_dcc_actions_paths(self, mock_save, reset_config):
        """Test registering multiple action paths."""
        path1 = os.path.join("path", "to", "maya", "actions1")
        path2 = os.path.join("path", "to", "maya", "actions2")
        paths = [path1, path2]
        register_dcc_actions_paths("maya", paths)

        result_paths = get_action_paths("maya")
        assert len(result_paths) >= 2, f"Expected at least 2 paths, got {len(result_paths)}"
        assert any(p.endswith("actions1") for p in result_paths), f"Path ending with 'actions1' not found in {result_paths}"
        assert any(p.endswith("actions2") for p in result_paths), f"Path ending with 'actions2' not found in {result_paths}"
        assert mock_save.call_count == 2

    def test_get_action_paths_with_dcc(self, reset_config):
        """Test getting action paths for a specific DCC."""
        maya_path = os.path.join("path", "to", "maya", "actions")
        houdini_path = os.path.join("path", "to", "houdini", "actions")
        register_dcc_actions_path("maya", maya_path)
        register_dcc_actions_path("houdini", houdini_path)

        maya_paths = get_action_paths("maya")
        assert len(maya_paths) > 0, "No paths were registered for maya"
        assert any(p.endswith("actions") for p in maya_paths), f"Expected path not found in {maya_paths}"

    def test_get_action_paths_all(self, reset_config):
        """Test getting action paths for all DCCs."""
        maya_path = os.path.join("path", "to", "maya", "actions")
        houdini_path = os.path.join("path", "to", "houdini", "actions")
        register_dcc_actions_path("maya", maya_path)
        register_dcc_actions_path("houdini", houdini_path)

        all_paths = get_action_paths()
        assert "maya" in all_paths
        assert "houdini" in all_paths
        assert any(p.endswith("actions") for p in all_paths["maya"]), f"Expected path not found in {all_paths['maya']}"
        assert any(p.endswith("actions") for p in all_paths["houdini"]), f"Expected path not found in {all_paths['houdini']}"

    @patch("dcc_mcp_core.utils.filesystem.save_actions_paths_config")
    def test_set_default_action_paths(self, mock_save, reset_config):
        """Test setting default action paths."""
        path1 = os.path.join("path", "to", "maya", "default1")
        path2 = os.path.join("path", "to", "maya", "default2")
        paths = [path1, path2]
        set_default_action_paths("maya", paths)

        # Import _config directly here to check internal state
        # Import local modules
        from dcc_mcp_core.utils.filesystem import _config
        assert "maya" in _config.default_actions_paths
        assert len(_config.default_actions_paths["maya"]) == 2
        mock_save.assert_called_once()

    def test_get_all_registered_dccs(self, reset_config):
        """Test getting all registered DCCs."""
        # Set default paths for some DCCs
        # 使用 os.path.join 创建平台无关的路径
        maya_path = os.path.join("path", "to", "maya", "default")
        houdini_path = os.path.join("path", "to", "houdini", "default")
        set_default_action_paths("maya", [maya_path])
        set_default_action_paths("houdini", [houdini_path])

        dccs = get_all_registered_dccs()
        assert "maya" in dccs
        assert "houdini" in dccs


class TestConfigIO:
    """Tests for configuration I/O functions."""

    @patch("builtins.open", new_callable=mock_open)
    @patch("pathlib.Path.mkdir")
    @patch("json.dump")
    def test_save_actions_paths_config(self, mock_json_dump, mock_mkdir, mock_open_file, reset_config):
        """Test saving action paths configuration."""
        # First register a DCC path, but prevent automatic saving
        with patch("dcc_mcp_core.utils.filesystem.save_actions_paths_config"):
            register_dcc_actions_path("maya", "/path/to/maya/actions")

        # Now test the save_actions_paths_config function
        config_path = "/path/to/config.json"

        # Use multiple patches to isolate function behavior
        with patch("builtins.open", new_callable=mock_open) as mock_open_file, \
             patch("pathlib.Path.mkdir") as mock_mkdir, \
             patch("json.dump") as mock_json_dump, \
             patch("dcc_mcp_core.utils.filesystem._load_config_if_needed"):  # Prevent loading configuration

            # Call the function to be tested
            # Import local modules
            from dcc_mcp_core.utils.filesystem import save_actions_paths_config
            result = save_actions_paths_config(config_path)

            # Verify results
            assert result is True
            mock_mkdir.assert_called_once()

            # Check the open function call, handle WindowsPath objects
            assert mock_open_file.call_count == 1, "Expected open to be called once"
            call_args = mock_open_file.call_args[0]
            assert len(call_args) == 2, "Expected open to be called with 2 arguments"

            # Using pathlib.Path to handle path separators
            # Import built-in modules
            from pathlib import Path
            assert Path(str(call_args[0])) == Path(config_path), f"Expected path to be equivalent to '{config_path}', got '{call_args[0]}'"
            assert call_args[1] == "w", f"Expected mode to be 'w', got '{call_args[1]}'"

            mock_json_dump.assert_called_once()

    @patch("builtins.open", new_callable=mock_open, read_data=json.dumps({
        "dcc_actions_paths": {"maya": ["/path/to/maya/actions"]},
        "default_actions_paths": {"maya": ["/path/to/maya/default"]}
    }))
    @patch("pathlib.Path.exists", return_value=True)
    def test_load_actions_paths_config(self, mock_exists, mock_file, reset_config):
        """Test loading action paths configuration."""
        config_path = "/path/to/config.json"
        result = load_actions_paths_config(config_path)

        assert result is True

        # Check the open function call, handle WindowsPath objects
        assert mock_file.call_count == 1, "Expected open to be called once"
        call_args = mock_file.call_args[0]
        assert len(call_args) == 1, "Expected open to be called with 1 argument"

        # Using pathlib.Path to compare paths, ignoring path separator differences
        # Import built-in modules
        from pathlib import Path
        assert Path(str(call_args[0])) == Path(config_path), f"Expected path to be equivalent to '{config_path}', got '{call_args[0]}'"

        # Check if the configuration has been updated
        paths = get_action_paths("maya")
        assert any(Path(p).parts[-1] == "actions" for p in paths), "Expected to find 'actions' in the path"

    @patch("pathlib.Path.exists", return_value=False)
    def test_load_actions_paths_config_nonexistent(self, mock_exists, reset_config):
        """Test loading action paths configuration when file doesn't exist."""
        result = load_actions_paths_config("/path/to/nonexistent.json")

        assert result is False


class TestEnvironmentVariables:
    """Tests for environment variable handling."""

    @patch.dict(os.environ, {
        f"{ENV_ACTION_PATH_PREFIX}maya": "/path/to/maya/env",
        f"{ENV_ACTION_PATH_PREFIX}houdini": "/path/to/houdini/env"
    })
    def test_get_actions_paths_from_env_all(self):
        """Test getting action paths from environment variables for all DCCs."""
        paths = get_actions_paths_from_env()

        assert "maya" in paths
        assert "houdini" in paths
        # Using os.path.normpath or os.sep to handle different path separators
        assert any(os.path.basename(p) == "env" and "maya" in p for p in paths["maya"]), f"Expected path not found in {paths['maya']}"
        assert any(os.path.basename(p) == "env" and "houdini" in p for p in paths["houdini"]), f"Expected path not found in {paths['houdini']}"

    @patch.dict(os.environ, {
        f"{ENV_ACTION_PATH_PREFIX}maya": "/path/to/maya/env",
        f"{ENV_ACTION_PATH_PREFIX}houdini": "/path/to/houdini/env"
    })
    def test_get_actions_paths_from_env_specific(self):
        """Test getting action paths from environment variables for a specific DCC."""
        paths = get_actions_paths_from_env("maya")

        assert "maya" in paths
        # Using os.path.basename and partial path matching for cross-platform compatibility
        assert any(os.path.basename(p) == "env" and "maya" in p for p in paths["maya"]), f"Expected path not found in {paths['maya']}"
        assert "houdini" not in paths

    @patch.dict(os.environ, {ENV_ACTIONS_DIR: "/path/to/actions"})
    def test_get_actions_dir_from_env(self):
        """Test getting actions directory from environment variables."""
        path = get_actions_dir_from_env()

        # Using os.path.basename for cross-platform compatibility
        assert os.path.basename(path) == "actions", f"Expected basename 'actions' not found in {path}"


class TestDiscovery:
    """Tests for action discovery functions."""

    @patch("dcc_mcp_core.utils.filesystem._discover_actions_in_paths")
    def test_discover_actions_specific_dcc(self, mock_discover, reset_config):
        """Test discovering actions for a specific DCC."""
        register_dcc_actions_path("maya", "/path/to/maya/actions")
        mock_discover.return_value = ["/path/to/maya/actions/action1.py"]

        actions = discover_actions("maya")

        assert "maya" in actions
        assert actions["maya"] == ["/path/to/maya/actions/action1.py"]

        # Modify the assertion to check if the added path is included in the paths list
        mock_discover.assert_called_once()
        args, kwargs = mock_discover.call_args
        assert len(args) == 2
        paths, extension = args

        # Check if the paths list contains a path that includes "path/to/maya/actions" or "path\to\maya\actions"
        assert any("path/to/maya/actions" in p or "path\\to\\maya\\actions" in p for p in paths), f"Expected path not found in {paths}"
        assert extension == ".py"

    @patch("dcc_mcp_core.utils.filesystem._discover_actions_in_paths")
    def test_discover_actions_all_dccs(self, mock_discover, reset_config):
        """Test discovering actions for all DCCs."""
        register_dcc_actions_path("maya", "/path/to/maya/actions")
        register_dcc_actions_path("houdini", "/path/to/houdini/actions")
        mock_discover.return_value = ["/path/to/actions/action1.py"]

        actions = discover_actions()

        assert "maya" in actions
        assert "houdini" in actions

        # Check if the function was called multiple times (once for each DCC)
        assert mock_discover.call_count >= 2

        # Check if the call arguments include the added paths
        maya_call_found = False
        houdini_call_found = False

        for call in mock_discover.call_args_list:
            args, kwargs = call
            paths, extension = args

            if any("path/to/maya/actions" in p or "path\\to\\maya\\actions" in p for p in paths):
                maya_call_found = True

            if any("path/to/houdini/actions" in p or "path\\to\\houdini\\actions" in p for p in paths):
                houdini_call_found = True

        assert maya_call_found, "Maya actions path not found in any call"
        assert houdini_call_found, "Houdini actions path not found in any call"

    @patch("pathlib.Path.exists", return_value=True)
    @patch("pathlib.Path.glob")
    def test_discover_actions_in_paths(self, mock_glob, mock_exists):
        """Test discovering actions in specific paths."""
        mock_glob.return_value = [
            Path("/path/to/actions/action1.py"),
            Path("/path/to/actions/_private.py"),  # Should be skipped
            Path("/path/to/actions/__init__.py")   # Should be skipped
        ]

        actions = _discover_actions_in_paths(["/path/to/actions"], ".py")

        assert len(actions) == 1
        # Using Path to handle path separators
        assert Path(actions[0]).name == "action1.py"
        assert "actions" in str(Path(actions[0]))


class TestDirectoryOperations:
    """Tests for directory operations."""

    @patch("pathlib.Path.exists", return_value=False)
    @patch("pathlib.Path.mkdir")
    def test_ensure_directory_exists_create(self, mock_mkdir, mock_exists):
        """Test ensuring a directory exists when it doesn't."""
        result = ensure_directory_exists("/path/to/directory")

        assert result is True
        mock_mkdir.assert_called_once_with(parents=True, exist_ok=True)

    @patch("pathlib.Path.exists", return_value=True)
    @patch("pathlib.Path.mkdir")
    def test_ensure_directory_exists_already(self, mock_mkdir, mock_exists):
        """Test ensuring a directory exists when it already does."""
        result = ensure_directory_exists("/path/to/directory")

        assert result is True
        mock_mkdir.assert_not_called()

    @patch("pathlib.Path.mkdir", side_effect=Exception("Test error"))
    @patch("pathlib.Path.exists", return_value=False)
    def test_ensure_directory_exists_error(self, mock_exists, mock_mkdir):
        """Test ensuring a directory exists when an error occurs."""
        result = ensure_directory_exists("/path/to/directory")

        assert result is False


class TestPathConversion:
    """Tests for path conversion functions."""

    def test_convert_path_to_module_basic(self):
        """Test converting a basic file path to a module path."""
        module_name = convert_path_to_module("/path/to/module.py")
        assert module_name == "module"

    def test_convert_path_to_module_with_extension(self):
        """Test converting a file path with extension to a module path."""
        module_name = convert_path_to_module("/path/to/module.extension.py")
        assert module_name == "module.extension"

    def test_convert_path_to_module_with_directory(self):
        """Test converting a file path with directories to a module path."""
        module_name = convert_path_to_module("/path/to/package/module.py")
        assert module_name == "module"


class TestPythonPathManagement:
    """Tests for Python path management."""

    def test_append_to_python_path(self):
        """Test appending to Python path within a context."""
        # Using an actual existing path instead of a fictional one
        script_path = os.path.abspath(__file__)
        script_dir = os.path.dirname(script_path)

        # Ensure the path is not in sys.path before the test
        if script_dir in sys.path:
            sys.path.remove(script_dir)
        assert script_dir not in sys.path

        # Check if the path is added within the context
        with append_to_python_path(script_path):
            assert script_dir in sys.path

        # Check if the path is removed after the context ends
        assert script_dir not in sys.path


class TestModuleLoading:
    """Tests for module loading functions."""

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    def test_load_module_from_path(self, mock_module_from_spec, mock_spec_from_file, mock_isfile):
        """Test loading a module from a file path."""
        # Setup mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module = MagicMock()
        mock_module_from_spec.return_value = mock_module

        # Call the function
        result = load_module_from_path("/path/to/module.py")

        # Verify the result
        assert result == mock_module
        mock_spec_from_file.assert_called_once()
        mock_module_from_spec.assert_called_once_with(mock_spec)
        mock_spec.loader.exec_module.assert_called_once_with(mock_module)

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    def test_load_module_from_path_with_dependencies(self, mock_module_from_spec, mock_spec_from_file, mock_isfile):
        """Test loading a module with dependencies."""
        # Setup mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module = MagicMock()
        mock_module_from_spec.return_value = mock_module

        # Call the function with dependencies
        dependencies = {"dep1": "value1", "dep2": "value2"}
        result = load_module_from_path("/path/to/module.py", dependencies=dependencies)

        # Verify dependencies were injected
        assert result.dep1 == "value1"
        assert result.dep2 == "value2"

    @patch("os.path.isfile", return_value=False)
    def test_load_module_from_path_file_not_exists(self, mock_isfile):
        """Test loading a module when the file doesn't exist."""
        with pytest.raises(ImportError, match="File does not exist"):
            load_module_from_path("/path/to/nonexistent.py")

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location", return_value=None)
    def test_load_module_from_path_error(self, mock_spec_from_file, mock_isfile):
        """Test loading a module when an error occurs in spec creation."""
        with pytest.raises(ImportError):
            load_module_from_path("/path/to/nonexistent.py")
