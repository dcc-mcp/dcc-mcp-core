"""Base Action class for DCC-MCP-Core.

This module provides the base Action class that all DCC-specific actions should inherit from.
It implements a structured approach to action definition, input validation, and execution.
"""

# Import built-in modules
import asyncio
from concurrent.futures import ThreadPoolExecutor
import logging
import traceback
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional
from typing import Type
from typing import Union

# Import third-party modules
from pydantic import BaseModel
from pydantic import Field
from pydantic import create_model

# Import local modules
from dcc_mcp_core.constants import DEFAULT_DCC
from dcc_mcp_core.models import ActionResultModel


class Action:
    """Base Action class for DCC operations.

    All DCC-specific actions should inherit from this class.
    It provides a structured approach to action definition, input validation, and execution.

    Class Attributes:
        name: Name of the action
        description: Description of the action
        order: Execution order (lower numbers execute first)
        tags: Tags for categorizing the action
        dcc: DCC application this action is for (e.g., 'maya', 'houdini')
    """

    # Metadata (class variables)
    name: ClassVar[str] = ""
    description: ClassVar[str] = ""
    order: ClassVar[int] = 0
    tags: ClassVar[List[str]] = []
    dcc: ClassVar[str] = DEFAULT_DCC

    # Input parameters model (using Pydantic)
    class InputModel(BaseModel):
        """Input parameters model.

        This model defines and validates the input parameters for the action.
        Subclasses should override this with their specific parameter definitions.

        Examples:
            ```python
            class InputModel(Action.InputModel):
                radius: float = Field(1.0, description="Radius of the sphere")
                position: List[float] = Field([0, 0, 0], description="Position of the sphere")
                name: Optional[str] = Field(None, description="Name of the sphere")

                # Parameter validation example
                @field_validator('radius')
                def validate_radius(cls, v):
                    if v <= 0:
                        raise ValueError("Radius must be positive")
                    return v

                # Parameter dependency example
                @model_validator(mode='after')
                def validate_dependencies(self, info: ValidationInfo):
                    # Example: if name is provided, position must also be provided
                    if self.name and not self.position:
                        raise ValueError("Position must be provided when name is specified")
                    return self
            ```

        """

    # Output context model (using Pydantic)
    class OutputModel(BaseModel):
        """Output context model.

        This model defines the structure of the context data returned by the action.
        Subclasses should override this with their specific output definitions.

        Attributes:
            prompt: Optional prompt for AI to guide next steps

        Examples:
            ```python
            class OutputModel(Action.OutputModel):
                object_name: str = Field(description="Name of the created object")
                position: List[float] = Field(description="Final position of the object")
                scene_stats: Dict[str, Any] = Field(default_factory=dict, description="Scene statistics")
            ```

        """

        prompt: Optional[str] = Field(None, description="Suggestion for AI about next steps or actions")

    def __init__(self, context: Optional[Dict[str, Any]] = None):
        """Initialize the action.

        Args:
            context: Optional dictionary of context data and dependencies

        """
        self.input = None
        self.output = None
        self.context = context or {}

    def setup(self, **kwargs) -> "Action":
        """Set up the environment and validate input parameters.

        This method validates the input parameters using the InputModel and
        sets up any necessary context before the action is executed.

        Args:
            **kwargs: Input parameters for the action

        Returns:
            self: Returns self for method chaining

        """
        # Validate input using InputModel and store as an attribute
        self.input = self.validate_input(**kwargs)

        # Initialize output model
        self.output = None

        # Setup additional context if needed
        self._setup_context()

        return self

    def _setup_context(self) -> None:
        """Set up additional context and dependencies.

        This method can be overridden by subclasses to set up any additional
        context or dependencies that are needed for the action to execute.
        By default, it does nothing.
        """

    def validate_input(self, **kwargs) -> InputModel:
        """Validate input parameters using the InputModel.

        This method creates an instance of the InputModel with the given parameters,
        which automatically validates them according to the model's field definitions.

        Args:
            **kwargs: Input parameters to validate

        Returns:
            InputModel: Validated input model instance

        Raises:
            ValidationError: If the input parameters are invalid

        """
        try:
            return self.InputModel(**kwargs)
        except Exception as e:
            # Log validation error and re-raise
            logging.getLogger(__name__).error(f"Input validation error: {e}")
            raise

    def process(self) -> ActionResultModel:
        """Process the action with the given parameters.

        This method handles execution and error handling. It uses the self.input
        attribute that was set during the setup method.

        Returns:
            ActionResultModel: Structured result of the action execution

        """
        try:
            # Execute the action (implemented by subclasses)
            self._execute()

            # If output model was set, use it to populate the result
            if self.output and isinstance(self.output, self.OutputModel):
                context_data = self.output.model_dump(exclude={"prompt"})
                prompt = self.output.prompt
            else:
                context_data = {}
                prompt = None

            # Create and return the result
            return ActionResultModel(
                success=True, message=f"Successfully executed {self.name}", prompt=prompt, context=context_data
            )
        except Exception as e:
            # Log the error
            logger = logging.getLogger(__name__)
            logger.error(f"Error executing {self.name}: {e}")
            logger.debug(traceback.format_exc())

            # Create and return the error result
            return ActionResultModel(
                success=False,
                message=f"Failed to execute {self.name}",
                error=str(e),
                context={"traceback": traceback.format_exc()},
            )

    def _execute(self) -> None:
        """Execute the action with validated parameters.

        This method should be implemented by subclasses to perform the actual action.
        It should use self.input for input parameters and set self.output with an
        instance of OutputModel to provide structured output data.

        Returns:
            None

        Raises:
            NotImplementedError: If the subclass does not implement this method

        """
        raise NotImplementedError("Subclasses must implement _execute method")

    async def process_async(self) -> ActionResultModel:
        """Process the action asynchronously with the given parameters.

        This method handles execution and error handling in an asynchronous context.
        It uses the self.input attribute that was set during the setup method.

        Returns:
            ActionResultModel: Structured result of the action execution

        """
        try:
            # Execute the action asynchronously (implemented by subclasses)
            await self._execute_async()

            # If output model was set, use it to populate the result
            if self.output and isinstance(self.output, self.OutputModel):
                context_data = self.output.model_dump(exclude={"prompt"})
                prompt = self.output.prompt
            else:
                context_data = {}
                prompt = None

            # Create and return the result
            return ActionResultModel(
                success=True, message=f"Successfully executed {self.name}", prompt=prompt, context=context_data
            )
        except Exception as e:
            # Log the error
            logger = logging.getLogger(__name__)
            logger.error(f"Error executing {self.name} asynchronously: {e}")
            logger.debug(traceback.format_exc())

            # Create and return the error result
            return ActionResultModel(
                success=False,
                message=f"Failed to execute {self.name} asynchronously",
                error=str(e),
                context={"traceback": traceback.format_exc()},
            )

    async def _execute_async(self) -> None:
        """Execute the action asynchronously with validated parameters.

        By default, this method runs the synchronous _execute method in a thread pool.
        Subclasses can override this method to provide a native asynchronous implementation.

        Returns:
            None

        Raises:
            NotImplementedError: If the subclass does not implement _execute method

        """
        # Run the synchronous _execute method in a thread pool
        loop = asyncio.get_event_loop()
        with ThreadPoolExecutor() as pool:
            await loop.run_in_executor(pool, self._execute)

    @classmethod
    def create_parameter_model(cls, **field_definitions) -> Type[BaseModel]:
        """Create a Pydantic model for parameters dynamically.

        This utility method allows for creating parameter models dynamically,
        which can be useful for complex parameter validation scenarios.

        Args:
            **field_definitions: Field definitions for the model

        Returns:
            Type[BaseModel]: A new Pydantic model class

        Examples:
            ```python
            # Create a parameter model dynamically
            params_model = Action.create_parameter_model(
                radius=Field(1.0, description="Radius of the sphere"),
                position=Field([0, 0, 0], description="Position of the sphere"),
                name=Field(None, description="Name of the sphere")
            )

            # Use the model to validate parameters
            params = params_model(radius=2.0, position=[1, 2, 3])
            ```

        """
        return create_model("DynamicModel", **field_definitions)

    @staticmethod
    def process_parameters(params: Union[Dict[str, Any], str]) -> Dict[str, Any]:
        """Process and normalize parameters for DCC tools.

        This function handles various parameter formats and normalizes them
        to a format that can be directly used by DCC commands.

        Args:
            params: Dictionary or string of parameters to process

        Returns:
            Processed parameters dictionary

        """
        # Handle string parameters
        if isinstance(params, str):
            try:
                # Try to parse as JSON
                # Import built-in modules
                import json

                return json.loads(params)
            except json.JSONDecodeError:
                # Try to parse as key=value pairs
                result = {}
                for pair in params.split(","):
                    if "=" in pair:
                        key, value = pair.split("=", 1)
                        result[key.strip()] = Action.process_parameter_value(value.strip())
                return result

        if not isinstance(params, dict):
            logging.getLogger(__name__).warning(
                f"Expected dict or string for params, got {type(params)}. Returning empty dict."
            )
            return {}

        return params

    @staticmethod
    def process_parameter_value(value: Any) -> Any:
        """Process a parameter value to convert it to the appropriate type.

        Args:
            value: Value to process

        Returns:
            Processed value of the appropriate type

        """
        if not isinstance(value, str):
            return value

        # Try to convert to appropriate type
        value_str = value.strip()

        # Boolean values
        if value_str.lower() in ("true", "yes", "1"):
            return True
        if value_str.lower() in ("false", "no", "0"):
            return False

        # None values
        if value_str.lower() in ("none", "null"):
            return None

        # Numeric values
        try:
            # Try integer first
            return int(value_str)
        except ValueError:
            try:
                # Then try float
                return float(value_str)
            except ValueError:
                # Keep as string
                return value_str
