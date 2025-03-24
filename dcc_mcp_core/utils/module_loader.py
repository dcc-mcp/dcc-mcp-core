"""Module loader.

Provides utility functions for dynamically loading Python modules.

"""

# Import built-in modules
# Import standard modules
import importlib.util
import os
import sys
from types import ModuleType
from typing import Any
from typing import Dict
from typing import Optional

# Import local modules
from dcc_mcp_core.utils.dependency_injector import inject_dependencies


def load_module_from_path(
    file_path: str,
    module_name: Optional[str] = None,
    dependencies: Optional[Dict[str, Any]] = None,
    dcc_name: Optional[str] = None,
) -> ModuleType:
    """Load Python module from file path and inject dependencies.

    This function allows loading Python modules directly from file paths and injecting necessary dependencies
    into the module. This is particularly useful for loading plugins or actions that may depend on specific
    environments.

    Args:
        file_path: Path to the Python file to load
        module_name: Optional module name, will be generated from file name if not provided
        dependencies: Optional dictionary of dependencies to inject into the module, with keys as attribute
            names and values as objects
        dcc_name: Optional DCC name to inject DCC-specific dependencies

    Returns:
        Loaded Python module

    Raises:
        ImportError: If the file does not exist or cannot be loaded

    """
    # Ensure file exists
    if not os.path.isfile(file_path):
        raise ImportError(f"File does not exist: {file_path}")

    # If module name is not provided, generate from file name
    if module_name is None:
        module_name = os.path.splitext(os.path.basename(file_path))[0]

    # Create module specification
    spec = importlib.util.spec_from_file_location(module_name, file_path)
    if spec is None:
        raise ImportError(f"Unable to create module specification for {file_path}")

    # Create module from specification
    module = importlib.util.module_from_spec(spec)

    # Set module's __name__ attribute properly
    module.__name__ = module_name
    module.__file__ = file_path

    # Add module to sys.modules, making it importable by other modules
    sys.modules[module_name] = module

    # Inject dependencies using dependency injector
    inject_dependencies(module, dependencies, inject_core_modules=True, dcc_name=dcc_name)

    # Execute module code
    try:
        spec.loader.exec_module(module)
    except Exception as e:
        # If module execution fails, remove from sys.modules and re-raise
        if module_name in sys.modules:
            del sys.modules[module_name]
        raise ImportError(f"Failed to execute module {module_name}: {e!s}")

    # Keep module in sys.modules to ensure proper imports between modules
    # This is important for modules that import each other

    return module
