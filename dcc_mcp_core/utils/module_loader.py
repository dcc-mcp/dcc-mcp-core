"""Module loader.

Provides utility functions for dynamically loading Python modules.

"""

# Import built-in modules
from contextlib import contextmanager

# Import standard modules
import importlib.util
import os
from pathlib import Path
import sys
from types import ModuleType
from typing import Any
from typing import Dict
from typing import Generator
from typing import Optional
from typing import Union

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


def convert_path_to_module(file_path: str) -> str:
    """Convert a file path to a Python module path.

    This function converts a file path (e.g., 'path/to/module.py') to a
    Python module path that can be used with importlib. For simplicity and to avoid
    issues with absolute paths, it returns only the filename without extension.

    Args:
        file_path: Path to the Python file

    Returns:
        Python module path suitable for importlib.import_module

    """
    # Convert to Path object
    path = Path(file_path)

    # Get the file name without extension
    file_name = path.stem

    # Remove any invalid characters for module names
    module_path = file_name.replace("-", "_")

    return module_path


@contextmanager
def append_to_python_path(script: Union[str, Path]) -> Generator[None, None, None]:
    """Temporarily append a directory to sys.path within a context.

    This context manager adds the directory containing the specified script
    to sys.path and removes it when exiting the context.

    Args:
        script: The absolute path to a script file.

    Yields:
        None

    Example:
        >>> with append_to_python_path('/path/to/script.py'):
        ...     import some_module  # module in script's directory

    """
    script_path = Path(script)
    if script_path.is_file():
        script_dir = script_path.parent
    else:
        script_dir = script_path

    # Convert to string representation for sys.path
    script_dir_str = str(script_dir)

    # Check if the path is already in sys.path
    if script_dir_str not in sys.path:
        sys.path.insert(0, script_dir_str)
        path_added = True
    else:
        path_added = False

    try:
        yield
    finally:
        # Only remove the path if we added it
        if path_added and script_dir_str in sys.path:
            sys.path.remove(script_dir_str)
