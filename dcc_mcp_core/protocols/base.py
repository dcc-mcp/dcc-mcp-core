"""Base classes for MCP primitives.

This module provides abstract base classes for MCP Resources and Prompts.
These classes should be inherited by concrete implementations in downstream
packages (e.g., dcc-mcp-maya).

Note: Actions are already defined in dcc_mcp_core.actions.base.Action
and serve as the equivalent of MCP Tools.
"""

# Import built-in modules
from abc import ABC
from abc import abstractmethod
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import BaseModel


class Resource(ABC):
    """Abstract base class for MCP Resources.

    Resources provide read-only data to LLMs. They are similar to GET endpoints
    in a REST API - they provide data without side effects.

    Class Attributes:
        uri: URI identifying the resource (e.g., "scene://objects")
        uri_template: URI template for dynamic resources (e.g., "scene://objects/{name}")
        name: Human-readable name for the resource
        description: Description of what the resource provides
        mime_type: MIME type of the resource content
        dcc: DCC application this resource is for

    Example:
        ```python
        class SceneObjectsResource(Resource):
            uri = "scene://objects"
            name = "Scene Objects"
            description = "List of all objects in the current scene"
            mime_type = "application/json"
            dcc = "maya"

            def read(self, **params) -> str:
                import maya.cmds as cmds
                import json
                objects = cmds.ls(dag=True)
                return json.dumps(objects)
        ```

    """

    # Metadata (class variables)
    uri: ClassVar[str] = ""
    uri_template: ClassVar[str] = ""  # For dynamic resources with parameters
    name: ClassVar[str] = ""
    description: ClassVar[str] = ""
    mime_type: ClassVar[str] = "text/plain"
    dcc: ClassVar[str] = "generic"

    def __init__(self, context: Optional[Dict[str, Any]] = None):
        """Initialize the resource.

        Args:
            context: Optional dictionary of context data and dependencies

        """
        self.context = context or {}

    @abstractmethod
    def read(self, **params: Any) -> str:
        """Read the resource content.

        Args:
            **params: Parameters extracted from URI template

        Returns:
            Resource content as a string

        Raises:
            NotImplementedError: If the subclass does not implement this method

        """
        raise NotImplementedError("Subclasses must implement read method")

    async def read_async(self, **params: Any) -> str:
        """Read the resource content asynchronously.

        By default, this calls the synchronous read method.
        Subclasses can override for native async implementation.

        Args:
            **params: Parameters extracted from URI template

        Returns:
            Resource content as a string

        """
        # Import built-in modules
        import asyncio
        from concurrent.futures import ThreadPoolExecutor

        loop = asyncio.get_event_loop()
        with ThreadPoolExecutor() as pool:
            return await loop.run_in_executor(pool, lambda: self.read(**params))

    @classmethod
    def get_uri(cls) -> str:
        """Get the URI for this resource.

        Returns the static URI if defined, otherwise the URI template.

        Returns:
            Resource URI or URI template

        """
        return cls.uri or cls.uri_template

    @classmethod
    def is_template(cls) -> bool:
        """Check if this resource uses a URI template.

        Returns:
            True if the resource uses a URI template

        """
        return bool(cls.uri_template) and not cls.uri


class Prompt(ABC):
    """Abstract base class for MCP Prompts.

    Prompts provide reusable templates for LLM interactions.
    They can accept arguments to customize the generated prompt.

    Class Attributes:
        name: Unique identifier for the prompt
        description: Human-readable description of the prompt
        dcc: DCC application this prompt is for

    Example:
        ```python
        class ModelReviewPrompt(Prompt):
            name = "model_review"
            description = "Generate a prompt for reviewing a 3D model"
            dcc = "maya"

            class ArgumentsModel(Prompt.ArgumentsModel):
                object_name: str = Field(description="Name of the object to review")
                check_topology: bool = Field(True, description="Whether to check topology")

            def render(self, **kwargs) -> str:
                args = self.ArgumentsModel(**kwargs)
                prompt = f"Please review the 3D model '{args.object_name}'."
                if args.check_topology:
                    prompt += " Check the topology for issues like n-gons and non-manifold geometry."
                return prompt
        ```

    """

    # Metadata (class variables)
    name: ClassVar[str] = ""
    description: ClassVar[str] = ""
    dcc: ClassVar[str] = "generic"

    class ArgumentsModel(BaseModel):
        """Arguments model for the prompt.

        Subclasses should override this with their specific argument definitions.
        """

    def __init__(self, context: Optional[Dict[str, Any]] = None):
        """Initialize the prompt.

        Args:
            context: Optional dictionary of context data and dependencies

        """
        self.context = context or {}

    @abstractmethod
    def render(self, **kwargs: Any) -> str:
        """Render the prompt with the given arguments.

        Args:
            **kwargs: Arguments for the prompt template

        Returns:
            Rendered prompt string

        Raises:
            NotImplementedError: If the subclass does not implement this method

        """
        raise NotImplementedError("Subclasses must implement render method")

    def render_messages(self, **kwargs: Any) -> List[Dict[str, Any]]:
        """Render the prompt as a list of messages.

        By default, returns a single user message with the rendered prompt.
        Subclasses can override for multi-turn conversations.

        Args:
            **kwargs: Arguments for the prompt template

        Returns:
            List of message dictionaries with 'role' and 'content' keys

        """
        return [{"role": "user", "content": self.render(**kwargs)}]

    @classmethod
    def get_arguments_schema(cls) -> Dict[str, Any]:
        """Get the JSON schema for prompt arguments.

        Returns:
            JSON schema dictionary for the arguments model

        """
        return cls.ArgumentsModel.model_json_schema()

    @classmethod
    def get_required_arguments(cls) -> List[str]:
        """Get the list of required argument names.

        Returns:
            List of required argument names

        """
        schema = cls.get_arguments_schema()
        return schema.get("required", [])
