#!/usr/bin/env python3
"""Generate JSON Schema from Python script function signatures.

This script introspects Python functions to extract parameter information
(type annotations, defaults, docstrings) and generates a JSON Schema
compatible with MCP tool inputSchema.

Usage:
    python generate_input_schema.py <script_path> [function_name]

If function_name is not provided, it looks for:
1. Function decorated with @skill_entry
2. Function named `main`
3. Any function with **kwargs
"""

import ast
import inspect
import json
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Union
from typing import get_type_hints

# Mock common DCC modules to allow scripts to be imported without DCC
_MOCK_MODULES = [
    "maya",
    "maya.cmds",
    "hou",
    "bpy",
    "bpy.props",
    "pxr",
    "shiboken2",
    "PySide2",
    "PySide2.QtCore",
    "PySide2.QtGui",
    "PySide2.QtWidgets",
    "PyQt5",
    "PyQt5.QtCore",
    "PyQt5.QtGui",
    "PyQt5.QtWidgets",
]


def mock_dcc_modules():
    """Mock DCC modules so scripts can be imported without the actual DCC."""
    import types

    for module_name in _MOCK_MODULES:
        if module_name not in sys.modules:
            mock_module = types.ModuleType(module_name)
            sys.modules[module_name] = mock_module


def python_type_to_json_schema(py_type: Any) -> Dict[str, Any]:
    """Convert Python type annotation to JSON Schema type."""
    # Handle Optional[T] (Union[T, None])
    if hasattr(py_type, "__origin__") and py_type.__origin__ is Union:
        args = [a for a in py_type.__args__ if a is not type(None)]
        if args:
            return python_type_to_json_schema(args[0])

    # Handle List[T]
    if hasattr(py_type, "__origin__") and (py_type.__origin__ is list or py_type.__origin__ is List):
        item_schema = python_type_to_json_schema(py_type.__args__[0]) if py_type.__args__ else {"type": "string"}
        return {"type": "array", "items": item_schema}

    # Basic types
    type_map = {
        str: "string",
        int: "integer",
        float: "number",
        bool: "boolean",
        list: "array",
        dict: "object",
    }

    if py_type in type_map:
        return {"type": type_map[py_type]}

    # Default to string for unknown types
    return {"type": "string"}


def extract_docstring_params(docstring: Optional[str]) -> Dict[str, str]:
    """Extract parameter descriptions from Google/Numpy/Sphinx style docstrings."""
    if not docstring:
        return {}

    params = {}
    lines = docstring.split("\n")
    in_args = False
    current_param = None
    current_desc = []

    for line in lines:
        line_stripped = line.strip()

        # Google style: "Args:" section
        if line_stripped.startswith("Args:") or line_stripped.startswith("Parameters:"):
            in_args = True
            continue

        if in_args:
            # End of Args section
            if not line_stripped or (line_stripped and not line[0].isspace()):
                if current_param:
                    params[current_param] = " ".join(current_desc).strip()
                    current_param = None
                    current_desc = []
                in_args = False
                continue

            # Parameter line: "param_name: description" or "param_name (type): description"
            if ":" in line and not line_stripped.startswith("-"):
                if current_param:
                    params[current_param] = " ".join(current_desc).strip()
                parts = line_stripped.split(":", 1)
                current_param = parts[0].strip().split(" ")[0].strip()
                desc = parts[1].strip() if len(parts) > 1 else ""
                current_desc = [desc] if desc else []
            elif current_param and line_stripped:
                current_desc.append(line_stripped)

    if current_param:
        params[current_param] = " ".join(current_desc).strip()

    return params


def generate_schema_from_function(func) -> Dict[str, Any]:
    """Generate JSON Schema from a Python function."""
    sig = inspect.signature(func)
    type_hints = get_type_hints(func)
    docstring = inspect.getdoc(func)
    param_descriptions = extract_docstring_params(docstring)

    properties = {}
    required = []

    for name, param in sig.parameters.items():
        # Skip *args / **kwargs — they describe arbitrary call-time fan-out,
        # not declared parameters. Match by ``param.kind`` rather than name:
        # the previous ``name == "kwargs"`` guard only caught the literal
        # name ``kwargs`` and let ``def main(**_)`` through as a required
        # parameter ``_``, which then made `dispatcher.dispatch` reject
        # every call as `ValidationFailed`.
        if param.kind in (
            inspect.Parameter.VAR_POSITIONAL,
            inspect.Parameter.VAR_KEYWORD,
        ):
            continue

        prop = {}

        # Get type from type hints
        if name in type_hints:
            prop.update(python_type_to_json_schema(type_hints[name]))
        elif param.annotation != inspect.Parameter.empty:
            prop.update(python_type_to_json_schema(param.annotation))
        else:
            prop["type"] = "string"  # Default to string

        # Get description from docstring
        if name in param_descriptions:
            prop["description"] = param_descriptions[name]

        # Handle default values
        if param.default != inspect.Parameter.empty:
            prop["default"] = (
                param.default if not isinstance(param.default, (list, dict)) else json.dumps(param.default)
            )
        else:
            required.append(name)

        properties[name] = prop

    schema = {
        "type": "object",
        "properties": properties,
    }

    if required:
        schema["required"] = required

    return schema


def generate_schema_from_core(func) -> Optional[Dict[str, Any]]:
    """Use dcc_mcp_core.schema when the installed Python package is available."""
    try:
        from dcc_mcp_core.schema import tool_spec_from_callable
    except Exception:
        return None

    try:
        schema = tool_spec_from_callable(func).input_schema
    except Exception:
        return None
    if isinstance(schema, dict) and schema.get("type"):
        return schema
    return None


def find_entry_function(script_path: str) -> Optional[str]:
    """Find the entry function in a Python script without executing it.

    Uses AST parsing to find:
    1. Function decorated with @skill_entry
    2. Function named `main`
    3. Any function that looks like an entry point
    """
    with Path(script_path).open(encoding="utf-8") as f:
        source = f.read()

    try:
        tree = ast.parse(source)
    except SyntaxError:
        return None

    # Look for @skill_entry decorator
    for node in ast.walk(tree):
        if isinstance(node, ast.FunctionDef):
            for decorator in node.decorator_list:
                if isinstance(decorator, ast.Name) and decorator.id == "skill_entry":
                    return node.name
                if isinstance(decorator, ast.Attribute) and decorator.attr == "skill_entry":
                    return node.name

    # Look for main function
    for node in ast.walk(tree):
        if isinstance(node, ast.FunctionDef) and node.name == "main":
            return "main"

    # Look for any function with **kwargs
    for node in ast.walk(tree):
        if isinstance(node, ast.FunctionDef) and node.args.kwarg is not None:
            return node.name

    return None


def generate_schema(script_path: str, function_name: Optional[str] = None) -> Dict[str, Any]:
    """Generate JSON Schema from a Python script.

    This function imports the script in a clean Python environment
    (with mocked DCC modules) and introspects the specified function.
    """
    import importlib.util

    # Find entry function if not specified
    if not function_name:
        function_name = find_entry_function(script_path)
        if not function_name:
            return {"type": "object"}

    # Mock DCC modules before importing
    mock_dcc_modules()

    # Load module from file
    module_name = Path(script_path).stem
    spec = importlib.util.spec_from_file_location(module_name, script_path)
    if not spec or not spec.loader:
        return {"type": "object"}

    module = importlib.util.module_from_spec(spec)

    # Import with mocked DCC modules
    try:
        spec.loader.exec_module(module)
    except Exception as e:
        print(f"Failed to import script '{script_path}': {e}", file=sys.stderr)
        return {"type": "object"}

    # Get the function
    if not hasattr(module, function_name):
        return {"type": "object"}

    func = getattr(module, function_name)
    if not callable(func):
        return {"type": "object"}

    core_schema = generate_schema_from_core(func)
    if core_schema:
        return core_schema

    return generate_schema_from_function(func)


def main() -> None:
    """Run the script and generate JSON Schema from a Python script."""
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Usage: python generate_input_schema.py <script_path> [function_name]"}))
        sys.exit(1)

    script_path = sys.argv[1]
    function_name = sys.argv[2] if len(sys.argv) > 2 else None

    schema = generate_schema(script_path, function_name)
    print(json.dumps(schema, indent=2))


if __name__ == "__main__":
    main()
