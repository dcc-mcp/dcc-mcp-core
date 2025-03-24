"""Dependency Injector.

Provides utility functions for injecting dependencies into dynamically loaded modules.
"""

# Import built-in modules
# Import standard modules
import importlib
import inspect
import sys
from types import ModuleType
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Set

# Global variables
_dcc_modules = {}


# Helper functions
def _get_module_name_from_path(file_path: str) -> str:
    """Generate a module name from a file path."""
    # Import built-in modules
    import os

    module_name = os.path.basename(file_path)
    if module_name.endswith(".py"):
        module_name = module_name[:-3]
    return module_name


def _try_import_module(module_name: str) -> Optional[ModuleType]:
    """Try to import a module, return None if it fails."""
    try:
        return importlib.import_module(module_name)
    except (ImportError, ModuleNotFoundError):
        return None


def _get_dcc_module(dcc_name: str) -> Optional[ModuleType]:
    """Get a DCC module if it exists."""
    global _dcc_modules
    dcc_name = dcc_name.lower()

    # If the module has already been loaded, return it directly
    if dcc_name in _dcc_modules and _dcc_modules[dcc_name] is not None:
        return _dcc_modules[dcc_name]

    # Try to load the DCC module
    module = _try_import_module(dcc_name)
    if module is not None:
        _dcc_modules[dcc_name] = module
        return module

    # If it exists in sys.modules, use it
    if dcc_name in sys.modules:
        _dcc_modules[dcc_name] = sys.modules[dcc_name]
        return sys.modules[dcc_name]

    return None


def _get_all_submodules(module: ModuleType, visited: Optional[Set[str]] = None) -> Dict[str, ModuleType]:
    """Recursively get all submodules of a module."""
    if visited is None:
        visited = set()

    result = {}

    # Get module name, try other attributes if __name__ does not exist
    if hasattr(module, "__name__"):
        module_name = module.__name__
    elif hasattr(module, "__file__"):
        # If __file__ attribute exists, use filename (without extension) as module name
        # Import built-in modules
        import os

        module_name = os.path.splitext(os.path.basename(module.__file__))[0]
    else:
        # If no available identifier, use module object id as unique identifier
        module_name = f"unknown_module_{id(module)}"

    # Prevent circular references
    if module_name in visited:
        return result

    visited.add(module_name)

    # Get all attributes of the module
    for attr_name, attr_value in inspect.getmembers(module):
        # Skip private and special attributes
        if attr_name.startswith("_"):
            continue

        # If it's a module, add it to the result
        if inspect.ismodule(attr_value):
            # Make sure it's a submodule
            if hasattr(attr_value, "__name__") and attr_value.__name__.startswith(module_name + "."):
                submodule_name = attr_value.__name__.split(".")[-1]
                result[submodule_name] = attr_value

    return result


def inject_dependencies(
    module: ModuleType, dependencies: Dict[str, Any], inject_core_modules: bool = False, dcc_name: Optional[str] = None
) -> None:
    """Inject dependencies into a module.

    This function injects dependencies into a module, making them available as attributes.
    This is particularly useful for plugin systems where dependencies need to be provided
    at runtime.

    Args:
        module: The module to inject dependencies into
        dependencies: Dictionary of dependencies to inject, keys are attribute names, values are objects
        inject_core_modules: If True, also inject the dcc_mcp_core module and its submodules
        dcc_name: Name of the DCC to inject as a module attribute

    """
    # Inject direct dependencies
    if dependencies is not None:
        for name, obj in dependencies.items():
            setattr(module, name, obj)

    # Inject DCC name if provided
    if dcc_name is not None:
        setattr(module, "DCC_NAME", dcc_name)

    if inject_core_modules:
        try:
            # Import the core module
            try:
                # Import local modules
                import dcc_mcp_core

                # Inject main module
                setattr(module, "dcc_mcp_core", dcc_mcp_core)

                # Inject common submodules
                core_submodules = ["decorators", "actions", "models", "utils", "parameters"]

                # Inject all submodules
                for submodule_name in core_submodules:
                    full_module_name = f"dcc_mcp_core.{submodule_name}"
                    try:
                        submodule = importlib.import_module(full_module_name)
                        setattr(module, submodule_name, submodule)

                        # For key modules, also inject their submodules
                        if submodule_name in ["decorators", "models"]:
                            try:
                                sub_submodules = _get_all_submodules(submodule)
                                for sub_name, sub_module in sub_submodules.items():
                                    setattr(module, sub_name, sub_module)
                            except Exception:
                                pass
                    except ImportError:
                        # Skip if submodule does not exist
                        pass
            except ImportError:
                # Skip if core module cannot be imported
                pass
        except Exception:
            pass


def inject_submodules(
    module: ModuleType, parent_module_name: str, submodule_names: List[str], recursive: bool = False
) -> None:
    """Inject specified submodules into a module.

    Args:
        module: The module to inject submodules into
        parent_module_name: The parent module name
        submodule_names: List of submodule names to inject
        recursive: Whether to recursively inject submodules of submodules

    """
    for submodule_name in submodule_names:
        full_module_name = f"{parent_module_name}.{submodule_name}"
        try:
            submodule = importlib.import_module(full_module_name)
            setattr(module, submodule_name, submodule)

            # If recursive injection is needed, get and inject submodules of submodules
            if recursive:
                sub_submodules = _get_all_submodules(submodule)
                for sub_name, sub_module in sub_submodules.items():
                    setattr(module, sub_name, sub_module)
        except ImportError:
            # Skip if submodule does not exist
            pass
