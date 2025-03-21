"""Example demonstrating the use of utility modules in DCC-MCP-Core.

This example shows how to use various utility functions from the restructured codebase.
"""

import logging
import sys
from pathlib import Path

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)

# Add the parent directory to the Python path
sys.path.append('..')

# Import DCC-MCP-Core modules
from dcc_mcp_core.utils import (
    # Platform utilities
    get_platform_dir, get_config_dir, get_data_dir, get_log_dir, get_actions_dir,
    
    # Decorators
    error_handler, format_result,
    
    # Exceptions
    MCPError, ValidationError
)
from dcc_mcp_core.utils.template import render_template


# Example using platform utilities
def show_platform_directories():
    """Display platform-specific directories used by DCC-MCP-Core."""
    logger.info("Platform-specific directories:")
    logger.info(f"Config directory: {get_config_dir()}")
    logger.info(f"Data directory: {get_data_dir()}")
    logger.info(f"Log directory: {get_log_dir()}")
    logger.info(f"Maya actions directory: {get_actions_dir('maya')}")
    logger.info(f"Houdini actions directory: {get_actions_dir('houdini')}")


# Example using decorators
@error_handler
def function_that_might_fail(value):
    """A function that might raise an exception."""
    if value < 0:
        raise ValidationError("Value must be positive")
    return value * 2


@format_result
def function_with_formatted_result(name, count=1):
    """A function that returns a formatted result."""
    return {
        "success": True,
        "message": f"Successfully processed {name}",
        "context": {
            "name": name,
            "count": count,
            "processed": True
        }
    }


# Example using template rendering
def render_example_template():
    """Render a simple template using the template utilities."""
    # Create a temporary template file
    template_dir = Path("./temp_templates")
    template_dir.mkdir(exist_ok=True)
    
    template_file = template_dir / "example.template"
    template_content = """Hello, {{ name }}!

This is a simple template example for {{ project }}.

Features:
{% for feature in features %}
- {{ feature }}
{% endfor %}
"""
    
    template_file.write_text(template_content)
    
    # Render the template
    context = {
        "name": "User",
        "project": "DCC-MCP-Core",
        "features": ["Easy to use", "Modular design", "AI-friendly"]
    }
    
    rendered = render_template("example.template", context, template_dir=str(template_dir))
    logger.info("Rendered template:")
    logger.info(rendered)
    
    # Clean up
    template_file.unlink()
    template_dir.rmdir()


def main():
    """Run the example."""
    logger.info("=== DCC-MCP-Core Utilities Example ===")
    
    # Show platform directories
    logger.info("\n1. Platform Directories:")
    show_platform_directories()
    
    # Demonstrate decorators
    logger.info("\n2. Decorator Examples:")
    
    # Error handler decorator example
    logger.info("Using @error_handler decorator:")
    try:
        result = function_that_might_fail(5)
        logger.info(f"Success: {result}")
    except Exception as e:
        logger.error(f"Error: {e}")
    
    try:
        result = function_that_might_fail(-5)
        logger.info(f"Success: {result}")
    except Exception as e:
        logger.error(f"Error: {e}")
    
    # Format result decorator example
    logger.info("\nUsing @format_result decorator:")
    result = function_with_formatted_result("test_item", 3)
    logger.info(f"Formatted result: {result}")
    
    # Template rendering example
    logger.info("\n3. Template Rendering:")
    render_example_template()
    
    logger.info("\n=== Example Complete ===")


if __name__ == "__main__":
    main()
