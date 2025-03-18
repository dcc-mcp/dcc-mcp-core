"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem."""

from importlib.metadata import version, PackageNotFoundError

# Import submodules to make them available at the package level
from dcc_mcp_core import exceptions
from dcc_mcp_core import logg_config
from dcc_mcp_core import parameters
from dcc_mcp_core import filesystem
from dcc_mcp_core import plugin_manager


__all__ = [
    "exceptions",
    "logg_config",
    "parameters",
    "filesystem",
    "plugin_manager",
]
