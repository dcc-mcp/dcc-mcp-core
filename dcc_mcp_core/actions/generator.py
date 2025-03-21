"""Action generator module for DCC-MCP-Core.

This module provides functionality for generating action templates and new actions
based on user requirements and natural language descriptions.
"""
import os
import re
import logging
from typing import Any, Dict, List

from dcc_mcp_core.utils.filesystem import get_actions_dir
from dcc_mcp_core.utils.template import get_template, render_template
from dcc_mcp_core.models import ActionResultModel

logger = logging.getLogger(__name__)


def create_action_template(dcc_name: str, action_name: str, description: str, 
                          functions: List[Dict[str, Any]], author: str = "DCC-MCP-Core User") -> ActionResultModel:
    """Create a new action template file for a specific DCC.

    This function helps generate new action files based on user requirements.
    It creates a template file with the specified functions in the user's actions directory.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        action_name: Name of the new action
        description: Description of the action
        functions: List of function definitions, each containing:
                  - name: Function name
                  - description: Function description
                  - parameters: List of parameter dictionaries with name, type, description, default
                  - return_description: Description of what the function returns
        author: Author of the action

    Returns:
        ActionResultModel with the result of the action creation

    """
    # Normalize the DCC name
    dcc_name = dcc_name.lower()

    # Get the actions directory for this DCC
    actions_dir = get_actions_dir(dcc_name)

    # Create the actions directory if it doesn't exist
    os.makedirs(actions_dir, exist_ok=True)

    # Create the action file path
    action_file_path = os.path.join(actions_dir, f"{action_name}.py")

    # Check if the action file already exists
    if os.path.exists(action_file_path):
        return ActionResultModel(
            success=False,
            message=f"Action file already exists: {action_file_path}",
            file_path=action_file_path
        )

    # Create the action file content
    content = _generate_action_file_content(action_name, description, functions, author, dcc_name)

    # Write the action file
    try:
        with open(action_file_path, 'w') as f:
            f.write(content)

        return ActionResultModel(
            success=True,
            message=f"Created action file: {action_file_path}",
            file_path=action_file_path
        )
    except Exception as e:
        logger.error(f"Failed to create action file: {e}")
        return ActionResultModel(
            success=False,
            message=f"Failed to create action file: {e}",
            file_path=action_file_path
        )


def _generate_action_file_content(action_name: str, description: str, 
                                functions: List[Dict[str, Any]], author: str, dcc_name: str) -> str:
    """Generate the content for an action file.

    Args:
        action_name: Name of the action
        description: Description of the action
        functions: List of function definitions
        author: Author of the action
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')

    Returns:
        String containing the action file content
    """
    # Prepare the context data for the template
    context_data = {
        'action_name': action_name,
        'description': description,
        'author': author,
        'functions': functions,
        'dcc_name': dcc_name
    }
    
    # Render the template with the context data
    return render_template('action.template', context_data)


def generate_action_for_ai(dcc_name: str, action_name: str, description: str, 
                          functions_description: str) -> ActionResultModel:
    """Helper function for AI to generate new actions based on natural language descriptions.
    
    This function parses a natural language description of functions and creates an action template.
    
    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        action_name: Name of the new action
        description: Description of the action
        functions_description: Natural language description of functions to include
        
    Returns:
        ActionResultModel with the result of the action creation
    """
    try:
        # Parse the functions description
        functions = _parse_functions_description(functions_description)
        
        # Create the action template
        result = create_action_template(dcc_name, action_name, description, functions)
        
        if result.success:
            return ActionResultModel(
                success=True,
                message=f"Successfully created action template: {result.file_path}",
                prompt="You can now implement the functions in the generated template.",
                context={
                    "file_path": result.file_path,
                    "action_name": action_name,
                    "functions": [func["name"] for func in functions]
                }
            )
        else:
            return ActionResultModel(
                success=False,
                message=f"Failed to create action template: {result.message}",
                error=result.message,
                context={"file_path": result.file_path}
            )
    except Exception as e:
        logger.error(f"Failed to generate action for AI: {e}")
        return ActionResultModel(
            success=False,
            message=f"Failed to generate action: {str(e)}",
            error=str(e)
        )


def _parse_functions_description(functions_description: str) -> List[Dict[str, Any]]:
    """Parse a natural language description of functions into structured function definitions.
    
    Args:
        functions_description: Natural language description of functions
        
    Returns:
        List of function definitions
    """
    # Simple parsing for demonstration purposes
    # In a real implementation, this would use more sophisticated NLP techniques
    functions = []
    
    # Split by function indicators
    function_blocks = re.split(r'\n\s*Function\s*\d*\s*:', functions_description)
    if len(function_blocks) <= 1:
        # Try alternative splitting patterns
        function_blocks = re.split(r'\n\s*\d+\.\s*', functions_description)
        
    # Process each function block
    for block in function_blocks:
        if not block.strip():
            continue
            
        # Extract function name
        name_match = re.search(r'([a-zA-Z][a-zA-Z0-9_]*)', block)
        if not name_match:
            continue
            
        function_name = name_match.group(1)
        
        # Extract function description
        description = block.strip()
        if '\n' in description:
            description = description.split('\n')[0].strip()
        
        # Create basic function definition
        function_def = {
            "name": function_name,
            "description": description,
            "parameters": [],
            "return_description": "Returns an ActionResultModel with success status, message, and context data."
        }
        
        # Try to extract parameters
        param_matches = re.findall(r'(?:parameter|param|arg)\s*:?\s*([a-zA-Z][a-zA-Z0-9_]*)\s*(?:\(([^\)]*)\))?', block, re.IGNORECASE)
        for param_match in param_matches:
            param_name = param_match[0]
            param_type = "Any"
            param_desc = ""
            
            # Try to determine parameter type
            if param_match[1]:
                type_desc = param_match[1].lower()
                if 'int' in type_desc or 'number' in type_desc:
                    param_type = "int"
                    param_desc = "Integer parameter"
                elif 'float' in type_desc or 'decimal' in type_desc:
                    param_type = "float"
                    param_desc = "Float parameter"
                elif 'bool' in type_desc:
                    param_type = "bool"
                    param_desc = "Boolean parameter"
                elif 'str' in type_desc or 'string' in type_desc or 'text' in type_desc:
                    param_type = "str"
                    param_desc = "String parameter"
                elif 'list' in type_desc or 'array' in type_desc:
                    param_type = "List[Any]"
                    param_desc = "List parameter"
                elif 'dict' in type_desc or 'map' in type_desc:
                    param_type = "Dict[str, Any]"
                    param_desc = "Dictionary parameter"
                else:
                    param_desc = type_desc.capitalize()
            
            function_def["parameters"].append({
                "name": param_name,
                "type": param_type,
                "description": param_desc,
                "default": None
            })
        
        functions.append(function_def)
    
    # If no functions were parsed, create a default one
    if not functions:
        functions.append({
            "name": "execute_action",
            "description": "Execute the main action functionality.",
            "parameters": [],
            "return_description": "Returns an ActionResultModel with success status, message, and context data."
        })
    
    return functions
