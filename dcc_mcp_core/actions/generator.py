"""Action generator module for DCC-MCP-Core.

This module provides functionality for generating action templates and new actions
based on user requirements and natural language descriptions.
"""

# Import built-in modules
from datetime import datetime
import logging
import os
import re
from typing import Any
from typing import Dict
from typing import List

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.filesystem import get_actions_dir
from dcc_mcp_core.utils.template import render_template

logger = logging.getLogger(__name__)


def create_action_template(
    dcc_name: str,
    action_name: str,
    description: str,
    functions: List[Dict[str, Any]],
    author: str = "DCC-MCP-Core User",
) -> ActionResultModel:
    """Create a new Action class template file.

    This function generates a new Python file containing one or more Action classes
    based on the provided specifications. Each Action class follows the new class-based
    design pattern with Pydantic models for input validation and structured output.

    Args:
        dcc_name: Name of the DCC tool (e.g., 'maya', 'nuke')
        action_name: Name of the action
        description: Description of the action
        functions: List of Action class definitions
        author: Author of the action

    Returns:
        ActionResultModel with success status, message, and file path in context

    """
    try:
        # Get the actions directory for the specified DCC
        actions_dir = get_actions_dir(dcc_name)

        # Create the directory if it doesn't exist
        os.makedirs(actions_dir, exist_ok=True)

        # Define the action file path
        action_file_path = os.path.join(actions_dir, f"{action_name}.py")

        # Check if the file already exists
        if os.path.exists(action_file_path):
            return ActionResultModel(
                success=False,
                message=f"Action class file already exists: {action_file_path}",
                context={"file_path": action_file_path},
            )

        # Generate the action file content
        content = _generate_action_content(action_name, description, functions, author, dcc_name)

        # Write the content to the file
        with open(action_file_path, "w") as f:
            f.write(content)

        return ActionResultModel(
            success=True,
            message=f"Created Action class file: {action_file_path}",
            context={"file_path": action_file_path},
        )
    except Exception as e:
        logger.error(f"Failed to create Action class template: {e}")
        return ActionResultModel(
            success=False,
            message=f"Failed to create Action class file: {e!s}",
            context={"file_path": action_file_path if "action_file_path" in locals() else None, "error": str(e)},
        )


def _generate_action_content(
    action_name: str, description: str, functions: List[Dict[str, Any]], author: str, dcc_name: str
) -> str:
    """Generate the content for an action file.

    Args:
        action_name: Name of the action
        description: Description of the action
        functions: List of Action class definitions
        author: Author of the action
        dcc_name: Name of the DCC tool

    Returns:
        Generated content for the action file with Action classes

    """
    # Create the context data for the template
    context_data = {
        "action_name": action_name,
        "description": description,
        "functions": functions,
        "author": author,
        "dcc_name": dcc_name,
        "date": datetime.now().strftime("%Y-%m-%d"),
    }

    # Render the template with the context data
    return render_template("action.template", context_data)


def generate_action_for_ai(
    dcc_name: str, action_name: str, description: str, functions_description: str
) -> ActionResultModel:
    """Generate an Action class template from a natural language description of functions.

    This function creates a new Action class template file based on a natural language
    description of the functionality. It uses a simple parsing approach to extract
    function names, descriptions, and parameters from the text description.

    Args:
        dcc_name: Name of the DCC tool (e.g., 'maya', 'nuke')
        action_name: Name of the action
        description: Description of the action
        functions_description: Natural language description of functions

    Returns:
        ActionResultModel with success status, message, and context

    """
    try:
        # Parse the functions description
        functions = _parse_functions_description(functions_description)

        # Create the action template
        result = create_action_template(dcc_name, action_name, description, functions)

        # Create the response
        if result.success:
            return ActionResultModel(
                success=True,
                message=f"Successfully created Action class template for {action_name}",
                prompt="You can now implement the Action classes in the generated file.",
                context={
                    "file_path": result.context.get("file_path"),
                    "action_name": action_name,
                    "functions": functions,
                },
            )
        else:
            error_message = result.message
            return ActionResultModel(
                success=False,
                message=f"Failed to create Action class template for {action_name}",
                error=error_message,
                context={"file_path": result.context.get("file_path"), "error": result.context.get("error")},
            )
    except Exception as e:
        return ActionResultModel(
            success=False,
            message=f"Error generating Action class template: {e!s}",
            error=str(e),
            context={"error": str(e)},
        )


def _parse_functions_description(functions_description: str) -> List[Dict[str, Any]]:
    """Parse a natural language description of functions into structured Action class definitions.

    This method extracts information about Action classes from a natural language description,
    identifying class names, descriptions, and parameters. The resulting structure is used to
    generate Action class templates with proper input validation and output models.

    Args:
        functions_description: Natural language description of functions or Action classes

    Returns:
        List of Action class definitions with parameters and metadata

    """
    # Simple parsing for demonstration purposes
    # In a real implementation, this would use more sophisticated NLP techniques
    functions = []

    # Split by function indicators
    function_blocks = re.split(r"\n\s*Function\s*\d*\s*:", functions_description)
    if len(function_blocks) <= 1:
        # Try alternative splitting patterns
        function_blocks = re.split(r"\n\s*\d+\.\s*", functions_description)

    # Process each function block
    for block in function_blocks:
        if not block.strip():
            continue

        # Extract function name (will be used as Action class name)
        name_match = re.search(r"([a-zA-Z][a-zA-Z0-9_]*)", block)
        if not name_match:
            continue

        function_name = name_match.group(1)

        # Extract function description - get the full description from the block
        lines = [line.strip() for line in block.split("\n") if line.strip()]
        description = ""

        # Find the first line after the function name that looks like a description
        for line in lines:
            # Check if line is not a function name or parameter line
            if (
                function_name not in line
                and not line.startswith("Parameter:")
                and not line.startswith("param:")
                and not line.startswith("arg:")
            ):
                description = line
                break

        if not description and len(lines) > 0:
            description = lines[0]

        # Create basic Action class definition
        function_def = {
            "name": function_name,
            "description": description,
            "parameters": [],
            "return_description": "Returns an ActionResultModel with success status, message, and context data.",
        }

        # Try to extract all possible parameter formats
        # First try matching parameter lines with prefix (param:, parameter:, arg:)
        param_matches = re.findall(
            r"(?:parameter|param|arg|Parameter)\s*:?\s*([a-zA-Z][a-zA-Z0-9_]*)\s*(?:\(([^\)]*)\))?\s*-?\s*(.*)?",
            block,
            re.IGNORECASE,
        )

        # Try to match parameters in the format "param_name (type) - description"
        additional_matches = re.findall(
            r"\b([a-zA-Z][a-zA-Z0-9_]*)\s*\(([^\)]*)\)\s*-\s*(.*)",
            block,
            re.IGNORECASE,
        )

        # Merge all matching results, ensure no duplicates
        seen_params = set()
        all_matches = []

        for match in param_matches + additional_matches:
            param_name = match[0]
            if param_name not in seen_params:
                seen_params.add(param_name)
                all_matches.append(match)

        param_matches = all_matches

        for param_match in param_matches:
            param_name = param_match[0]
            param_type = "Any"
            param_desc = param_match[2].strip() if len(param_match) > 2 and param_match[2].strip() else ""

            # Try to determine parameter type
            if len(param_match) > 1 and param_match[1]:
                type_desc = param_match[1].lower()
                if "int" in type_desc or "number" in type_desc:
                    param_type = "int"
                    if not param_desc:
                        param_desc = "Integer parameter"
                elif "float" in type_desc or "decimal" in type_desc:
                    param_type = "float"
                    if not param_desc:
                        param_desc = "Float parameter"
                elif "bool" in type_desc:
                    param_type = "bool"
                    if not param_desc:
                        param_desc = "Boolean parameter"
                elif "str" in type_desc or "string" in type_desc or "text" in type_desc:
                    param_type = "str"
                    if not param_desc:
                        param_desc = "String parameter"
                elif "list" in type_desc or "array" in type_desc:
                    param_type = "List[Any]"
                    if not param_desc:
                        param_desc = "List parameter"
                elif "dict" in type_desc or "map" in type_desc:
                    param_type = "Dict[str, Any]"
                    if not param_desc:
                        param_desc = "Dictionary parameter"
                else:
                    if not param_desc:
                        param_desc = type_desc.capitalize()

            function_def["parameters"].append(
                {"name": param_name, "type": param_type, "description": param_desc, "default": None}
            )

        functions.append(function_def)

    # If no functions were parsed, create a default one
    if not functions:
        functions.append(
            {
                "name": "Execute",
                "description": "Execute the main action functionality.",
                "parameters": [],
                "return_description": "Returns an ActionResultModel with success status, message, and context data.",
            }
        )

    return functions
