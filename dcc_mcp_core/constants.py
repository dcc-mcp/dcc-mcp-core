"""Constants for the DCC-MCP ecosystem.

This module provides centralized constants that are used throughout the DCC-MCP ecosystem.
"""

# Application information
APP_NAME = "dcc-mcp"
APP_AUTHOR = "dcc-mcp"

# Logging
LOG_APP_NAME = "dcc-mcp-core"
DEFAULT_LOG_LEVEL = "DEBUG"

# Environment variables
ENV_LOG_LEVEL = "MCP_LOG_LEVEL"
ENV_PLUGIN_PATH_PREFIX = "DCC_MCP_PLUGIN_PATH_"

# File names
PLUGIN_PATHS_CONFIG = "plugin_paths.json"

# Boolean flag keys for parameter processing
BOOLEAN_FLAG_KEYS = [
    'query', 'q', 'edit', 'e', 'select', 'sl', 'selection',
    'visible', 'v', 'hidden', 'h'
]

# Plugin metadata configuration
PLUGIN_METADATA = {
    "name": {
        "attr": "__plugin_name__",
        "default": None  # Will use plugin_name as default
    },
    "version": {
        "attr": "__plugin_version__",
        "default": "unknown"
    },
    "description": {
        "attr": "__plugin_description__",
        "default": ""
    },
    "author": {
        "attr": "__plugin_author__",
        "default": "unknown"
    },
    "requires": {
        "attr": "__plugin_requires__",
        "default": []
    }
}
