"""Template utilities for DCC-MCP-Core.

This module provides utilities for rendering templates using Jinja2.
"""

# Import built-in modules
import os
from typing import Any
from typing import Dict
from typing import Optional

# Import third-party modules
import jinja2

# Import local modules
from dcc_mcp_core.utils.filesystem import get_templates_directory


def render_template(template_name: str, context_data: Dict[str, Any], template_dir: Optional[str] = None) -> str:
    """Render a template with the given context data.

    Args:
        template_name: Name of the template file (e.g., 'action.template')
        context_data: Dictionary containing the data to render the template with
        template_dir: Optional directory path where templates are stored.
                     If None, uses the default templates directory.

    Returns:
        The rendered template as a string

    """
    # Get the template directory path if not provided
    if template_dir is None:
        template_dir = get_templates_directory()

    # Set up Jinja2 environment
    env = jinja2.Environment(loader=jinja2.FileSystemLoader(template_dir), trim_blocks=True, lstrip_blocks=True)

    # Load the template
    template = env.get_template(template_name)

    # Render the template with context
    return template.render(**context_data)


def get_template(template_name: str, template_dir: Optional[str] = None) -> str:
    """Get the contents of a template file.

    Args:
        template_name: Name of the template file (e.g., 'action.template')
        template_dir: Optional directory path where templates are stored.
                     If None, uses the default templates directory.

    Returns:
        The template content as a string

    """
    # Get the template directory path if not provided
    if template_dir is None:
        template_dir = get_templates_directory()

    # Get the template file path
    template_path = os.path.join(template_dir, template_name)

    # Read the template file
    with open(template_path) as f:
        return f.read()
