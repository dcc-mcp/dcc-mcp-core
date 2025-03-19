"""A basic test plugin with complete metadata.

This plugin demonstrates the basic structure of a plugin with all metadata fields.
"""

# Plugin metadata
__plugin_name__ = "basic_plugin"
__plugin_version__ = "1.0.0"
__plugin_description__ = "A basic test plugin with complete metadata"
__plugin_author__ = "Test Author"
__plugin_requires__ = ["dependency1", "dependency2"]


def hello_world() -> str:
    """Return a simple hello world greeting.

    Returns:
        A greeting message

    """
    return "Hello, World!"


def add_numbers(a: int, b: int) -> int:
    """Add two numbers together.
    
    Args:
        a: First number
        b: Second number
        
    Returns:
        Sum of the two numbers

    """
    return a + b


def process_data(data: dict, verbose: bool = False) -> dict:
    """Process some data.
    
    Args:
        data: The data to process
        verbose: Whether to be verbose
        
    Returns:
        The processed data

    """
    result = {"processed": True}
    result.update(data)
    
    if verbose:
        result["log"] = "Processing completed successfully"
        
    return result
