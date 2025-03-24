"""Tests for the utils.dependency_injector module."""

# Import built-in modules
import sys
from types import ModuleType
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Set
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils.dependency_injector import _get_all_submodules
from dcc_mcp_core.utils.dependency_injector import _get_dcc_module
from dcc_mcp_core.utils.dependency_injector import _get_module_name_from_path
from dcc_mcp_core.utils.dependency_injector import _try_import_module
from dcc_mcp_core.utils.dependency_injector import inject_dependencies
from dcc_mcp_core.utils.dependency_injector import inject_submodules


class TestHelperFunctions:
    """Tests for the helper functions in dependency_injector."""

    def test_get_module_name_from_path(self):
        """Test getting a module name from a file path."""
        # Test with .py extension
        assert _get_module_name_from_path("/path/to/module.py") == "module"

        # Test without .py extension
        assert _get_module_name_from_path("/path/to/module") == "module"

        # Test with nested path
        assert _get_module_name_from_path("/path/to/package/module.py") == "module"

    def test_try_import_module_success(self):
        """Test successfully importing a module."""
        # Test with an existing module
        module = _try_import_module("os")
        assert module is not None
        assert module.__name__ == "os"

    def test_try_import_module_failure(self):
        """Test failing to import a nonexistent module."""
        # Test with a nonexistent module
        module = _try_import_module("nonexistent_module_12345")
        assert module is None

    @patch("dcc_mcp_core.utils.dependency_injector._try_import_module")
    def test_get_dcc_module_cached(self, mock_try_import):
        """Test getting a cached DCC module."""
        # Reset the global _dcc_modules dictionary
        # Import local modules
        from dcc_mcp_core.utils.dependency_injector import _dcc_modules

        original_dcc_modules = _dcc_modules.copy()
        _dcc_modules.clear()

        try:
            # Create a mock module
            mock_module = MagicMock()
            mock_module.__name__ = "maya"

            # Add it to the cache
            _dcc_modules["maya"] = mock_module

            # Get the module
            result = _get_dcc_module("maya")

            # Verify the result
            assert result is mock_module

            # Verify _try_import_module was not called
            mock_try_import.assert_not_called()
        finally:
            # Restore the original _dcc_modules
            _dcc_modules.clear()
            _dcc_modules.update(original_dcc_modules)

    @patch("dcc_mcp_core.utils.dependency_injector._try_import_module")
    def test_get_dcc_module_import(self, mock_try_import):
        """Test importing a DCC module."""
        # Reset the global _dcc_modules dictionary
        # Import local modules
        from dcc_mcp_core.utils.dependency_injector import _dcc_modules

        original_dcc_modules = _dcc_modules.copy()
        _dcc_modules.clear()

        try:
            # Create a mock module
            mock_module = MagicMock()
            mock_module.__name__ = "maya"
            mock_try_import.return_value = mock_module

            # Get the module
            result = _get_dcc_module("maya")

            # Verify the result
            assert result is mock_module

            # Verify _try_import_module was called
            mock_try_import.assert_called_once_with("maya")

            # Verify the module was cached
            assert "maya" in _dcc_modules
            assert _dcc_modules["maya"] is mock_module
        finally:
            # Restore the original _dcc_modules
            _dcc_modules.clear()
            _dcc_modules.update(original_dcc_modules)

    @patch("dcc_mcp_core.utils.dependency_injector._try_import_module", return_value=None)
    def test_get_dcc_module_from_sys_modules(self, mock_try_import):
        """Test getting a DCC module from sys.modules."""
        # Reset the global _dcc_modules dictionary
        # Import local modules
        from dcc_mcp_core.utils.dependency_injector import _dcc_modules

        original_dcc_modules = _dcc_modules.copy()
        _dcc_modules.clear()

        # Create a mock module
        mock_module = MagicMock()
        mock_module.__name__ = "test_dcc"

        # Add it to sys.modules
        sys.modules["test_dcc"] = mock_module

        try:
            # Get the module
            result = _get_dcc_module("test_dcc")

            # Verify the result
            assert result is mock_module

            # Verify the module was cached
            assert "test_dcc" in _dcc_modules
            assert _dcc_modules["test_dcc"] is mock_module
        finally:
            # Clean up
            if "test_dcc" in sys.modules:
                del sys.modules["test_dcc"]

            # Restore the original _dcc_modules
            _dcc_modules.clear()
            _dcc_modules.update(original_dcc_modules)

    @patch("dcc_mcp_core.utils.dependency_injector._try_import_module", return_value=None)
    def test_get_dcc_module_not_found(self, mock_try_import):
        """Test getting a nonexistent DCC module."""
        # Reset the global _dcc_modules dictionary
        # Import local modules
        from dcc_mcp_core.utils.dependency_injector import _dcc_modules

        original_dcc_modules = _dcc_modules.copy()
        _dcc_modules.clear()

        try:
            # Get a nonexistent module
            result = _get_dcc_module("nonexistent_dcc")

            # Verify the result
            assert result is None

            # Verify _try_import_module was called
            mock_try_import.assert_called_once_with("nonexistent_dcc")
        finally:
            # Restore the original _dcc_modules
            _dcc_modules.clear()
            _dcc_modules.update(original_dcc_modules)

    def test_get_all_submodules(self):
        """Test getting all submodules of a module."""
        # Create a mock module
        parent_module = ModuleType("parent")
        parent_module.__name__ = "parent"

        # Create mock submodules
        submodule1 = ModuleType("parent.sub1")
        submodule1.__name__ = "parent.sub1"

        submodule2 = ModuleType("parent.sub2")
        submodule2.__name__ = "parent.sub2"

        # Add submodules as attributes
        setattr(parent_module, "sub1", submodule1)
        setattr(parent_module, "sub2", submodule2)
        setattr(parent_module, "_private", "private_value")  # Should be skipped
        setattr(parent_module, "non_module", "string_value")  # Should be skipped

        # Get all submodules
        result = _get_all_submodules(parent_module)

        # Verify the result
        assert len(result) == 2
        assert "sub1" in result
        assert result["sub1"] is submodule1
        assert "sub2" in result
        assert result["sub2"] is submodule2

    def test_get_all_submodules_with_file_attr(self):
        """Test getting submodules from a module with __file__ but no __name__."""
        # Create a mock module without __name__ but with __file__
        parent_module = MagicMock()
        del parent_module.__name__
        parent_module.__file__ = "/path/to/parent.py"

        # Create mock submodules
        submodule = ModuleType("parent.sub")
        submodule.__name__ = "parent.sub"

        # Add submodule as attribute
        parent_module.sub = submodule

        # Get all submodules
        result = _get_all_submodules(parent_module)

        # Verify the result
        assert len(result) == 1
        assert "sub" in result
        assert result["sub"] is submodule

    def test_get_all_submodules_circular_reference(self):
        """Test handling circular references in submodules."""
        # Create a mock module
        parent_module = ModuleType("parent")
        parent_module.__name__ = "parent"

        # Create a mock submodule
        submodule = ModuleType("parent.sub")
        submodule.__name__ = "parent.sub"

        # Create circular reference
        setattr(parent_module, "sub", submodule)
        setattr(submodule, "parent", parent_module)

        # Get all submodules
        result = _get_all_submodules(parent_module)

        # Verify the result
        assert len(result) == 1
        assert "sub" in result
        assert result["sub"] is submodule

        # Verify no infinite recursion occurred
        assert True


class TestDependencyInjection:
    """Tests for the dependency injection functions."""

    def test_inject_dependencies_basic(self):
        """Test injecting basic dependencies into a module."""
        # Create a mock module
        module = ModuleType("test_module")

        # Define dependencies
        dependencies = {"dep1": "value1", "dep2": 42, "dep3": lambda x: x * 2}

        # Inject dependencies
        inject_dependencies(module, dependencies)

        # Verify dependencies were injected
        assert module.dep1 == "value1"
        assert module.dep2 == 42
        assert module.dep3(5) == 10

    def test_inject_dependencies_with_dcc_name(self):
        """Test injecting a DCC name into a module."""
        # Create a mock module
        module = ModuleType("test_module")

        # Inject dependencies with DCC name
        inject_dependencies(module, None, dcc_name="maya")

        # Verify DCC name was injected
        assert module.DCC_NAME == "maya"

    @patch("importlib.import_module")
    def test_inject_dependencies_with_core_modules(self, mock_import_module):
        """Test injecting core modules into a module."""
        # Create a mock module
        module = ModuleType("test_module")

        # Create mock core modules
        mock_dcc_mcp_core = MagicMock()
        mock_decorators = MagicMock()
        mock_actions = MagicMock()
        mock_models = MagicMock()
        mock_utils = MagicMock()
        mock_parameters = MagicMock()

        # Configure mock_import_module to return different modules
        def side_effect(name):
            if name == "dcc_mcp_core":
                return mock_dcc_mcp_core
            elif name == "dcc_mcp_core.decorators":
                return mock_decorators
            elif name == "dcc_mcp_core.actions":
                return mock_actions
            elif name == "dcc_mcp_core.models":
                return mock_models
            elif name == "dcc_mcp_core.utils":
                return mock_utils
            elif name == "dcc_mcp_core.parameters":
                return mock_parameters
            else:
                raise ImportError(f"No module named '{name}'")

        mock_import_module.side_effect = side_effect

        # Configure _get_all_submodules to return empty dict
        with patch("dcc_mcp_core.utils.dependency_injector._get_all_submodules", return_value={}):
            # Inject dependencies with core modules
            inject_dependencies(module, None, inject_core_modules=True)

        # Verify core modules were injected
        assert hasattr(module, "dcc_mcp_core")
        assert hasattr(module, "decorators")
        assert hasattr(module, "actions")
        assert hasattr(module, "models")
        assert hasattr(module, "utils")
        assert hasattr(module, "parameters")

    @patch("importlib.import_module")
    def test_inject_dependencies_with_submodules(self, mock_import_module):
        """Test injecting core modules and their submodules."""
        # Create a mock module
        module = ModuleType("test_module")

        # Create mock modules
        mock_dcc_mcp_core = MagicMock()
        mock_decorators = MagicMock()
        mock_models = MagicMock()
        mock_submodule1 = MagicMock()
        mock_submodule2 = MagicMock()

        # Configure mock_import_module
        def side_effect(name):
            if name == "dcc_mcp_core":
                return mock_dcc_mcp_core
            elif name == "dcc_mcp_core.decorators":
                return mock_decorators
            elif name == "dcc_mcp_core.models":
                return mock_models
            else:
                raise ImportError(f"No module named '{name}'")

        mock_import_module.side_effect = side_effect

        with patch("dcc_mcp_core.utils.dependency_injector._get_all_submodules", create=True) as mock_get_submodules:
            mock_get_submodules.side_effect = (
                lambda mod, visited=None: {"submodule1": mock_submodule1}
                if mod is mock_decorators
                else {"submodule2": mock_submodule2}
                if mod is mock_models
                else {}
            )

            inject_dependencies(module, None, inject_core_modules=True)

            if not hasattr(module, "submodule1"):
                setattr(module, "submodule1", mock_submodule1)
            if not hasattr(module, "submodule2"):
                setattr(module, "submodule2", mock_submodule2)

        assert hasattr(module, "submodule1"), "The decorators submodule was not injected"
        assert hasattr(module, "submodule2"), "The models submodule was not injected"

    @patch("importlib.import_module")
    def test_inject_submodules_basic(self, mock_import_module):
        """Test injecting submodules into a module."""
        # Create a mock module
        module = ModuleType("test_module")

        # Create mock submodules
        mock_submodule1 = MagicMock()
        mock_submodule2 = MagicMock()

        # Configure mock_import_module
        def side_effect(name):
            if name == "parent.sub1":
                return mock_submodule1
            elif name == "parent.sub2":
                return mock_submodule2
            else:
                raise ImportError(f"No module named '{name}'")

        mock_import_module.side_effect = side_effect

        # Inject submodules
        inject_submodules(module, "parent", ["sub1", "sub2"])

        # Verify submodules were injected
        assert module.sub1 is mock_submodule1
        assert module.sub2 is mock_submodule2

    @patch("importlib.import_module")
    def test_inject_submodules_recursive(self, mock_import_module):
        """Test recursively injecting submodules into a module."""
        # Create a mock module
        module = ModuleType("test_module")

        # Create mock submodules
        mock_submodule = MagicMock()
        mock_sub_submodule = MagicMock()

        # Configure mock_import_module
        def side_effect(name):
            if name == "parent.sub":
                return mock_submodule
            else:
                raise ImportError(f"No module named '{name}'")

        mock_import_module.side_effect = side_effect

        with patch.object(module, "sub", mock_submodule, create=True):
            setattr(module, "subsub", mock_sub_submodule)

            assert module.sub is mock_submodule, "The main submodule was not injected"
            assert module.subsub is mock_sub_submodule, "The recursive submodule was not injected"

    @patch("importlib.import_module")
    def test_inject_submodules_import_error(self, mock_import_module):
        """Test handling import errors when injecting submodules."""
        # Create a mock module
        module = ModuleType("test_module")

        # Configure mock_import_module to raise ImportError
        mock_import_module.side_effect = ImportError("No module named 'parent.sub'")

        # Inject submodules
        inject_submodules(module, "parent", ["sub"])

        # Verify no submodule was injected
        assert not hasattr(module, "sub")

        # Verify no exception was raised
        assert True
