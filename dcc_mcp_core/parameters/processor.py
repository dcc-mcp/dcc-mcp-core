"""Parameter processing utilities for the DCC-MCP ecosystem.

This module provides utilities for processing, validating, and normalizing
parameters passed between components.
"""

# Import built-in modules
import ast
import json
import logging
import re
from typing import Any
from typing import Dict
from typing import Type
from typing import Union

# Import local modules
from dcc_mcp_core.utils.constants import BOOLEAN_FLAG_KEYS

logger = logging.getLogger(__name__)


def process_parameters(params: Union[Dict[str, Any], str]) -> Dict[str, Any]:
    """Process and normalize parameters for DCC tools.

    This function handles various parameter formats and normalizes them
    to a format that can be directly used by DCC commands.

    Args:
        params: Dictionary or string of parameters to process

    Returns:
        Processed parameters dictionary

    """
    # Handle string parameters
    if isinstance(params, str):
        return parse_kwargs_string(params)

    if not isinstance(params, dict):
        logger.warning(f"Expected dict or string for params, got {type(params)}. Returning empty dict.")
        return {}

    processed_params = {}

    # Handle special 'kwargs' parameter if present
    if "kwargs" in params and isinstance(params["kwargs"], str):
        kwargs_str = params["kwargs"]
        logger.debug(f"Processing string kwargs: {kwargs_str}")

        # Parse the kwargs string
        parsed_kwargs = parse_kwargs_string(kwargs_str)
        if parsed_kwargs:
            # Create a copy of the original params without the 'kwargs' key
            processed_params = {k: v for k, v in params.items() if k != "kwargs"}
            # Add the parsed kwargs
            processed_params.update(parsed_kwargs)
            logger.debug(f"Processed parameters: {processed_params}")
            return processed_params

    # Process each parameter
    for key, value in params.items():
        processed_params[key] = process_parameter_value(key, value)

    return processed_params


def process_parameter_value(key: str, value: Any) -> Any:
    """Process a single parameter value based on its name and value.

    Args:
        key: Parameter name
        value: Parameter value

    Returns:
        Processed parameter value

    """
    # Handle boolean values
    if key in BOOLEAN_FLAG_KEYS:
        if isinstance(value, (int, float)) and (value == 0 or value == 1):
            return bool(value)
        elif isinstance(value, str):
            return process_boolean_parameter(value)

    # Handle string values that might need conversion
    if isinstance(value, str):
        # Try to convert strings that look like lists or dicts
        if (value.startswith("[") and value.endswith("]")) or (value.startswith("{") and value.endswith("}")):
            try:
                return json.loads(value)
            except json.JSONDecodeError:
                try:
                    return ast.literal_eval(value)
                except (SyntaxError, ValueError):
                    # Keep as string if conversion fails
                    pass
        else:
            # Try simple type conversion for strings
            return process_string_parameter(value)

    # Return the value as is for other types
    return value


def parse_kwargs_string(kwargs_str: str) -> Dict[str, Any]:
    """Parse a string representation of kwargs into a dictionary.

    This function tries multiple parsing methods to handle different
    string formats that might be passed as kwargs.

    Args:
        kwargs_str: String representation of kwargs

    Returns:
        Dictionary of parsed kwargs or empty dict if parsing failed

    """
    # Input validation
    if not kwargs_str or not isinstance(kwargs_str, str):
        return {}

    # Clean up the string
    kwargs_str = kwargs_str.strip()

    # Try JSON parsing first
    try:
        # Try to parse as a JSON object directly
        return parse_json(kwargs_str)
    except json.JSONDecodeError:
        # If that fails, try to wrap it in curly braces
        try:
            if not kwargs_str.startswith("{"):
                modified_str = "{" + kwargs_str + "}"
                return parse_json(modified_str)
        except json.JSONDecodeError:
            pass

    # Try ast.literal_eval
    try:
        # Try to convert the string to a valid Python dictionary expression
        if not kwargs_str.startswith("{"):
            # Replace equals with colons for key-value pairs
            modified_str = "{" + kwargs_str.replace("=", ":") + "}"
        else:
            modified_str = kwargs_str

        # Use ast.literal_eval for safe evaluation
        return parse_ast_literal(modified_str)
    except (SyntaxError, ValueError):
        pass

    # Simple key=value parsing as last resort
    return parse_key_value_pairs(kwargs_str)


def parse_json(kwargs_str: str) -> Dict[str, Any]:
    """Parse a JSON string into a dictionary.

    Args:
        kwargs_str: JSON string to parse

    Returns:
        Dictionary parsed from JSON string

    Raises:
        json.JSONDecodeError: If the string is not valid JSON

    """
    return json.loads(kwargs_str)


def parse_ast_literal(kwargs_str: str) -> Dict[str, Any]:
    """Parse a string of key=value pairs into a dictionary using ast.literal_eval.

    Args:
        kwargs_str: String to parse

    Returns:
        Parsed dictionary

    Raises:
        SyntaxError: If the string is not a valid Python literal
        ValueError: If the parsed result is not a dictionary

    """
    try:
        # First try to parse as a standard Python literal
        result = ast.literal_eval(kwargs_str)
        if isinstance(result, dict):
            return result
        raise ValueError(f"Parsed result is not a dictionary: {result}")
    except (ValueError, SyntaxError) as e:
        # If it's a SyntaxError, re-raise it directly
        if isinstance(e, SyntaxError):
            raise
        # If it's a ValueError from ast.literal_eval, check if it's due to invalid syntax
        if kwargs_str.startswith("{") and kwargs_str.endswith("}") and ":" in kwargs_str:
            # This looks like it was trying to be a dict but had invalid syntax
            raise SyntaxError(f"Invalid Python literal: {kwargs_str}") from e
        # Otherwise, re-raise the original error
        raise


def parse_key_value_pairs(kwargs_str: str) -> Dict[str, Any]:
    """Parse a string of key=value pairs into a dictionary.

    Args:
        kwargs_str: String of key=value pairs

    Returns:
        Dictionary parsed from key=value pairs

    """
    result = {}
    # Use regex to handle quoted values with spaces
    # Match patterns like: key="value with spaces" or key=value
    pattern = r'(\w+)=(["\']([^"\']*)["\'](\s+|$)|[^\s]+)'
    matches = re.findall(pattern, kwargs_str)

    for match in matches:
        key = match[0]
        # If value is quoted, use the content inside quotes
        if match[2]:  # This is the group inside quotes
            value = match[2]
        else:  # This is the non-quoted value
            value = match[1]
            # Handle quoted values that weren't captured by the regex group
            if value.startswith('"') and value.endswith('"'):
                value = value[1:-1]
            elif value.startswith("'") and value.endswith("'"):
                value = value[1:-1]
            # Handle None
            elif value == "None":
                value = None
            # Handle booleans
            elif value.lower() == "true":
                value = True
            elif value.lower() == "false":
                value = False
            # Handle numeric values
            elif value.isdigit():
                value = int(value)
            elif value.replace(".", "", 1).isdigit() and value.count(".") == 1:
                value = float(value)
            # Handle boolean flags (0/1 for common flags)
            elif key in BOOLEAN_FLAG_KEYS:
                value = value == "1"

        result[key] = value

    return result


def process_string_parameter(value: str) -> Any:
    """Process a string parameter and convert it to the appropriate type.

    Args:
        value: String value to process

    Returns:
        Processed value of the appropriate type

    """
    # Remove quotes if present
    if (value.startswith('"') and value.endswith('"')) or (value.startswith("'") and value.endswith("'")):
        value = value[1:-1]

    # Try to convert to appropriate type
    if value.lower() in ("true", "yes", "y", "on"):
        return True
    elif value.lower() in ("false", "no", "n", "off"):
        return False
    elif value.isdigit():
        return int(value)
    elif value.replace(".", "", 1).isdigit() and value.count(".") == 1:
        return float(value)
    elif value.lower() == "none" or value.lower() == "null":
        return None
    else:
        return value


def process_boolean_parameter(value: Any) -> bool:
    """Process a parameter and convert it to a boolean value.

    Args:
        value: Value to convert to boolean

    Returns:
        Boolean representation of the value

    """
    if isinstance(value, bool):
        return value
    elif isinstance(value, (int, float)):
        return bool(value)
    elif isinstance(value, str):
        value_lower = value.lower()
        if value_lower in ("true", "yes", "y", "on", "1"):
            return True
        elif value_lower in ("false", "no", "n", "off", "0"):
            return False
    # Default to False for anything else
    return False


def safe_convert_type(value: Any, target_type: Type) -> Any:
    """Safely convert a value to the target type.

    This function attempts to convert a value to the specified type,
    falling back to the original value if conversion fails.

    Args:
        value: The value to convert
        target_type: The type to convert to

    Returns:
        The converted value or the original value if conversion fails

    """
    if value is None:
        return None

    # If the value is already of the target type, return it
    if isinstance(value, target_type):
        return value

    try:
        # Handle common type conversions
        if isinstance(target_type, type) and target_type is bool:
            return process_boolean_parameter(value)
        elif isinstance(target_type, type) and target_type is int:
            return int(float(value)) if isinstance(value, str) else int(value)
        elif isinstance(target_type, type) and target_type is float:
            return float(value)
        elif isinstance(target_type, type) and target_type is str:
            return str(value)
        elif isinstance(target_type, type) and target_type is list:
            if isinstance(value, str):
                if value.startswith("[") and value.endswith("]"):
                    return json.loads(value)
                else:
                    return [item.strip() for item in value.split(",")]
            elif isinstance(value, (tuple, set)):
                return list(value)
            else:
                return [value]
        elif isinstance(target_type, type) and target_type is dict:
            if isinstance(value, str):
                return parse_kwargs_string(value)
            else:
                return dict(value)
        else:
            # For other types, try direct conversion
            return target_type(value)
    except (ValueError, TypeError, json.JSONDecodeError) as e:
        logger.debug(f"Failed to convert {value} to {target_type.__name__}: {e}")
        return value
