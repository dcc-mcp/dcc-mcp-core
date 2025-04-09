"""Additional tests for module_loader module to improve code coverage.

This module contains tests specifically designed to improve code coverage for
the module_loader utility module.
"""

# Import built-in modules
import os
import sys
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
from pyfakefs.fake_filesystem_unittest import Patcher
import pytest

# Import local modules
from dcc_mcp_core.utils.module_loader import append_to_python_path
from dcc_mcp_core.utils.module_loader import convert_path_to_module
from dcc_mcp_core.utils.module_loader import load_module_from_path


@pytest.fixture
def fs():
    """Set up fake filesystem for testing."""
    with Patcher() as patcher:
        # Create a basic directory structure
        patcher.fs.create_dir("/test")
        patcher.fs.create_dir("/test/package")
        patcher.fs.create_file("/test/package/__init__.py", contents="")
        patcher.fs.create_file(
            "/test/package/module.py",
            contents="""# Test module
TEST_VARIABLE = 'test_value'

def test_function():
    return 'test_result'
""",
        )
        patcher.fs.create_file(
            "/test/package/error_module.py",
            contents="""# Module with syntax error
TEST_VARIABLE = 'test_value'

def test_function():
    return 'test_result'

# Syntax error
if True
    print('Error')
""",
        )

        yield patcher.fs


def test_convert_path_to_module():
    """Test convert_path_to_module function."""
    # Test with various paths
    assert convert_path_to_module("/test/package/module.py") == "module"
    assert convert_path_to_module("/test/package/__init__.py") == "__init__"
    assert convert_path_to_module("/test/file.txt") == "file"

    # Test with hyphenated name
    assert convert_path_to_module("/test/hyphenated-name.py") == "hyphenated_name"


def test_append_to_python_path(fs):
    """Test append_to_python_path function."""
    # Test with file path
    test_path = os.path.normpath("/test/package/module.py")
    original_sys_path = sys.path.copy()

    with append_to_python_path(test_path):
        expected_path = os.path.normpath("/test/package")
        assert any(os.path.normpath(p) == expected_path for p in sys.path)

    # After context exit, path should be removed
    expected_path = os.path.normpath("/test/package")
    assert not any(os.path.normpath(p) == expected_path for p in sys.path)
    assert len(sys.path) == len(original_sys_path)

    # Test with directory path
    test_dir = os.path.normpath("/test/package")
    with append_to_python_path(test_dir):
        assert any(os.path.normpath(p) == test_dir for p in sys.path)

    # Test with path already in sys.path
    normalized_path = os.path.normpath("/test/package")
    sys.path.insert(0, normalized_path)
    with append_to_python_path(os.path.normpath("/test/package/module.py")):
        assert any(os.path.normpath(p) == normalized_path for p in sys.path)

    # Path should still be in sys.path because it was there before
    assert any(os.path.normpath(p) == normalized_path for p in sys.path)

    # Clean up
    for i, p in enumerate(sys.path):
        if os.path.normpath(p) == normalized_path:
            sys.path.pop(i)
            break


def test_load_module_from_path(fs):
    """Test load_module_from_path function."""
    # Use fakefs to create a test file
    test_dir = os.path.join(os.getcwd(), "test_modules")
    fs.create_dir(test_dir)
    test_file = os.path.join(test_dir, "test_module.py")

    fs.create_file(
        test_file,
        contents="""# Test module
TEST_VARIABLE = 'test_value'

def test_function():
    return 'test_result'
""",
    )

    # Test loading a valid module
    with patch("importlib.util.spec_from_file_location") as mock_spec:
        # Mock the spec and loader
        mock_loader = MagicMock()
        mock_loader.exec_module = MagicMock()
        mock_spec.return_value = MagicMock()
        mock_spec.return_value.loader = mock_loader

        # Mock module_from_spec
        mock_module = MagicMock()
        mock_module.__name__ = "test_module"
        mock_module.TEST_VARIABLE = "test_value"
        mock_module.test_function = MagicMock(return_value="test_result")

        with patch("importlib.util.module_from_spec", return_value=mock_module):
            # Test loading a valid module
            module = load_module_from_path(test_file)
            assert module is not None
            assert module.TEST_VARIABLE == "test_value"
            assert module.test_function() == "test_result"

            # Test loading a module with a custom name
            module = load_module_from_path(test_file, "custom_name")
            assert module.__name__ == "custom_name"

            # Test with dependencies
            with patch("dcc_mcp_core.utils.module_loader.inject_dependencies") as mock_inject:
                mock_inject.return_value = mock_module
                dependencies = {"INJECTED_VALUE": "injected_test"}
                module = load_module_from_path(test_file, dependencies=dependencies)
                assert mock_inject.call_count > 0

            # Test with dcc_name
            with patch("dcc_mcp_core.utils.module_loader.inject_dependencies") as mock_inject:
                mock_inject.return_value = mock_module
                module = load_module_from_path(test_file, dcc_name="maya")
                assert mock_inject.call_count > 0

    # Test loading a non-existent file
    non_existent_file = os.path.join(test_dir, "non_existent.py")
    with pytest.raises(ImportError):
        load_module_from_path(non_existent_file)

    # Test with module execution error
    with patch("importlib.util.spec_from_file_location") as mock_spec:
        mock_spec.return_value = MagicMock()
        mock_spec.return_value.loader = MagicMock()
        mock_spec.return_value.loader.exec_module = MagicMock(side_effect=Exception("Test execution error"))

        with pytest.raises(ImportError):
            load_module_from_path(test_file)


def test_load_module_from_path_with_spec_none(fs):
    """Test load_module_from_path function when spec is None."""
    # Test with spec_from_file_location returning None
    with patch("importlib.util.spec_from_file_location") as mock_spec:
        mock_spec.return_value = None

        with pytest.raises(ImportError):
            load_module_from_path("/test/package/module.py")
