"""MCP Protocol definitions and base classes.

This package provides:
- MCP type definitions (Tool, Resource, Prompt schemas)
- Abstract base classes for MCP primitives
- Protocol interfaces for downstream implementations
- Adapters for converting dcc-mcp-core primitives to MCP types
"""

# Import local modules
from dcc_mcp_core.protocols.adapter import MCPAdapter
from dcc_mcp_core.protocols.base import Prompt
from dcc_mcp_core.protocols.base import Resource
from dcc_mcp_core.protocols.server import MCPPromptsProtocol
from dcc_mcp_core.protocols.server import MCPResourcesProtocol
from dcc_mcp_core.protocols.server import MCPServerProtocol
from dcc_mcp_core.protocols.server import MCPToolsProtocol
from dcc_mcp_core.protocols.types import PromptArgument
from dcc_mcp_core.protocols.types import PromptDefinition
from dcc_mcp_core.protocols.types import ResourceDefinition
from dcc_mcp_core.protocols.types import ResourceTemplateDefinition
from dcc_mcp_core.protocols.types import ToolAnnotations
from dcc_mcp_core.protocols.types import ToolDefinition

__all__ = [
    # Adapter
    "MCPAdapter",
    "MCPPromptsProtocol",
    "MCPResourcesProtocol",
    # Protocol interfaces
    "MCPServerProtocol",
    "MCPToolsProtocol",
    "Prompt",
    "PromptArgument",
    "PromptDefinition",
    # Base classes
    "Resource",
    "ResourceDefinition",
    "ResourceTemplateDefinition",
    "ToolAnnotations",
    # Type definitions
    "ToolDefinition",
]
