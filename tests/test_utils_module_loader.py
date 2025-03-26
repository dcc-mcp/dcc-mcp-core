"""Tests for the utils.module_loader module."""

# Import built-in modules
import os
import sys
from types import ModuleType
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils.module_loader import load_module_from_path


class TestModuleLoader:
    """Tests for the module_loader functions."""

    @pytest.fixture
    def mock_module(self):
        """Create a mock module for testing."""
        module = ModuleType("test_module")
        module.__file__ = "/path/to/test_module.py"
        return module

    @pytest.fixture
    def temp_python_file(self, tmp_path):
        """Create a temporary Python file for testing."""
        file_path = tmp_path / "test_module.py"
        with open(file_path, "w") as f:
            f.write('"""Test module for testing module_loader."""\n')
            f.write("\n")
            f.write('TEST_VARIABLE = "test_value"\n')
            f.write("\n")
            f.write("def test_function():\n")
            f.write('    """Test function."""\n')
            f.write('    return "test_result"\n')
        return file_path

    def test_load_module_from_path_nonexistent_file(self):
        """Test loading a module from a nonexistent file path."""
        with pytest.raises(ImportError, match="File does not exist"):
            load_module_from_path("/path/to/nonexistent.py")

    @patch("importlib.util.spec_from_file_location")
    def test_load_module_from_path_invalid_spec(self, mock_spec_from_file):
        """Test loading a module with an invalid spec."""
        # Set up mock to return None for spec
        mock_spec_from_file.return_value = None

        with pytest.raises(ImportError, match="Unable to create module specification"):
            # Use a path that exists but will generate an invalid spec
            load_module_from_path(__file__)

    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    @patch("dcc_mcp_core.utils.module_loader.inject_dependencies")
    def test_load_module_from_path_exec_failure(
        self, mock_inject, mock_module_from_spec, mock_spec_from_file, mock_module
    ):
        """Test loading a module that fails during execution."""
        # Set up mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module_from_spec.return_value = mock_module

        # Make exec_module raise an exception
        mock_spec.loader.exec_module.side_effect = Exception("Test execution error")

        with pytest.raises(ImportError, match="Failed to execute module"):
            load_module_from_path(__file__)

        # Verify the module was removed from sys.modules
        assert mock_module.__name__ not in sys.modules

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    @patch("dcc_mcp_core.utils.module_loader.inject_dependencies")
    def test_load_module_from_path_success(
        self, mock_inject, mock_module_from_spec, mock_spec_from_file, mock_isfile, mock_module
    ):
        """Test successfully loading a module from a file path."""
        # Set up mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module_from_spec.return_value = mock_module

        # Call the function
        load_module_from_path("/path/to/test_module.py")

        # Verify the module was added to sys.modules
        assert mock_module.__name__ in sys.modules
        assert sys.modules[mock_module.__name__] == mock_module

        # Verify dependencies were injected
        mock_inject.assert_called_once_with(mock_module, None, inject_core_modules=True, dcc_name=None)

        # Verify the module was executed
        mock_spec.loader.exec_module.assert_called_once_with(mock_module)

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    @patch("dcc_mcp_core.utils.module_loader.inject_dependencies")
    def test_load_module_from_path_with_custom_name(
        self, mock_inject, mock_module_from_spec, mock_spec_from_file, mock_isfile, mock_module
    ):
        """Test loading a module with a custom module name."""
        # Set up mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module_from_spec.return_value = mock_module

        # Call the function with a custom module name
        load_module_from_path("/path/to/test_module.py", module_name="custom_name")

        # Verify the module was added to sys.modules with the custom name
        assert "custom_name" in sys.modules
        assert sys.modules["custom_name"] == mock_module

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    @patch("dcc_mcp_core.utils.module_loader.inject_dependencies")
    def test_load_module_from_path_with_dependencies(
        self, mock_inject, mock_module_from_spec, mock_spec_from_file, mock_isfile, mock_module
    ):
        """Test loading a module with dependencies."""
        # Set up mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module_from_spec.return_value = mock_module

        # Define dependencies
        dependencies = {"dep1": "value1", "dep2": "value2"}

        # Call the function with dependencies
        load_module_from_path("/path/to/test_module.py", dependencies=dependencies)

        # Verify dependencies were injected
        mock_inject.assert_called_once_with(mock_module, dependencies, inject_core_modules=True, dcc_name=None)

    @patch("os.path.isfile", return_value=True)
    @patch("importlib.util.spec_from_file_location")
    @patch("importlib.util.module_from_spec")
    @patch("dcc_mcp_core.utils.module_loader.inject_dependencies")
    def test_load_module_from_path_with_dcc_name(
        self, mock_inject, mock_module_from_spec, mock_spec_from_file, mock_isfile, mock_module
    ):
        """Test loading a module with a DCC name."""
        # Set up mocks
        mock_spec = MagicMock()
        mock_spec_from_file.return_value = mock_spec
        mock_module_from_spec.return_value = mock_module

        # Call the function with a DCC name
        load_module_from_path("/path/to/test_module.py", dcc_name="maya")

        # Verify DCC name was passed to inject_dependencies
        mock_inject.assert_called_once_with(mock_module, None, inject_core_modules=True, dcc_name="maya")

    def test_load_module_from_path_integration(self, temp_python_file):
        """Integration test for loading a real module from a file path."""
        # Skip cleanup of sys.modules to avoid affecting other tests
        module_name = os.path.splitext(os.path.basename(temp_python_file))[0]

        try:
            # Load the module
            module = load_module_from_path(str(temp_python_file))

            # Verify the module was loaded correctly
            assert isinstance(module, ModuleType)
            assert module.__name__ == module_name
            assert module.__file__ == str(temp_python_file)
            assert module.TEST_VARIABLE == "test_value"
            assert module.test_function() == "test_result"

            # Verify the module was added to sys.modules
            assert module_name in sys.modules
            assert sys.modules[module_name] == module
        finally:
            # Clean up sys.modules
            if module_name in sys.modules:
                del sys.modules[module_name]
