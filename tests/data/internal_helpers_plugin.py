"""A test plugin with internal helper functions.

This plugin demonstrates how internal helper functions are not auto-registered.
"""

# Plugin metadata
__plugin_name__ = "internal_helpers_plugin"
__plugin_version__ = "1.0.0"
__plugin_description__ = "Plugin with internal helper functions"
__plugin_author__ = "Test Author"


# Internal helper function (should not be auto-registered)
def _calculate_value(base: int, multiplier: int) -> int:
    """Calculate a value using the given base and multiplier.

    Args:
        base: Base value
        multiplier: Multiplier
        
    Returns:
        Calculated value

    """
    return base * multiplier


# Public function that uses the internal helper
def get_calculated_value(base: int, multiplier: int = 2) -> dict:
    """Get a calculated value using internal helpers.
    
    Args:
        base: Base value
        multiplier: Multiplier (default: 2)
        
    Returns:
        Dictionary with the calculated value

    """
    result = _calculate_value(base, multiplier)
    
    return {
        "base": base,
        "multiplier": multiplier,
        "result": result
    }


# Another internal helper (should not be auto-registered)
def _format_result(value: int) -> str:
    """Format a result value.
    
    Args:
        value: The value to format
        
    Returns:
        Formatted string

    """
    return f"The result is: {value}"


# Public function that uses both internal helpers
def get_formatted_calculation(base: int, multiplier: int = 2) -> dict:
    """Get a formatted calculation result.
    
    Args:
        base: Base value
        multiplier: Multiplier (default: 2)
        
    Returns:
        Dictionary with the calculation and formatted result

    """
    value = _calculate_value(base, multiplier)
    formatted = _format_result(value)
    
    return {
        "calculation": f"{base} * {multiplier} = {value}",
        "formatted": formatted
    }
