"""Dependency Injector.

Provides utility functions for injecting dependencies into dynamically loaded modules.
This is particularly useful for plugin systems where dependencies need to be provided
at runtime, such as when loading action modules from external files.

The main functions are:
- inject_dependencies: Inject dependencies into a module
- inject_submodules: Inject specified submodules into a module

Example:
    >>> import types
    >>> # Create a new module
    >>> my_module = types.ModuleType('my_module')
    >>> # Inject dependencies
    >>> inject_dependencies(my_module, {'my_dependency': 'value'}, dcc_name='maya')
    >>> # Now the module has the dependencies as attributes
    >>> my_module.my_dependency
    'value'
    >>> my_module.DCC_NAME
    'maya'

"""

# Import built-in modules
import importlib
import inspect
import os
from types import ModuleType
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Set


def _get_all_submodules(module: ModuleType, visited: Optional[Set[str]] = None) -> Dict[str, ModuleType]:
    """Recursively get all submodules of a module.

    This function recursively gets all submodules of a module, avoiding circular references.
    It handles modules that may not have a __name__ attribute by using __file__ or a unique ID.

    Args:
        module: The module to get submodules from
        visited: Set of module names that have already been visited (to prevent circular references)

    Returns:
        Dict[str, ModuleType]: Dictionary mapping submodule names to submodule objects

    """
    if visited is None:
        visited = set()

    result = {}

    # Get module name, try other attributes if __name__ does not exist
    if hasattr(module, "__name__"):
        module_name = module.__name__
    elif hasattr(module, "__file__"):
        # If __file__ attribute exists, use filename (without extension) as module name
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
    module: ModuleType,
    dependencies: Optional[Dict[str, Any]] = None,
    inject_core_modules: bool = False,
    dcc_name: Optional[str] = None,
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

    Example:
        >>> import types
        >>> my_module = types.ModuleType('my_module')
        >>> inject_dependencies(
        ...     my_module,
        ...     {'my_dependency': 'value'},
        ...     inject_core_modules=True,
        ...     dcc_name='maya'
        ... )
        >>> my_module.my_dependency
        'value'
        >>> my_module.DCC_NAME
        'maya'
        >>> hasattr(my_module, 'dcc_mcp_core')
        True

    """
    # Inject direct dependencies
    if dependencies is not None:
        for name, obj in dependencies.items():
            setattr(module, name, obj)

    # Inject DCC name if provided
    if dcc_name is not None:
        setattr(module, "DCC_NAME", dcc_name)

    # Inject core modules if requested
    if inject_core_modules:
        _inject_core_modules(module)


def _inject_core_modules(module: ModuleType) -> None:
    """Inject dcc_mcp_core module and its submodules into a module.

    This function injects the dcc_mcp_core module and its common submodules into
    the target module, making them available as attributes.

    Args:
        module: The module to inject core modules into

    """
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
        # Catch all exceptions to prevent failure
        pass


def inject_submodules(
    module: ModuleType, parent_module_name: str, submodule_names: List[str], recursive: bool = False
) -> None:
    """Inject specified submodules into a module.

    This function injects specified submodules from a parent module into a target module,
    making them available as attributes. It can optionally inject submodules recursively.

    Args:
        module: The module to inject submodules into
        parent_module_name: The parent module name
        submodule_names: List of submodule names to inject
        recursive: Whether to recursively inject submodules of submodules

    Example:
        >>> import types
        >>> my_module = types.ModuleType('my_module')
        >>> inject_submodules(my_module, 'dcc_mcp_core', ['actions', 'models'])
        >>> hasattr(my_module, 'actions')
        True
        >>> hasattr(my_module, 'models')
        True

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
