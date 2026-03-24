"""Additional tests for filesystem module to improve code coverage.

This module contains tests specifically designed to improve code coverage for
the filesystem utility module.
"""

# Import built-in modules
import os
from unittest.mock import patch

# Import third-party modules
from pyfakefs.fake_filesystem_unittest import Patcher
import pytest

# Import local modules
from dcc_mcp_core.constants import APP_AUTHOR
from dcc_mcp_core.constants import APP_NAME
from dcc_mcp_core.utils.filesystem import ensure_directory_exists
from dcc_mcp_core.utils.filesystem import get_actions_dir
from dcc_mcp_core.utils.filesystem import get_actions_paths_from_env
from dcc_mcp_core.utils.filesystem import get_config_dir
from dcc_mcp_core.utils.filesystem import get_data_dir
from dcc_mcp_core.utils.filesystem import get_log_dir
from dcc_mcp_core.utils.filesystem import get_platform_dir
from dcc_mcp_core.utils.filesystem import get_templates_directory
from dcc_mcp_core.utils.filesystem import get_user_actions_directory
from dcc_mcp_core.utils.filesystem import get_user_data_dir


@pytest.fixture
def fs():
    """Set up fake filesystem for testing."""
    with Patcher() as patcher:
        # Create a basic directory structure
        patcher.fs.create_dir("/test")
        patcher.fs.create_dir("/test/package")
        patcher.fs.create_dir("/test/package/subpackage")
        patcher.fs.create_file("/test/package/__init__.py")
        patcher.fs.create_file("/test/package/module.py")
        patcher.fs.create_file("/test/package/subpackage/__init__.py")
        patcher.fs.create_file("/test/package/subpackage/module.py")
        patcher.fs.create_file("/test/package/not_a_module.txt")

        # Create actions directory structure
        patcher.fs.create_dir("/test/actions")
        patcher.fs.create_dir("/test/actions/maya")
        patcher.fs.create_dir("/test/actions/nuke")
        patcher.fs.create_file("/test/actions/maya/action1.py")
        patcher.fs.create_file("/test/actions/nuke/action2.py")

        yield patcher.fs


def test_ensure_directory_exists(fs):
    """Test ensure_directory_exists function."""
    # Test creating a new directory
    new_dir = "/test/new_dir"
    assert not os.path.exists(new_dir)
    result = ensure_directory_exists(new_dir)
    assert result is True
    assert os.path.exists(new_dir)
    assert os.path.isdir(new_dir)

    # Test with an existing directory
    result = ensure_directory_exists(new_dir)  # Should not raise an exception
    assert result is True

    # Test with a file path
    file_path = "/test/file.txt"
    fs.create_file(file_path)
    dir_path = os.path.dirname(file_path)
    result = ensure_directory_exists(dir_path)  # Should not raise an exception
    assert result is True


def test_get_actions_paths_from_env():
    """Test get_actions_paths_from_env function."""
    # Test with no environment variables set
    with patch.dict(os.environ, {}, clear=True):
        with patch("dcc_mcp_core.utils.filesystem.get_user_actions_directory") as mock_get_user_actions:
            mock_get_user_actions.return_value = "/test/user/actions/maya"
            paths = get_actions_paths_from_env("maya")
            assert isinstance(paths, list)
            assert len(paths) == 1  # Should include the user actions directory

    # Test with environment variables set - On Windows, path separator is semicolon instead of colon
    with patch.dict(os.environ, {"DCC_MCP_ACTION_PATH_MAYA": "/test/path1;/test/path2"}, clear=True):
        with patch("dcc_mcp_core.utils.filesystem.get_user_actions_directory") as mock_get_user_actions:
            mock_get_user_actions.return_value = "/test/user/actions/maya"
            paths = get_actions_paths_from_env("maya")
            assert isinstance(paths, list)
            assert len(paths) == 1  # Should include the user actions directory
            assert "/test/user/actions/maya" in paths

    # Test with None dcc_name
    with patch.dict(os.environ, {}, clear=True):
        paths = get_actions_paths_from_env(None)
        assert isinstance(paths, list)
        assert len(paths) == 0


def test_get_actions_dir(fs):
    """Test get_actions_dir function."""
    # Test with maya DCC
    with patch("dcc_mcp_core.utils.filesystem.get_data_dir") as mock_get_data_dir:
        mock_get_data_dir.return_value = "/test/data"

        maya_actions_dir = get_actions_dir("maya")
        assert maya_actions_dir == os.path.join("/test/data", "actions", "maya")

        # Test with ensure_exists=False
        maya_actions_dir = get_actions_dir("maya", ensure_exists=False)
        assert maya_actions_dir == os.path.join("/test/data", "actions", "maya")

    # Test with houdini DCC
    with patch("dcc_mcp_core.utils.filesystem.get_data_dir") as mock_get_data_dir:
        mock_get_data_dir.return_value = "/test/data"

        houdini_actions_dir = get_actions_dir("houdini")
        assert houdini_actions_dir == os.path.join("/test/data", "actions", "houdini")


def test_get_templates_directory():
    """Test get_templates_directory function."""
    # Test that the function returns a string
    template_dir = get_templates_directory()
    assert isinstance(template_dir, str)
    assert len(template_dir) > 0

    # Test with mocked Path
    with patch("dcc_mcp_core.utils.filesystem.Path") as mock_path:
        mock_path.return_value.parent.__truediv__.return_value.resolve.return_value = "/test/templates"
        template_dir = get_templates_directory()
        assert template_dir == "/test/templates"


def test_get_user_actions_directory(fs):
    """Test get_user_actions_directory function."""
    # Test with a specific DCC name
    with patch("dcc_mcp_core.utils.filesystem.get_data_dir") as mock_get_data_dir:
        mock_get_data_dir.return_value = "/test/data"

        # Test with maya DCC
        maya_dir = get_user_actions_directory("maya")
        expected_maya_dir = os.path.join("/test/data", "actions", "maya")
        assert maya_dir == expected_maya_dir

        # Test with houdini DCC
        houdini_dir = get_user_actions_directory("houdini")
        expected_houdini_dir = os.path.join("/test/data", "actions", "houdini")
        assert houdini_dir == expected_houdini_dir


def test_get_user_data_dir():
    """Test get_user_data_dir function."""
    # Test that the function returns a string
    with patch("dcc_mcp_core.utils.filesystem.platformdirs.user_data_dir") as mock_user_data_dir:
        mock_user_data_dir.return_value = "/test/user/data"

        user_data_dir = get_user_data_dir()
        assert isinstance(user_data_dir, str)
        assert user_data_dir == "/test/user/data"

        # Verify that it was called with the correct parameters
        mock_user_data_dir.assert_called_once()


def test_get_config_dir():
    """Test get_config_dir function."""
    # Test with ensure_exists=True
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/config"

        config_dir = get_config_dir(ensure_exists=True)
        assert config_dir == "/test/config"
        mock_get_platform_dir.assert_called_once()

    # Test with ensure_exists=False
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/config"

        config_dir = get_config_dir(ensure_exists=False)
        assert config_dir == "/test/config"
        mock_get_platform_dir.assert_called_once_with("config", APP_NAME, APP_AUTHOR, False)


def test_get_data_dir():
    """Test get_data_dir function."""
    # Test with ensure_exists=True
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/data"

        data_dir = get_data_dir(ensure_exists=True)
        assert data_dir == "/test/data"
        mock_get_platform_dir.assert_called_once()

    # Test with ensure_exists=False
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/data"

        data_dir = get_data_dir(ensure_exists=False)
        assert data_dir == "/test/data"
        mock_get_platform_dir.assert_called_once_with("data", APP_NAME, APP_AUTHOR, False)


def test_get_log_dir():
    """Test get_log_dir function."""
    # Test with ensure_exists=True
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/log"

        log_dir = get_log_dir(ensure_exists=True)
        assert log_dir == "/test/log"
        mock_get_platform_dir.assert_called_once()

    # Test with ensure_exists=False
    with patch("dcc_mcp_core.utils.filesystem.get_platform_dir") as mock_get_platform_dir:
        mock_get_platform_dir.return_value = "/test/log"

        log_dir = get_log_dir(ensure_exists=False)
        assert log_dir == "/test/log"
        mock_get_platform_dir.assert_called_once_with("log", APP_NAME, APP_AUTHOR, False)


def test_get_platform_dir():
    """Test get_platform_dir function."""
    # Test with ensure_exists=True
    with patch("dcc_mcp_core.utils.filesystem.platformdirs") as mock_platformdirs:
        mock_platformdirs.user_config_dir.return_value = "/test/platform/config"
        mock_platformdirs.user_data_dir.return_value = "/test/platform/data"
        mock_platformdirs.user_log_dir.return_value = "/test/platform/log"

        # Test config dir
        with patch("dcc_mcp_core.utils.filesystem.os.makedirs") as mock_makedirs:
            config_dir = get_platform_dir("config", APP_NAME, APP_AUTHOR, True)
            assert config_dir == "/test/platform/config"
            mock_platformdirs.user_config_dir.assert_called_once()
            mock_makedirs.assert_called_once_with("/test/platform/config", exist_ok=True)

        # Test data dir
        with patch("dcc_mcp_core.utils.filesystem.os.makedirs") as mock_makedirs:
            data_dir = get_platform_dir("data", APP_NAME, APP_AUTHOR, True)
            assert data_dir == "/test/platform/data"
            mock_platformdirs.user_data_dir.assert_called_once()
            mock_makedirs.assert_called_once_with("/test/platform/data", exist_ok=True)

        # Test log dir
        with patch("dcc_mcp_core.utils.filesystem.os.makedirs") as mock_makedirs:
            log_dir = get_platform_dir("log", APP_NAME, APP_AUTHOR, True)
            assert log_dir == "/test/platform/log"
            mock_platformdirs.user_log_dir.assert_called_once()
            mock_makedirs.assert_called_once_with("/test/platform/log", exist_ok=True)

    # Test with ensure_exists=False
    with patch("dcc_mcp_core.utils.filesystem.platformdirs") as mock_platformdirs:
        mock_platformdirs.user_config_dir.return_value = "/test/platform/config"

        with patch("dcc_mcp_core.utils.filesystem.os.makedirs") as mock_makedirs:
            config_dir = get_platform_dir("config", APP_NAME, APP_AUTHOR, False)
            assert config_dir == "/test/platform/config"
            mock_platformdirs.user_config_dir.assert_called_once_with(APP_NAME, appauthor=APP_AUTHOR)
            assert not mock_makedirs.called
