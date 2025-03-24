"""Tests for module_loader.py."""

# Import standard modules
# Import built-in modules
import os
from pathlib import Path
import sys
import tempfile

# Import third-party modules
from pyfakefs.fake_filesystem_unittest import Patcher
import pytest

# Import local modules
from dcc_mcp_core.utils.module_loader import load_module_from_path


@pytest.fixture
def temp_module_file():
    """Create a temporary Python module file for testing."""
    with tempfile.NamedTemporaryFile(suffix=".py", delete=False) as f:
        f.write(b'''# -*- coding: utf-8 -*-
"""
Test module for load_module_from_path
"""
import sys

def test_function():
    return "test_function_result"

def get_dependency(name):
    """Get dependency object"""
    if hasattr(sys.modules[__name__], name):
        return getattr(sys.modules[__name__], name)
    return None
''')
        module_path = f.name

    yield module_path

    # 清理临时文件
    try:
        os.unlink(module_path)
    except (OSError, PermissionError):
        pass


def test_load_module_from_path_basic(temp_module_file):
    """Test basic module loading functionality."""
    # Load module
    module = load_module_from_path(temp_module_file)

    # Verify module is loaded correctly
    assert hasattr(module, "test_function")
    assert module.test_function() == "test_function_result"

    # Verify default dependency is injected
    assert hasattr(module, "dcc_mcp_core")


def test_load_module_from_path_with_dependencies(temp_module_file):
    """Test module loading with dependencies."""
    # Create test dependency
    test_dep = {"key": "value"}

    # Load module and inject dependency
    module = load_module_from_path(temp_module_file, dependencies={"test_dep": test_dep})

    # Verify dependency is injected
    assert hasattr(module, "test_dep")
    assert module.test_dep == test_dep
    assert module.get_dependency("test_dep") == test_dep


def test_load_module_from_path_with_custom_name(temp_module_file):
    """Test module loading with custom name."""
    custom_name = "custom_module_name"

    # Load module with custom name
    module = load_module_from_path(temp_module_file, module_name=custom_name)

    # Verify module name is set correctly
    assert module.__name__ == custom_name


def test_load_module_from_path_file_not_found():
    """Test loading non-existent file."""
    non_existent_file = "/path/to/non_existent_file.py"

    # Verify loading non-existent file raises ImportError
    with pytest.raises(ImportError) as excinfo:
        load_module_from_path(non_existent_file)

    # Verify error message contains file path
    assert "File does not exist" in str(excinfo.value)
    assert non_existent_file in str(excinfo.value)


def test_load_module_from_path_with_fakefs(tmpdir):
    """Test module loading with pytest's temporary directory."""
    # Create a module file in the temporary directory
    module_path = tmpdir.join("fake_module.py")
    module_path.write("""
# -*- coding: utf-8 -*-
def fake_function():
    return "fake_result"
""")

    # Load the module
    module = load_module_from_path(str(module_path))

    # Verify the module is loaded correctly
    assert hasattr(module, "fake_function")
    assert module.fake_function() == "fake_result"
