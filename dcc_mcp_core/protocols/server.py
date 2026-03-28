"""MCP Server Protocol interface.

This module defines the protocol interface that downstream implementations
(e.g., dcc-mcp-maya) should follow to implement MCP servers.

The protocol uses Python's typing.Protocol for structural subtyping,
allowing duck typing while maintaining type safety.
"""

# Import built-in modules
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:
    from typing_extensions import Protocol
    from typing_extensions import runtime_checkable

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.protocols.types import PromptDefinition
from dcc_mcp_core.protocols.types import ResourceDefinition
from dcc_mcp_core.protocols.types import ToolDefinition


@runtime_checkable
class MCPServerProtocol(Protocol):
    """Protocol interface for MCP Server implementations.

    This protocol defines the interface that downstream packages
    (e.g., dcc-mcp-maya, dcc-mcp-blender) should implement to provide
    MCP server functionality.

    The protocol is designed to be transport-agnostic - implementations
    can use stdio, SSE, HTTP, or any other transport mechanism.

    Example implementation:
        ```python
        from dcc_mcp_core.protocols.server import MCPServerProtocol
        from dcc_mcp_core.protocols import ToolDefinition, ResourceDefinition

        class MayaMCPServer:
            '''MCP Server implementation for Maya.'''

            def __init__(self, action_manager):
                self.action_manager = action_manager

            async def list_tools(self) -> List[ToolDefinition]:
                return self.action_manager.export_mcp_tools()

            async def call_tool(self, name: str, arguments: Dict[str, Any]) -> ActionResultModel:
                return await self.action_manager.call_action_async(name, **arguments)

            # ... implement other methods
        ```

    """

    # Server info
    @property
    def name(self) -> str:
        """Server name."""
        ...

    @property
    def version(self) -> str:
        """Server version."""
        ...

    # Tools (Actions)
    async def list_tools(self) -> List[ToolDefinition]:
        """List all available tools.

        Returns:
            List of ToolDefinition objects

        """
        ...

    async def call_tool(self, name: str, arguments: Dict[str, Any]) -> ActionResultModel:
        """Call a tool by name with the given arguments.

        Args:
            name: Name of the tool to call
            arguments: Arguments to pass to the tool

        Returns:
            ActionResultModel with the result of the tool execution

        """
        ...

    # Resources
    async def list_resources(self) -> List[ResourceDefinition]:
        """List all available resources.

        Returns:
            List of ResourceDefinition objects

        """
        ...

    async def read_resource(self, uri: str) -> str:
        """Read a resource by URI.

        Args:
            uri: URI of the resource to read

        Returns:
            Resource content as a string

        """
        ...

    # Prompts
    async def list_prompts(self) -> List[PromptDefinition]:
        """List all available prompts.

        Returns:
            List of PromptDefinition objects

        """
        ...

    async def get_prompt(self, name: str, arguments: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Get a rendered prompt by name.

        Args:
            name: Name of the prompt
            arguments: Arguments to pass to the prompt template

        Returns:
            Dictionary with 'description' and 'messages' keys

        """
        ...


@runtime_checkable
class MCPToolsProtocol(Protocol):
    """Minimal protocol for MCP Tools support only.

    Use this protocol when you only need to implement tool functionality
    without resources or prompts.
    """

    async def list_tools(self) -> List[ToolDefinition]:
        """List all available tools."""
        ...

    async def call_tool(self, name: str, arguments: Dict[str, Any]) -> ActionResultModel:
        """Call a tool by name."""
        ...


@runtime_checkable
class MCPResourcesProtocol(Protocol):
    """Minimal protocol for MCP Resources support only.

    Use this protocol when you only need to implement resource functionality.
    """

    async def list_resources(self) -> List[ResourceDefinition]:
        """List all available resources."""
        ...

    async def read_resource(self, uri: str) -> str:
        """Read a resource by URI."""
        ...


@runtime_checkable
class MCPPromptsProtocol(Protocol):
    """Minimal protocol for MCP Prompts support only.

    Use this protocol when you only need to implement prompt functionality.
    """

    async def list_prompts(self) -> List[PromptDefinition]:
        """List all available prompts."""
        ...

    async def get_prompt(self, name: str, arguments: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Get a rendered prompt by name."""
        ...
