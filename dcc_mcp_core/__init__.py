"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem."""

# Import built-in modules
from importlib.metadata import PackageNotFoundError
from importlib.metadata import version

# Import local modules
# Import submodules to make them available at the package level
from dcc_mcp_core import exceptions
from dcc_mcp_core import filesystem
from dcc_mcp_core import logg_config
from dcc_mcp_core import parameters
from dcc_mcp_core import plugin_manager

__all__ = [
    "exceptions",
    "filesystem",
    "logg_config",
    "parameters",
    "plugin_manager",
]
